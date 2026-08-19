use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::config::MAX_CACHE_ENTRIES;

#[derive(Clone)]
struct CacheEntry {
    expires_at: Instant,
    last_accessed: Instant,
    value: serde_json::Value,
}

struct LruCache {
    entries: HashMap<String, CacheEntry>,
}

impl LruCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    fn purge_expired(&mut self, now: Instant) {
        self.entries.retain(|_, v| v.expires_at > now);
    }

    fn evict_if_full(&mut self) {
        while self.entries.len() > MAX_CACHE_ENTRIES {
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, v)| v.last_accessed)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest_key);
            } else {
                break;
            }
        }
    }

    fn get(&mut self, key: &str, now: Instant) -> Option<serde_json::Value> {
        self.purge_expired(now);
        if let Some(entry) = self.entries.get_mut(key) {
            if entry.expires_at > now {
                entry.last_accessed = now;
                return Some(entry.value.clone());
            } else {
                self.entries.remove(key);
            }
        }
        None
    }

    fn put(&mut self, key: String, ttl: Duration, value: serde_json::Value, now: Instant) -> serde_json::Value {
        self.entries.insert(
            key,
            CacheEntry {
                expires_at: now + ttl,
                last_accessed: now,
                value: value.clone(),
            },
        );
        self.purge_expired(now);
        self.evict_if_full();
        value
    }
}

static GLOBAL_CACHE: LazyLock<Mutex<LruCache>> = LazyLock::new(|| Mutex::new(LruCache::new()));

pub fn cache_get(key: &str) -> Option<serde_json::Value> {
    let now = Instant::now();
    let mut lock = GLOBAL_CACHE.lock().unwrap();
    lock.get(key, now)
}

pub fn cache_put(key: &str, ttl: Duration, value: serde_json::Value) -> serde_json::Value {
    let now = Instant::now();
    let mut lock = GLOBAL_CACHE.lock().unwrap();
    lock.put(key.to_string(), ttl, value, now)
}
