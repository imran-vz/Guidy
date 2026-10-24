//! Secrets (API keys), stored in the OS credential
//! store: the macOS Keychain, Windows Credential Manager, or the Linux kernel
//! keyring, through the `keyring` crate. See docs/MEETING_NOTES_ARCH.md.
//!
//! Every secret is a string under one key name, in the service
//! [`SERVICE`]. Use the key-name helpers ([`provider_api_key`],
//! [`LINEAR_API_KEY`]) rather than raw strings.
//!
//! [`MEETINGS_DATA_KEY`] is read only to convert meeting data an earlier
//! version encrypted with it ([`crate::meetings::legacy`]); once converted,
//! [`retire_meetings_data_key`] keeps it under a backup name and nothing
//! creates a new one.
//!
//! Unit tests (`cfg(test)` or the `test-support` feature) use an in-memory
//! backend, so they never touch the real Keychain. Debug builds also honour
//! `GUIDY_SECRETS_BACKEND=memory`; release builds ignore it, so an environment
//! variable can't silently divert keys away from the Keychain.
//! [`set_backend`] swaps the backend explicitly.
//!
//! Keychain reads are cached in-process (including misses, except for the
//! meetings data key), because callers such as the settings UI read keys on
//! every refresh. [`forget_meetings_data_key`] drops the cached data key.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use parking_lot::{Mutex, RwLock};

/// Keychain service name for every Guidy secret.
pub const SERVICE: &str = "dev.guidy.app";

/// Key of the Linear personal API key.
pub const LINEAR_API_KEY: &str = "linear_api_key";

/// Key of the 32-byte meetings data key (base64) earlier versions encrypted
/// `meetings.db`, the meeting audio and the processing sidecars with.
pub const MEETINGS_DATA_KEY: &str = "meetings_data_key";

/// Env var that selects the backend: `memory` forces the in-memory one.
/// Honoured only in debug / test / `test-support` builds.
pub const BACKEND_ENV: &str = "GUIDY_SECRETS_BACKEND";

/// Key of an LLM provider's API key (`post_process_providers` id).
pub fn provider_api_key(provider_id: &str) -> String {
    format!("provider_api_key.{provider_id}")
}

/// A place secrets are stored.
pub trait SecretsBackend: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    /// Deleting a missing key is not an error.
    fn delete(&self, key: &str) -> Result<()>;
    /// Forget any cached value of `key`, so the next read asks the store.
    fn invalidate(&self, _key: &str) {}
}

/// The OS credential store through `keyring` (Keychain on macOS).
pub struct KeychainBackend {
    service: String,
}

impl KeychainBackend {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, key)
            .with_context(|| format!("open keychain entry '{key}'"))
    }
}

impl SecretsBackend for KeychainBackend {
    fn get(&self, key: &str) -> Result<Option<String>> {
        match self.entry(key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow!("read keychain entry '{key}': {e}")),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.entry(key)?
            .set_password(value)
            .map_err(|e| anyhow!("write keychain entry '{key}': {e}"))
    }

    fn delete(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow!("delete keychain entry '{key}': {e}")),
        }
    }
}

/// Process-local secrets, for tests and `GUIDY_SECRETS_BACKEND=memory`.
#[derive(Default)]
pub struct MemoryBackend {
    map: Mutex<HashMap<String, String>>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretsBackend for MemoryBackend {
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.map.lock().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.map.lock().insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.map.lock().remove(key);
        Ok(())
    }
}

/// Wraps a slow backend (the Keychain) with an in-process read cache. Reads
/// are cached, misses too (except for [`MEETINGS_DATA_KEY`], so restoring it
/// in the Keychain is seen on the next read); writes update the cache after
/// they succeed.
///
/// Backend reads run outside the lock, so every write or invalidation bumps
/// the key's generation, and a read only caches its value if the generation
/// is unchanged since it started: a slow read can't overwrite a newer write.
struct Cached<B> {
    inner: B,
    cache: Mutex<CacheState>,
}

#[derive(Default)]
struct CacheState {
    values: HashMap<String, Option<String>>,
    generations: HashMap<String, u64>,
}

impl CacheState {
    fn generation(&self, key: &str) -> u64 {
        self.generations.get(key).copied().unwrap_or(0)
    }

    /// Record a write (`Some(value)`: the stored value; `None`: unknown).
    fn written(&mut self, key: &str, value: Option<Option<String>>) {
        *self.generations.entry(key.to_string()).or_default() += 1;
        match value {
            Some(v) => self.values.insert(key.to_string(), v),
            None => self.values.remove(key),
        };
    }
}

