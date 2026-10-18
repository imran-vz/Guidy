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
            Arc::new(MemoryBackend::new())
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

/// The 32-byte meetings data key. Generated randomly and stored (base64) on
/// first use, then cached in-process. A read error is returned as is and
/// never causes a new key to be generated, so existing data stays readable.
pub fn meetings_data_key() -> Result<[u8; 32]> {
    let mut cache = data_key_cache().lock();
    if let Some(k) = *cache {
        return Ok(k);
    }
    let engine = base64::engine::general_purpose::STANDARD;
    let key = match get(MEETINGS_DATA_KEY)? {
        Some(encoded) => {
            let bytes = engine
                .decode(encoded.trim())
                .context("meetings data key is not valid base64")?;
            <[u8; 32]>::try_from(bytes.as_slice())
                .map_err(|_| anyhow!("meetings data key has {} bytes, not 32", bytes.len()))?
        }
        None => {
            let mut k = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::rng(), &mut k);
            set(MEETINGS_DATA_KEY, &engine.encode(k))?;
            log::info!("secrets: generated a new meetings data key");
            k
        }
    };
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
    fn meetings_data_key_is_generated_once_and_stable() {
        let a = meetings_data_key().unwrap();
        let b = meetings_data_key().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, [0u8; 32]);
        let stored = get(MEETINGS_DATA_KEY).unwrap().unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(stored)
            .unwrap();
        assert_eq!(decoded, a);
    }
}
