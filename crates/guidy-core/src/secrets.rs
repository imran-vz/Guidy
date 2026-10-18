//! Secrets (API keys, the meetings data key), stored in the OS credential
//! store: the macOS Keychain, Windows Credential Manager, or the Linux kernel
//! keyring, through the `keyring` crate. See docs/MEETING_NOTES_ARCH.md.
//!
//! Every secret is a string under one key name, in the service
//! [`SERVICE`]. Use the key-name helpers ([`provider_api_key`],
//! [`LINEAR_API_KEY`], [`MEETINGS_DATA_KEY`]) rather than raw strings.
//!
//! Unit tests (`cfg(test)` or the `test-support` feature) use an in-memory
//! backend, so they never touch the real Keychain. Debug builds also honour
//! `GUIDY_SECRETS_BACKEND=memory`; release builds ignore it, so an environment
//! variable can't silently divert keys away from the Keychain.
//! [`set_backend`] swaps the backend explicitly.
//!
//! Keychain reads are cached in-process (including misses), because callers
//! such as the settings UI read keys on every refresh.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use parking_lot::{Mutex, RwLock};

/// Keychain service name for every Guidy secret.
pub const SERVICE: &str = "dev.guidy.app";

/// Key of the Linear personal API key.
pub const LINEAR_API_KEY: &str = "linear_api_key";

/// Key of the 32-byte meetings data key (base64), used for `meetings.db` and
/// the encrypted meeting audio.
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

/// Wraps a slow backend (the Keychain) with an in-process read cache. Only
/// successful reads are cached; writes update the cache after they succeed.
struct Cached<B> {
    inner: B,
    cache: Mutex<HashMap<String, Option<String>>>,
}