impl<B> Cached<B> {
    fn new(inner: B) -> Self {
        Self {
            inner,
            cache: Mutex::new(CacheState::default()),
        }
    }
}

impl<B: SecretsBackend> SecretsBackend for Cached<B> {
    fn get(&self, key: &str) -> Result<Option<String>> {
        /// Reads a key that keeps changing during the read gets before the
        /// last one is returned (uncached) anyway.
        const MAX_READS: usize = 3;
        let mut reads = 0;
        loop {
            let generation = {
                let cache = self.cache.lock();
                if let Some(v) = cache.values.get(key) {
                    return Ok(v.clone());
                }
                cache.generation(key)
            };
            let v = self.inner.get(key)?;
            reads += 1;
            let mut cache = self.cache.lock();
            if cache.generation(key) == generation {
                if v.is_some() || key != MEETINGS_DATA_KEY {
                    cache.values.insert(key.to_string(), v.clone());
                }
                return Ok(v);
            }
            // Written meanwhile: that value is newer than this read. With
            // none cached (invalidated, or a failed write), read again.
            if let Some(newer) = cache.values.get(key) {
                return Ok(newer.clone());
            }
            if reads >= MAX_READS {
                return Ok(v);
            }
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        let result = self.inner.set(key, value);
        // A failed write may or may not have landed: forget the key.
        let stored = result.is_ok().then(|| Some(value.to_string()));
        self.cache.lock().written(key, stored);
        result
    }

    fn delete(&self, key: &str) -> Result<()> {
        let result = self.inner.delete(key);
        let stored = result.is_ok().then_some(None);
        self.cache.lock().written(key, stored);
        result
    }

    fn invalidate(&self, key: &str) {
        self.cache.lock().written(key, None);
    }
}

/// Whether `GUIDY_SECRETS_BACKEND` is honoured at all in this build.
const ENV_OVERRIDE_ALLOWED: bool = cfg!(any(test, debug_assertions, feature = "test-support"));

fn env_requests_memory(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.eq_ignore_ascii_case("memory"))
}

fn use_memory_by_default() -> bool {
    if cfg!(any(test, feature = "test-support")) {
        return true;
    }
    let requested = env_requests_memory(std::env::var(BACKEND_ENV).ok().as_deref());
    if requested && !ENV_OVERRIDE_ALLOWED {
        log::warn!("secrets: ignoring {BACKEND_ENV}=memory in a release build");
        return false;
    }
    requested
}

/// [`SERVICE`], or in a debug build with a dev profile its own service, so
/// a dev build never reads or replaces the installed app's secrets.
fn keychain_service() -> String {
    #[cfg(debug_assertions)]
    if crate::dev_profile_dir().is_some() {
        return format!("{SERVICE}.dev");
    }
    SERVICE.to_string()
}

fn slot() -> &'static RwLock<Arc<dyn SecretsBackend>> {
    static BACKEND: OnceLock<RwLock<Arc<dyn SecretsBackend>>> = OnceLock::new();
    BACKEND.get_or_init(|| {
        let backend: Arc<dyn SecretsBackend> = if use_memory_by_default() {
            log::debug!("secrets: using the in-memory backend");
            Arc::new(MemoryBackend::new())
        } else {
            Arc::new(Cached::new(KeychainBackend::new(keychain_service())))
        };
        RwLock::new(backend)
    })
}

fn backend() -> Arc<dyn SecretsBackend> {
    slot().read().clone()
}

/// Replace the process-wide backend (tests, embedding). Also forgets the
/// cached meetings data key.
pub fn set_backend(backend: Arc<dyn SecretsBackend>) {
    *slot().write() = backend;
    *data_key_cache().lock() = None;
}

/// Read a secret. `Ok(None)` when it doesn't exist.
pub fn get(key: &str) -> Result<Option<String>> {
    backend().get(key)
}

/// Store a secret. An empty value deletes it.
pub fn set(key: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return delete(key);
    }
    backend().set(key, value)
}

/// Delete a secret. Deleting a missing key is not an error.
pub fn delete(key: &str) -> Result<()> {
    backend().delete(key)
}

/// Meeting data encrypted by an earlier version exists, but no meetings
/// data key is stored to convert it with. Nothing is changed: the user
/// either restores the key (Retry) or moves the old data aside
/// (`reset_meetings_data`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeetingsDataKeyMissing;

