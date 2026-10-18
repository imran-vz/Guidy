//! A small TTL cache for Linear lists (teams, projects, users, labels).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// How long fetched lists are reused.
pub const TTL: Duration = Duration::from_secs(5 * 60);

pub struct TtlCache<V> {
    ttl: Duration,
    entries: Mutex<HashMap<String, (Instant, V)>>,
}

impl<V: Clone> TtlCache<V> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, key: &str) -> Option<V> {
        let entries = self.entries.lock();
        let (at, v) = entries.get(key)?;
        (at.elapsed() < self.ttl).then(|| v.clone())
    }

    pub fn put(&self, key: &str, value: V) {
        self.entries
            .lock()
            .insert(key.to_string(), (Instant::now(), value));
    }

    pub fn clear(&self) {
        self.entries.lock().clear();
    }

    /// Cached value, or `fetch()` stored on success.
    pub fn get_or_try<E>(&self, key: &str, fetch: impl FnOnce() -> Result<V, E>) -> Result<V, E> {
        if let Some(v) = self.get(key) {
            return Ok(v);
        }
        let v = fetch()?;
        self.put(key, v.clone());
        Ok(v)
    }
}

impl<V: Clone> Default for TtlCache<V> {
    fn default() -> Self {
        Self::new(TTL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caches_until_expiry_and_clear() {
        let c: TtlCache<u32> = TtlCache::default();
        let mut calls = 0;
        for _ in 0..3 {
            let v: Result<u32, ()> = c.get_or_try("k", || {
                calls += 1;
                Ok(7)
            });
            assert_eq!(v, Ok(7));
        }
        assert_eq!(calls, 1);
        c.clear();
        assert_eq!(c.get("k"), None);
        let expired: TtlCache<u32> = TtlCache::new(Duration::ZERO);
        expired.put("k", 1);
        assert_eq!(expired.get("k"), None);
        // Errors aren't cached.
        let e: Result<u32, &str> = c.get_or_try("x", || Err("boom"));
        assert!(e.is_err());
        assert_eq!(c.get("x"), None);
    }
}
