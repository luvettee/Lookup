use std::collections::HashMap;
use std::future::Future;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tracing::debug;

use crate::config::{
    AUTH_FAILURE_COOLDOWN, CONNECTION_FAILURE_COOLDOWN, PROVIDER_FAILURE_COOLDOWN,
    RATE_LIMIT_COOLDOWN, TIMEOUT_FAILURE_COOLDOWN,
};

#[derive(Clone, Debug)]
pub struct ProviderHealth {
    pub healthy: bool,
    pub cooldown_until: Instant,
    pub last_latency: Option<Duration>,
    pub failure_count: usize,
    pub success_count: usize,
    pub failure_reason: Option<String>,
    pub last_seen: Instant,
    pub preferred: bool,
    pub json_validated: bool,
    pub last_validated: Option<Instant>,
}

impl Default for ProviderHealth {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            healthy: true,
            cooldown_until: now,
            last_latency: None,
            failure_count: 0,
            success_count: 0,
            failure_reason: None,
            last_seen: now,
            preferred: false,
            json_validated: false,
            last_validated: None,
        }
    }
}

static PROVIDER_HEALTH: LazyLock<Mutex<HashMap<String, ProviderHealth>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn health_available(name: &str) -> bool {
    let now = Instant::now();
    let lock = PROVIDER_HEALTH.lock().unwrap();
    if let Some(h) = lock.get(name) {
        now >= h.cooldown_until
    } else {
        true
    }
}

pub fn failure_kind(err_msg: &str) -> (&'static str, Duration) {
    let msg = err_msg.to_lowercase();
    if msg.contains("unauthorized")
        || msg.contains("invalid key")
        || msg.contains("api key")
        || msg.contains("401")
        || msg.contains("403")
        || msg.contains("forbidden")
    {
        ("authentication", AUTH_FAILURE_COOLDOWN)
    } else if msg.contains("429")
        || msg.contains("rate limit")
        || msg.contains("too many requests")
    {
        ("rate_limit", RATE_LIMIT_COOLDOWN)
    } else if msg.contains("timeout") || msg.contains("timed out") {
        ("timeout", TIMEOUT_FAILURE_COOLDOWN)
    } else if msg.contains("could not reach")
        || msg.contains("connection")
        || msg.contains("dns")
        || msg.contains("resolve")
        || msg.contains("network")
        || msg.contains("json request failed")
    {
        ("connection", CONNECTION_FAILURE_COOLDOWN)
    } else {
        ("failure", PROVIDER_FAILURE_COOLDOWN)
    }
}

pub fn health_success(name: &str, latency: Duration) {
    let now = Instant::now();
    let mut lock = PROVIDER_HEALTH.lock().unwrap();
    let entry = lock.entry(name.to_string()).or_default();
    entry.healthy = true;
    entry.cooldown_until = now;
    entry.last_latency = Some(latency);
    entry.failure_count = 0;
    entry.success_count += 1;
    entry.last_seen = now;
}

pub fn health_failure(name: &str, error: &str) {
    let (reason, cooldown) = failure_kind(error);
    debug!(
        "Provider {} failed ({}): {} — cooldown {}s",
        name,
        reason,
        error,
        cooldown.as_secs()
    );
    let now = Instant::now();
    let mut lock = PROVIDER_HEALTH.lock().unwrap();
    let entry = lock.entry(name.to_string()).or_default();
    entry.healthy = false;
    entry.cooldown_until = now + cooldown;
    entry.failure_count += 1;
    entry.success_count = 0;
    entry.failure_reason = Some(reason.to_string());
    entry.last_seen = now;
    entry.preferred = false;
}

pub async fn attempt_provider<F, Fut, T>(name: &str, f: F) -> Result<T, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let started = Instant::now();
    match f().await {
        Ok(val) => {
            health_success(name, started.elapsed());
            Ok(val)
        }
        Err(err) => {
            health_failure(name, &err);
            Err(err)
        }
    }
}

pub fn mark_searxng_preferred(url: &str) {
    let target = format!("searxng:{}", url);
    let mut lock = PROVIDER_HEALTH.lock().unwrap();
    for (name, status) in lock.iter_mut() {
        if name.starts_with("searxng:") {
            status.preferred = name == &target;
        }
    }
}

pub fn set_searxng_validated(url: &str) {
    let target = format!("searxng:{}", url);
    let now = Instant::now();
    let mut lock = PROVIDER_HEALTH.lock().unwrap();
    let entry = lock.entry(target).or_default();
    entry.json_validated = true;
    entry.last_validated = Some(now);
}

pub fn get_provider_health_snapshot() -> HashMap<String, ProviderHealth> {
    let lock = PROVIDER_HEALTH.lock().unwrap();
    lock.clone()
}