impl std::fmt::Display for MeetingsDataKeyMissing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "the meetings data key is missing from the Keychain, \
             so meeting notes encrypted by an earlier version can't be converted",
        )
    }
}

impl std::error::Error for MeetingsDataKeyMissing {}

fn decode_data_key(encoded: &str) -> Result<[u8; 32]> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .context("meetings data key is not valid base64")?;
    <[u8; 32]>::try_from(bytes.as_slice())
        .map_err(|_| anyhow!("meetings data key has {} bytes, not 32", bytes.len()))
}

fn data_key_cache() -> &'static Mutex<Option<[u8; 32]>> {
    static KEY: OnceLock<Mutex<Option<[u8; 32]>>> = OnceLock::new();
    KEY.get_or_init(|| Mutex::new(None))
}

/// The key in `cache`, else the stored one, cached. Never generates one.
fn cached_data_key(
    cache: &Mutex<Option<[u8; 32]>>,
    backend: &dyn SecretsBackend,
) -> Result<[u8; 32]> {
    let mut cache = cache.lock();
    if let Some(k) = *cache {
        return Ok(k);
    }
    let key = match backend.get(MEETINGS_DATA_KEY)? {
        Some(encoded) => decode_data_key(&encoded)?,
        None => return Err(MeetingsDataKeyMissing.into()),
    };
    *cache = Some(key);
    Ok(key)
}

fn forget_data_key(cache: &Mutex<Option<[u8; 32]>>, backend: &dyn SecretsBackend) {
    let mut cache = cache.lock();
    *cache = None;
    backend.invalidate(MEETINGS_DATA_KEY);
}

/// Keep the key (if any) under a backup name, then delete it. A read error
/// changes nothing.
fn retire_data_key(backend: &dyn SecretsBackend, backup_suffix: &str) -> Result<bool> {
    let Some(old) = backend.get(MEETINGS_DATA_KEY)? else {
        return Ok(false);
    };
    backend.set(&format!("{MEETINGS_DATA_KEY}.{backup_suffix}"), &old)?;
    backend.delete(MEETINGS_DATA_KEY)?;
    Ok(true)
}

/// The meetings data key an earlier version encrypted meeting data with,
/// cached in-process: [`MeetingsDataKeyMissing`] when none is stored. A read
/// error is returned as is.
pub fn legacy_meetings_data_key() -> Result<[u8; 32]> {
    cached_data_key(data_key_cache(), backend().as_ref())
}

/// Forget the cached meetings data key (in-process and the Keychain read
/// cache), so the next use reads the Keychain again: before a Retry (the
/// user may have restored the key), and when the key didn't decrypt the
/// meeting data.
pub fn forget_meetings_data_key() {
    forget_data_key(data_key_cache(), backend().as_ref());
}

