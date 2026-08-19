use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use tracing::warn;

use crate::config::{cache_db_path, MAX_CACHE_ENTRIES};

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
        if let Some(entry) = self.entries.get_mut(key) {
            if entry.expires_at > now {
                entry.last_accessed = now;
                return Some(entry.value.clone());
            }
            self.entries.remove(key);
        }
        None
    }

    fn put(
        &mut self,
        key: String,
        ttl: Duration,
        value: serde_json::Value,
        now: Instant,
    ) -> serde_json::Value {
        self.entries.insert(
            key,
            CacheEntry {
                expires_at: now + ttl,
                last_accessed: now,
                value: value.clone(),
            },
        );
        if self.entries.len() > MAX_CACHE_ENTRIES {
            self.purge_expired(now);
            self.evict_if_full();
        }
        value
    }
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn open_persistent_cache() -> Option<Connection> {
    let path = cache_db_path()?;
    let connection = match Connection::open(&path) {
        Ok(connection) => connection,
        Err(error) => {
            warn!("Could not open SQLite cache at {}: {}", path.display(), error);
            return None;
        }
    };

    if let Err(error) = connection.busy_timeout(Duration::from_millis(250)) {
        warn!("Could not configure SQLite cache timeout: {}", error);
        return None;
    }
    if let Err(error) = connection.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS cache_entries (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL,
             expires_at INTEGER NOT NULL,
             last_accessed INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS cache_expiry_idx ON cache_entries(expires_at);",
    ) {
        warn!("Could not initialize SQLite cache: {}", error);
        return None;
    }

    Some(connection)
}

fn persistent_get(key: &str) -> Option<(serde_json::Value, Duration)> {
    let now = unix_seconds();
    let mut lock = PERSISTENT_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let connection = lock.as_mut()?;

    let row = connection
        .query_row(
            "SELECT value, expires_at FROM cache_entries WHERE key = ?1",
            params![key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .ok()
        .flatten();

    let Some((serialized, expires_at)) = row else {
        return None;
    };
    if expires_at <= now {
        let _ = connection.execute("DELETE FROM cache_entries WHERE key = ?1", params![key]);
        return None;
    }

    let value = match serde_json::from_str(&serialized) {
        Ok(value) => value,
        Err(_) => {
            let _ = connection.execute("DELETE FROM cache_entries WHERE key = ?1", params![key]);
            return None;
        }
    };
    let _ = connection.execute(
        "UPDATE cache_entries SET last_accessed = ?2 WHERE key = ?1",
        params![key, now],
    );

    Some((value, Duration::from_secs((expires_at - now) as u64)))
}

fn persistent_put(key: &str, ttl: Duration, value: &serde_json::Value) {
    let Ok(serialized) = serde_json::to_string(value) else {
        return;
    };
    let now = unix_seconds();
    let ttl_secs = ttl.as_secs().min(i64::MAX as u64) as i64;
    let expires_at = now.saturating_add(ttl_secs);

    let mut lock = PERSISTENT_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(connection) = lock.as_mut() else {
        return;
    };

    if connection
        .execute(
            "INSERT INTO cache_entries(key, value, expires_at, last_accessed)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET
                 value = excluded.value,
                 expires_at = excluded.expires_at,
                 last_accessed = excluded.last_accessed",
            params![key, serialized, expires_at, now],
        )
        .is_err()
    {
        return;
    }

    let _ = connection.execute("DELETE FROM cache_entries WHERE expires_at <= ?1", params![now]);
    let _ = connection.execute(
        "DELETE FROM cache_entries WHERE key IN (
             SELECT key FROM cache_entries
             ORDER BY last_accessed DESC
             LIMIT -1 OFFSET ?1
         )",
        params![MAX_CACHE_ENTRIES as i64],
    );
}

static GLOBAL_CACHE: LazyLock<Mutex<LruCache>> = LazyLock::new(|| Mutex::new(LruCache::new()));
static PERSISTENT_CACHE: LazyLock<Mutex<Option<Connection>>> =
    LazyLock::new(|| Mutex::new(open_persistent_cache()));

pub fn cache_get(key: &str) -> Option<serde_json::Value> {
    let now = Instant::now();
    if let Some(value) = GLOBAL_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key, now)
    {
        return Some(value);
    }

    let (value, ttl) = persistent_get(key)?;
    GLOBAL_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .put(key.to_string(), ttl, value.clone(), Instant::now());
    Some(value)
}

pub fn cache_put(key: &str, ttl: Duration, value: serde_json::Value) -> serde_json::Value {
    let cached = GLOBAL_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .put(key.to_string(), ttl, value, Instant::now());
    persistent_put(key, ttl, &cached);
    cached
}