impl<B: SecretsBackend> SecretsBackend for Cached<B> {
    fn get(&self, key: &str) -> Result<Option<String>> {
        if let Some(v) = self.cache.lock().get(key) {
            return Ok(v.clone());
        }
        let v = self.inner.get(key)?;
        self.cache.lock().insert(key.to_string(), v.clone());
        Ok(v)
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.inner.set(key, value)?;
        self.cache
            .lock()
            .insert(key.to_string(), Some(value.to_string()));
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.inner.delete(key)?;
        self.cache.lock().insert(key.to_string(), None);
        Ok(())
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

fn slot() -> &'static RwLock<Arc<dyn SecretsBackend>> {
    static BACKEND: OnceLock<RwLock<Arc<dyn SecretsBackend>>> = OnceLock::new();
    BACKEND.get_or_init(|| {
        let backend: Arc<dyn SecretsBackend> = if use_memory_by_default() {
            log::debug!("secrets: using the in-memory backend");
            let memory = MemoryBackend::new();
            // Test builds start like an installed app after first use: with a
            // meetings data key, so code that reads it needs no setup.
            if cfg!(any(test, feature = "test-support")) {
                let _ = generate_data_key(&memory);
            }
            Arc::new(memory)
        } else {
            Arc::new(Cached {
                inner: KeychainBackend::new(SERVICE),
                cache: Mutex::new(HashMap::new()),
            })
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

fn data_key_cache() -> &'static Mutex<Option<[u8; 32]>> {
    static KEY: OnceLock<Mutex<Option<[u8; 32]>>> = OnceLock::new();
    KEY.get_or_init(|| Mutex::new(None))
}

/// No meetings data key is stored, but encrypted meeting data exists, so a
/// new key would leave that data unreadable. Nothing is generated: the user
/// either restores the key (Retry) or moves the old data aside
/// (`reset_meetings_data`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeetingsDataKeyMissing;

impl std::fmt::Display for MeetingsDataKeyMissing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "the meetings data key is missing from the Keychain, \
             so existing meeting notes can't be decrypted",
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

fn generate_data_key(backend: &dyn SecretsBackend) -> Result<[u8; 32]> {
    let mut k = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut k);
    backend.set(
        MEETINGS_DATA_KEY,
        &base64::engine::general_purpose::STANDARD.encode(k),
    )?;
    Ok(k)
}

/// The stored key; a missing one is generated only when `may_create`.
fn load_data_key(backend: &dyn SecretsBackend, may_create: bool) -> Result<[u8; 32]> {
    match backend.get(MEETINGS_DATA_KEY)? {
        Some(encoded) => decode_data_key(&encoded),
        None if may_create => {
            let k = generate_data_key(backend)?;
            log::info!("secrets: generated a new meetings data key");
            Ok(k)
        }
        None => Err(MeetingsDataKeyMissing.into()),
    }
}

/// Keep the current key (if any) under a backup name, then store a new one.
fn replace_data_key(backend: &dyn SecretsBackend, backup_suffix: &str) -> Result<[u8; 32]> {
    if let Some(old) = backend.get(MEETINGS_DATA_KEY)? {
        backend.set(&format!("{MEETINGS_DATA_KEY}.{backup_suffix}"), &old)?;
    }
    let k = generate_data_key(backend)?;
    log::info!("secrets: replaced the meetings data key (old one kept as .{backup_suffix})");
    Ok(k)
}

/// The 32-byte meetings data key, cached in-process. Never generates one:
/// [`MeetingsDataKeyMissing`] when none is stored (only
/// [`open_meetings_data_key`] creates it). A read error is returned as is.
pub fn meetings_data_key() -> Result<[u8; 32]> {
    let mut cache = data_key_cache().lock();
    if let Some(k) = *cache {
        return Ok(k);
    }
    let key = load_data_key(backend().as_ref(), false)?;
    *cache = Some(key);
    Ok(key)
}

/// [`meetings_data_key`], generating and storing a key when none exists, but
/// only if `has_encrypted_data` is false. With existing data a missing key is
/// [`MeetingsDataKeyMissing`]: a fresh key would make that data unreadable.
pub fn open_meetings_data_key(has_encrypted_data: bool) -> Result<[u8; 32]> {
    let mut cache = data_key_cache().lock();
    if let Some(k) = *cache {
        return Ok(k);
    }
    let key = load_data_key(backend().as_ref(), !has_encrypted_data)?;
    *cache = Some(key);
    Ok(key)
}

/// Store a new meetings data key. Call only once the data encrypted with the
/// old key has been moved aside. The old key, if readable, is kept under
/// `meetings_data_key.<backup_suffix>` so that data stays recoverable; a
/// Keychain read error aborts without changing anything.
pub fn replace_meetings_data_key(backup_suffix: &str) -> Result<[u8; 32]> {
    let mut cache = data_key_cache().lock();
    let key = replace_data_key(backend().as_ref(), backup_suffix)?;
    *cache = Some(key);
    Ok(key)
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
        let c = Cached {
            inner: Counting(Mutex::new(0), MemoryBackend::new()),
            cache: Mutex::new(HashMap::new()),
        };
        assert_eq!(c.get("k").unwrap(), None);
        assert_eq!(c.get("k").unwrap(), None);
        assert_eq!(*c.inner.0.lock(), 1);
        c.set("k", "v").unwrap();
        assert_eq!(c.get("k").unwrap().as_deref(), Some("v"));
        c.delete("k").unwrap();
        assert_eq!(c.get("k").unwrap(), None);
        assert_eq!(*c.inner.0.lock(), 1);
    }

    #[test]
    fn meetings_data_key_is_stable() {
        let a = meetings_data_key().unwrap();
        let b = open_meetings_data_key(true).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, [0u8; 32]);
        let stored = get(MEETINGS_DATA_KEY).unwrap().unwrap();
        assert_eq!(decode_data_key(&stored).unwrap(), a);
    }

    #[test]
    fn a_missing_data_key_is_created_only_without_existing_data() {
        let b = MemoryBackend::new();
        let e = load_data_key(&b, false).unwrap_err();
        assert!(e.downcast_ref::<MeetingsDataKeyMissing>().is_some(), "{e}");
        assert_eq!(b.get(MEETINGS_DATA_KEY).unwrap(), None, "nothing written");

        let k = load_data_key(&b, true).unwrap();
        // Once stored it is read back, whatever `may_create` says.
        assert_eq!(load_data_key(&b, false).unwrap(), k);
        assert_eq!(load_data_key(&b, true).unwrap(), k);
    }

    #[test]
    fn a_read_error_never_creates_a_key() {
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
        assert!(load_data_key(&b, true).is_err());
        assert!(replace_data_key(&b, "x").is_err());
        assert_eq!(b.0.get(MEETINGS_DATA_KEY).unwrap(), None);
    }

    #[test]
    fn replacing_the_data_key_keeps_the_old_one() {
        let b = MemoryBackend::new();
        // No old key: nothing to back up.
        let first = replace_data_key(&b, "bak1").unwrap();
        assert_eq!(b.get("meetings_data_key.bak1").unwrap(), None);
        let second = replace_data_key(&b, "bak2").unwrap();
        assert_ne!(first, second);
        assert_eq!(load_data_key(&b, false).unwrap(), second);
        let backup = b.get("meetings_data_key.bak2").unwrap().unwrap();
        assert_eq!(decode_data_key(&backup).unwrap(), first);
    }
}