/// Once nothing is encrypted with it any more: keep the meetings data key
/// under `meetings_data_key.<backup_suffix>` (so a backup of the encrypted
/// data stays readable) and delete it. Returns whether there was one.
pub fn retire_meetings_data_key(backup_suffix: &str) -> Result<bool> {
    let mut cache = data_key_cache().lock();
    let retired = retire_data_key(backend().as_ref(), backup_suffix)?;
    *cache = None;
    backend().invalidate(MEETINGS_DATA_KEY);
    Ok(retired)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_use_the_memory_backend() {
        assert!(use_memory_by_default());
    }

    #[test]
    fn env_override_is_debug_only_and_case_insensitive() {
        // Test builds are debug-ish (cfg(test)), so the override is allowed.
        const { assert!(ENV_OVERRIDE_ALLOWED) };
        assert!(env_requests_memory(Some("memory")));
        assert!(env_requests_memory(Some("MEMORY")));
        assert!(!env_requests_memory(Some("keychain")));
        assert!(!env_requests_memory(None));
    }

    #[test]
    fn set_get_delete_round_trip() {
        let key = provider_api_key("secrets-test-round-trip");
        assert_eq!(get(&key).unwrap(), None);
        set(&key, "sk-123").unwrap();
        assert_eq!(get(&key).unwrap().as_deref(), Some("sk-123"));
        set(&key, "sk-456").unwrap();
        assert_eq!(get(&key).unwrap().as_deref(), Some("sk-456"));
        delete(&key).unwrap();
        assert_eq!(get(&key).unwrap(), None);
        // Deleting again is fine.
        delete(&key).unwrap();
    }

    #[test]
    fn empty_value_deletes() {
        let key = provider_api_key("secrets-test-empty");
        set(&key, "v").unwrap();
        set(&key, "").unwrap();
        assert_eq!(get(&key).unwrap(), None);
    }

    #[test]
    fn key_names_are_distinct() {
        assert_eq!(provider_api_key("openai"), "provider_api_key.openai");
        assert_ne!(LINEAR_API_KEY, MEETINGS_DATA_KEY);
    }

    #[test]
    fn cached_backend_caches_misses_and_writes() {
        struct Counting(Mutex<usize>, MemoryBackend);
        impl SecretsBackend for Counting {
            fn get(&self, key: &str) -> Result<Option<String>> {
                *self.0.lock() += 1;
                self.1.get(key)
            }
            fn set(&self, key: &str, value: &str) -> Result<()> {
                self.1.set(key, value)
            }
            fn delete(&self, key: &str) -> Result<()> {
                self.1.delete(key)
            }
        }
        let c = Cached::new(Counting(Mutex::new(0), MemoryBackend::new()));
        assert_eq!(c.get("k").unwrap(), None);
        assert_eq!(c.get("k").unwrap(), None);
        assert_eq!(*c.inner.0.lock(), 1);
        c.set("k", "v").unwrap();
        assert_eq!(c.get("k").unwrap().as_deref(), Some("v"));
        c.delete("k").unwrap();
        assert_eq!(c.get("k").unwrap(), None);
        assert_eq!(*c.inner.0.lock(), 1);
        // An invalidated key is read again.
        c.inner.1.set("k", "outside").unwrap();
        c.invalidate("k");
        assert_eq!(c.get("k").unwrap().as_deref(), Some("outside"));
        assert_eq!(*c.inner.0.lock(), 2);
        // A missing data key is never cached: restoring it is seen.
        assert_eq!(c.get(MEETINGS_DATA_KEY).unwrap(), None);
        c.inner.1.set(MEETINGS_DATA_KEY, "restored").unwrap();
        assert_eq!(
            c.get(MEETINGS_DATA_KEY).unwrap().as_deref(),
            Some("restored")
        );
    }

    #[test]
    fn a_slow_read_never_overwrites_a_newer_write() {
        use std::sync::mpsc;
        /// `get` reads, then waits for a go-ahead before returning.
        struct Slow {
            map: MemoryBackend,
            read: Mutex<mpsc::Sender<()>>,
            go: Mutex<mpsc::Receiver<()>>,
        }
        impl SecretsBackend for Slow {
            fn get(&self, key: &str) -> Result<Option<String>> {
                let v = self.map.get(key);
                self.read.lock().send(()).unwrap();
                self.go.lock().recv().unwrap();
                v
            }
            fn set(&self, key: &str, value: &str) -> Result<()> {
                self.map.set(key, value)
            }
            fn delete(&self, key: &str) -> Result<()> {
                self.map.delete(key)
            }
        }
        let (read_tx, read_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel();
        let c = Arc::new(Cached::new(Slow {
            map: MemoryBackend::new(),
            read: Mutex::new(read_tx),
            go: Mutex::new(go_rx),
        }));

        for write_new in [true, false] {
            c.invalidate("k");
            c.inner.map.set("k", "old").unwrap();
            let reader = {
                let c = c.clone();
                std::thread::spawn(move || c.get("k").unwrap())
            };
            read_rx.recv().unwrap(); // the reader has the old value
            if write_new {
                c.set("k", "new").unwrap();
            } else {
                c.delete("k").unwrap();
            }
            go_tx.send(()).unwrap();
            let expected = write_new.then(|| "new".to_string());
            assert_eq!(reader.join().unwrap(), expected, "the newer value wins");
            // Cached: no backend read (it would block).
            assert_eq!(c.get("k").unwrap(), expected);
        }
    }

    #[test]
    fn an_invalidation_during_a_slow_read_rereads() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::mpsc;
        /// `get` reads, then waits for a go-ahead before returning; `set`
        /// fails (after writing) while `fail_writes` is on.
        struct Slow {
            map: MemoryBackend,
            read: Mutex<mpsc::Sender<()>>,
            go: Mutex<mpsc::Receiver<()>>,
            fail_writes: AtomicBool,
        }
        impl SecretsBackend for Slow {
            fn get(&self, key: &str) -> Result<Option<String>> {
                let v = self.map.get(key);
                self.read.lock().send(()).unwrap();
                self.go.lock().recv().unwrap();
                v
            }
            fn set(&self, key: &str, value: &str) -> Result<()> {
                self.map.set(key, value)?;
                if self.fail_writes.load(Ordering::SeqCst) {
                    return Err(anyhow!("write failed"));
                }
                Ok(())
            }
            fn delete(&self, key: &str) -> Result<()> {
                self.map.delete(key)
            }
        }
        let (read_tx, read_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel();
        let c = Arc::new(Cached::new(Slow {
            map: MemoryBackend::new(),
            read: Mutex::new(read_tx),
            go: Mutex::new(go_rx),
            fail_writes: AtomicBool::new(false),
        }));

        for failed_write in [false, true] {
            c.invalidate("k");
            c.inner.map.set("k", "old").unwrap();
            let reader = {
                let c = c.clone();
                std::thread::spawn(move || c.get("k").unwrap())
            };
            read_rx.recv().unwrap(); // the reader has the old value
            if failed_write {
                c.inner.fail_writes.store(true, Ordering::SeqCst);
                assert!(c.set("k", "new").is_err());
                c.inner.fail_writes.store(false, Ordering::SeqCst);
            } else {
                c.inner.map.set("k", "new").unwrap();
                c.invalidate("k");
            }
            go_tx.send(()).unwrap();
            // Nothing cached for the key: the reader reads it again.
            read_rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("the stale read is not returned");
            go_tx.send(()).unwrap();
            assert_eq!(reader.join().unwrap().as_deref(), Some("new"));
            // Cached: no backend read (it would block).
            assert_eq!(c.get("k").unwrap().as_deref(), Some("new"));
        }
    }

    fn store_key(backend: &dyn SecretsBackend, k: [u8; 32]) {
        backend
            .set(
                MEETINGS_DATA_KEY,
                &base64::engine::general_purpose::STANDARD.encode(k),
            )
            .unwrap();
    }

    #[test]
    fn forgetting_the_data_key_rereads_the_keychain() {
        let backend = Cached::new(MemoryBackend::new());
        let cache = Mutex::new(None);
        // Missing: nothing generated.
        let e = cached_data_key(&cache, &backend).unwrap_err();
        assert!(e.downcast_ref::<MeetingsDataKeyMissing>().is_some(), "{e}");
        assert_eq!(backend.inner.get(MEETINGS_DATA_KEY).unwrap(), None);
        // The user restores the key in the Keychain: Retry sees it.
        store_key(&backend.inner, [1u8; 32]);
        assert_eq!(cached_data_key(&cache, &backend).unwrap(), [1u8; 32]);

        // A key that turns out not to decrypt the data is reused until
        // forgotten, then the Keychain is read again.
        store_key(&backend.inner, [7u8; 32]);
        assert_eq!(cached_data_key(&cache, &backend).unwrap(), [1u8; 32]);
        forget_data_key(&cache, &backend);
        assert_eq!(cached_data_key(&cache, &backend).unwrap(), [7u8; 32]);
    }

    #[test]
    fn retiring_the_data_key_keeps_it_under_a_backup_name() {
        let b = MemoryBackend::new();
        assert!(!retire_data_key(&b, "bak1").unwrap());
        assert_eq!(b.get("meetings_data_key.bak1").unwrap(), None);
        store_key(&b, [9u8; 32]);
        assert!(retire_data_key(&b, "bak2").unwrap());
        assert_eq!(b.get(MEETINGS_DATA_KEY).unwrap(), None);
        let backup = b.get("meetings_data_key.bak2").unwrap().unwrap();
        assert_eq!(decode_data_key(&backup).unwrap(), [9u8; 32]);
    }

    #[test]
    fn a_read_error_retires_nothing() {
        struct Broken(MemoryBackend);
        impl SecretsBackend for Broken {
            fn get(&self, _: &str) -> Result<Option<String>> {
                Err(anyhow!("keychain locked"))
            }
            fn set(&self, key: &str, value: &str) -> Result<()> {
                self.0.set(key, value)
            }
            fn delete(&self, key: &str) -> Result<()> {
                self.0.delete(key)
            }
        }
        let b = Broken(MemoryBackend::new());
        store_key(&b.0, [9u8; 32]);
        assert!(retire_data_key(&b, "x").is_err());
        assert!(b.0.get(MEETINGS_DATA_KEY).unwrap().is_some());
    }
}
