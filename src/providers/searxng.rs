use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use regex::Regex;
use serde_json::{json, Value};
use url::form_urlencoded;
use url::Url;

use crate::config::{
    searxng_urls, DEFAULT_SEARXNG_INSTANCES, MAX_DISCOVERED_INSTANCES, MAX_SEARX_VALIDATION_WAVES,
    NETWORK_TIMEOUT, SEARX_DIRECTORY_MAX_BYTES, SEARX_DIRECTORY_STALE_TTL, SEARX_DIRECTORY_TTL,
    SEARX_PREFERRED_TIMEOUT, SEARX_PUBLIC_VALIDATION_BUDGET, SEARX_RACE_SIZE, SEARX_SEARCH_TIMEOUT,
    SEARX_SPACE_DIRECTORY_URL,
};
use crate::health::{
    attempt_provider, get_provider_health_snapshot, health_available, mark_searxng_preferred,
    set_searxng_validated,
};
use crate::net::{get_json, validate_url};

#[derive(Clone, Debug)]
struct SearxDirectoryEntry {
    url: String,
    latency: f64,
    success: f64,
    uptime: f64,
}

struct SearxDirectoryCache {
    instances: Vec<SearxDirectoryEntry>,
    expires_at: Instant,
    updated_at: Instant,
}

static SEARX_DIRECTORY: LazyLock<Mutex<SearxDirectoryCache>> = LazyLock::new(|| {
    let now = Instant::now();
    Mutex::new(SearxDirectoryCache {
        instances: Vec::new(),
        expires_at: now,
        updated_at: now,
    })
});

fn parse_searx_directory(payload: &Value) -> Result<Vec<SearxDirectoryEntry>, String> {
    let instances_map = payload
        .get("instances")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "Invalid searx.space directory".to_string())?;

    let mut candidates = Vec::new();

    for (raw_url, detail) in instances_map {
        let clean_url = raw_url.trim_end_matches('/');
        let url = match validate_url(clean_url, false) {
            Ok(u) => u,
            Err(_) => continue,
        };

        let parsed_u = match Url::parse(&url) {
            Ok(p) => p,
            Err(_) => continue,
        };

        if parsed_u.scheme() != "https"
            || parsed_u.query().is_some()
            || parsed_u.fragment().is_some()
            || detail.get("network_type").and_then(|v| v.as_str()) != Some("normal")
        {
            continue;
        }

        if detail.get("main").and_then(|v| v.as_bool()) == Some(false)
            || detail.get("analytics").and_then(|v| v.as_bool()) == Some(true)
            || detail.get("error").is_some()
        {
            continue;
        }

        let http = detail.get("http").and_then(|v| v.as_object());
        let tls = detail.get("tls").and_then(|v| v.as_object());
        let network = detail.get("network").and_then(|v| v.as_object());
        let timing = detail.get("timing").and_then(|v| v.as_object());
        let search = timing.and_then(|t| t.get("search")).and_then(|v| v.as_object());
        let uptime = detail.get("uptime").and_then(|v| v.as_object());

        let http_ok = http.map(|h| h.get("status_code").and_then(|v| v.as_i64()) == Some(200) && h.get("error").is_none()).unwrap_or(false);
        let tls_ok = tls.map(|t| t.get("error").is_none() && t.get("version").is_some()).unwrap_or(false);
        let network_ok = network.map(|n| n.get("error").is_none()).unwrap_or(false);
        let search_ok = search.map(|s| s.get("error").is_none()).unwrap_or(false);
        let version_ok = detail.get("version").is_some();

        if !http_ok || !tls_ok || !network_ok || !search_ok || !version_ok {
            continue;
        }

        let success = search
            .and_then(|s| s.get("success_percentage"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let timings_all = search.and_then(|s| s.get("all")).and_then(|v| v.as_object());
        let latency = timings_all
            .and_then(|t| {
                t.get("median")
                    .or_else(|| t.get("mean"))
                    .or_else(|| t.get("value"))
            })
            .and_then(|v| v.as_f64())
            .unwrap_or(99.0);

        let week_uptime = uptime
            .and_then(|u| u.get("uptimeWeek"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let month_uptime = uptime
            .and_then(|u| u.get("uptimeMonth"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        if success < 90.0 || !(0.0..=3.0).contains(&latency) || week_uptime < 90.0 || month_uptime < 90.0 {
            continue;
        }

        candidates.push(SearxDirectoryEntry {
            url,
            latency,
            success,
            uptime: week_uptime.min(month_uptime),
        });
    }

    candidates.sort_by(|a, b| {
        b.success
            .partial_cmp(&a.success)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.latency.partial_cmp(&b.latency).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| b.uptime.partial_cmp(&a.uptime).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.url.cmp(&b.url))
    });

    if candidates.is_empty() {
        return Err("searx.space listed no suitable instances".to_string());
    }

    candidates.truncate(MAX_DISCOVERED_INSTANCES);
    Ok(candidates)
}

fn get_cached_directory(allow_stale: bool) -> Vec<SearxDirectoryEntry> {
    let now = Instant::now();
    let lock = SEARX_DIRECTORY.lock().unwrap();
    if !lock.instances.is_empty() {
        if now < lock.expires_at || (allow_stale && now.duration_since(lock.updated_at) < SEARX_DIRECTORY_STALE_TTL) {
            return lock.instances.clone();
        }
    }
    Vec::new()
}

fn emergency_instances() -> Vec<SearxDirectoryEntry> {
    DEFAULT_SEARXNG_INSTANCES
        .iter()
        .map(|&u| SearxDirectoryEntry {
            url: u.trim_end_matches('/').to_string(),
            latency: 99.0,
            success: 0.0,
            uptime: 0.0,
        })
        .collect()
}

async fn discover_searxng_instances() -> Vec<SearxDirectoryEntry> {
    let cached = get_cached_directory(false);
    if !cached.is_empty() {
        return cached;
    }

    let health_name = "directory:searx.space";
    if !health_available(health_name) {
        let stale = get_cached_directory(true);
        return if !stale.is_empty() { stale } else { emergency_instances() };
    }

    let fetch_result = attempt_provider(health_name, || async {
        get_json(
            SEARX_SPACE_DIRECTORY_URL,
            Some(NETWORK_TIMEOUT),
            Some(SEARX_DIRECTORY_MAX_BYTES),
            None,
        )
        .await
    })
    .await;

    if let Ok(payload) = fetch_result {
        if let Ok(instances) = parse_searx_directory(&payload) {
            let now = Instant::now();
            let mut lock = SEARX_DIRECTORY.lock().unwrap();
            lock.instances = instances.clone();
            lock.expires_at = now + SEARX_DIRECTORY_TTL;
            lock.updated_at = now;
            return instances;
        }
    }

    let stale = get_cached_directory(true);
    if !stale.is_empty() {
        stale
    } else {
        emergency_instances()
    }
}

async fn public_searxng_candidates() -> Vec<String> {
    let directory = discover_searxng_instances().await;
    let mut metadata = HashMap::new();
    for item in &directory {
        metadata.insert(item.url.clone(), item.clone());
    }

    let mut urls = Vec::new();
    for &d in DEFAULT_SEARXNG_INSTANCES {
        urls.push(d.to_string());
    }
    for item in &directory {
        if !urls.contains(&item.url) {
            urls.push(item.url.clone());
        }
    }

    let health = get_provider_health_snapshot();
    for (name, status) in &health {
        if name.starts_with("searxng:") && status.healthy {
            let u = name.strip_prefix("searxng:").unwrap();
            if !urls.iter().any(|existing| existing == u) {
                urls.push(u.to_string());
            }
        }
    }

    let mut safe_urls = Vec::new();
    for u in urls {
        let clean = u.trim_end_matches('/');
        if let Ok(safe_url) = validate_url(clean, false) {
            if safe_url.starts_with("https://") && health_available(&format!("searxng:{}", safe_url)) {
                safe_urls.push(safe_url);
            }
        }
    }

    safe_urls.dedup();

    safe_urls.sort_by(|a, b| {
        let h_a = health.get(&format!("searxng:{}", a));
        let h_b = health.get(&format!("searxng:{}", b));

        let pref_a = h_a.map(|h| h.preferred).unwrap_or(false);
        let pref_b = h_b.map(|h| h.preferred).unwrap_or(false);

        let healthy_a = h_a.map(|h| h.healthy).unwrap_or(true);
        let healthy_b = h_b.map(|h| h.healthy).unwrap_or(true);

        let valid_a = h_a.map(|h| h.json_validated).unwrap_or(false);
        let valid_b = h_b.map(|h| h.json_validated).unwrap_or(false);

        let def_a = DEFAULT_SEARXNG_INSTANCES.contains(&a.as_str());
        let def_b = DEFAULT_SEARXNG_INSTANCES.contains(&b.as_str());

        let lat_a = h_a
            .and_then(|h| h.last_latency.map(|d| d.as_secs_f64()))
            .or_else(|| metadata.get(a).map(|m| m.latency))
            .unwrap_or(99.0);
        let lat_b = h_b
            .and_then(|h| h.last_latency.map(|d| d.as_secs_f64()))
            .or_else(|| metadata.get(b).map(|m| m.latency))
            .unwrap_or(99.0);

        let fail_a = h_a.map(|h| h.failure_count).unwrap_or(0);
        let fail_b = h_b.map(|h| h.failure_count).unwrap_or(0);

        let succ_a = metadata.get(a).map(|m| m.success).unwrap_or(0.0);
        let succ_b = metadata.get(b).map(|m| m.success).unwrap_or(0.0);

        pref_b
            .cmp(&pref_a)
            .then_with(|| healthy_b.cmp(&healthy_a))
            .then_with(|| valid_b.cmp(&valid_a))
            .then_with(|| def_b.cmp(&def_a))
            .then_with(|| lat_a.partial_cmp(&lat_b).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| fail_a.cmp(&fail_b))
            .then_with(|| succ_b.partial_cmp(&succ_a).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.cmp(b))
    });

    safe_urls
}

async fn race_searxng(
    candidates: &[String],
    encoded_params: &str,
    timeout: Duration,
    accept_empty: bool,
) -> Result<Value, String> {
    let race_set = &candidates[..candidates.len().min(SEARX_RACE_SIZE)];
    if race_set.is_empty() {
        return Err("no eligible instances".to_string());
    }

    let mut futs = Vec::new();
    for url in race_set {
        let base_url = url.clone();
        let enc = encoded_params.to_string();
        let req_url = format!("{}/search?{}", base_url, enc);
        let provider_name = format!("searxng:{}", base_url);

        futs.push(async move {
            let res = attempt_provider(&provider_name, || async {
                let data = get_json(&req_url, Some(timeout), None, None).await?;
                if data.get("results").and_then(|v| v.as_array()).is_none() {
                    return Err("Invalid SearXNG JSON search response".to_string());
                }
                set_searxng_validated(&base_url);
                Ok(data)
            })
            .await;
            (base_url, res)
        });
    }

    let results = futures::future::join_all(futs).await;
    let mut saw_empty = false;

    for (url, res) in results {
        if let Ok(data) = res {
            if let Some(arr) = data.get("results").and_then(|v| v.as_array()) {
                if !arr.is_empty() {
                    mark_searxng_preferred(&url);
                    return Ok(data);
                } else {
                    saw_empty = true;
                }
            }
        }
    }

    if saw_empty && accept_empty {
        return Ok(json!({"results": []}));
    }

    Err("instances returned no results or failed".to_string())
}

async fn search_searxng_waves(
    candidates: &[String],
    encoded_params: &str,
    budget: Duration,
    stop_after_empty: bool,
) -> Result<Value, String> {
    let started = Instant::now();
    let limit = candidates.len().min(SEARX_RACE_SIZE * MAX_SEARX_VALIDATION_WAVES);
    let mut saw_empty = false;

    for offset in (0..limit).step_by(SEARX_RACE_SIZE) {
        let elapsed = started.elapsed();
        if elapsed >= budget {
            break;
        }
        let remaining = budget - elapsed;
        let end_idx = (offset + SEARX_RACE_SIZE).min(candidates.len());
        let wave: Vec<String> = candidates[offset..end_idx]
            .iter()
            .filter(|u| health_available(&format!("searxng:{}", u)))
            .cloned()
            .collect();

        if wave.is_empty() {
            continue;
        }

        let wave_timeout = SEARX_SEARCH_TIMEOUT.min(remaining);
        match race_searxng(&wave, encoded_params, wave_timeout, false).await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if e.contains("instances returned no results") {
                    saw_empty = true;
                    if stop_after_empty {
                        return Ok(json!({"results": []}));
                    }
                }
            }
        }
    }

    if saw_empty {
        return Ok(json!({"results": []}));
    }

    Err("all ranked instance waves failed".to_string())
}

pub fn searxng_query_rewrite(query: &str) -> String {
    if query.contains('"')
        || query.contains("site:")
        || query.contains("http://")
        || query.contains("https://")
    {
        return query.to_string();
    }

    static QUALIFIER_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^(.+?)\s+in\s+([^,;:!?]+)$").unwrap());

    if let Some(cap) = QUALIFIER_RE.captures(query) {
        let subject = cap[1].trim();
        let qualifier = cap[2].trim();
        if !subject.is_empty() && !qualifier.is_empty() && qualifier.split_whitespace().count() <= 6 {
            return format!("{} {}", qualifier, subject);
        }
    }

    query.to_string()
}

pub fn rank_searxng_results(query: &str, mut results: Vec<Value>) -> Vec<Value> {
    static STOPWORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
        let mut s = HashSet::new();
        for &word in &[
            "a", "an", "and", "are", "at", "best", "do", "for", "from", "how", "in", "is", "of",
            "on", "the", "things", "to", "what", "where", "with",
        ] {
            s.insert(word);
        }
        s
    });

    let terms: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 2 && !STOPWORDS.contains(s))
        .map(|s| s.to_string())
        .collect();

    static TRAVEL_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\b(things? to do|places? to (visit|see))\b").unwrap());

    let travel_intent = TRAVEL_RE.is_match(query);

    results.sort_by(|a, b| {
        let score_a = score_searxng_item(a, &terms, travel_intent);
        let score_b = score_searxng_item(b, &terms, travel_intent);
        score_b.cmp(&score_a)
    });

    results
}

fn score_searxng_item(item: &Value, terms: &[String], travel_intent: bool) -> usize {
    let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
    let content = item.get("content").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
    let snippet = item.get("snippet").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
    let desc = item.get("description").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
    let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();

    let rest = format!("{} {} {} {}", content, snippet, desc, url);

    let mut val = 0;
    for term in terms {
        if title.contains(term) {
            val += 3;
        }
        if rest.contains(term) {
            val += 1;
        }
    }

    if travel_intent {
        for term in &[
            "attraction", "tourism", "travel", "visit", "activity", "activities", "holiday",
            "destination",
        ] {
            if title.contains(term) || rest.contains(term) {
                val += 2;
            }
        }
    }

    val
}

pub async fn searxng_search(params: &HashMap<String, String>) -> Result<Value, String> {
    let mut query_params = form_urlencoded::Serializer::new(String::new());
    for (k, v) in params {
        query_params.append_pair(k, v);
    }
    let encoded = query_params.finish();

    // 1. Check configured instances
    let configured_urls = searxng_urls();
    let mut safe_configured = Vec::new();
    for u in configured_urls {
        if let Ok(safe_url) = validate_url(&u, false) {
            if health_available(&format!("searxng:{}", safe_url)) {
                safe_configured.push(safe_url);
            }
        }
    }

    if !safe_configured.is_empty() {
        if let Ok(res) = race_searxng(&safe_configured, &encoded, SEARX_SEARCH_TIMEOUT, true).await {
            return Ok(res);
        }
    }

    // 2. Preferred instance
    let health = get_provider_health_snapshot();
    let preferred: Vec<String> = health
        .iter()
        .filter(|(name, status)| {
            name.starts_with("searxng:")
                && status.preferred
                && status.healthy
                && status.json_validated
                && health_available(name)
        })
        .map(|(name, _)| name.strip_prefix("searxng:").unwrap().to_string())
        .collect();

    if let Some(pref) = preferred.first() {
        if let Ok(res) = race_searxng(&[pref.clone()], &encoded, SEARX_PREFERRED_TIMEOUT, false).await {
            return Ok(res);
        }
    }

    // 3. Public instances
    let public_cands = public_searxng_candidates().await;
    let public: Vec<String> = public_cands
        .into_iter()
        .filter(|u| !preferred.contains(u))
        .collect();

    if public.is_empty() {
        return Err("No healthy public SearXNG instance is currently available. Do not retry immediately.".to_string());
    }

    let has_time_range = params.contains_key("time_range");
    let result = search_searxng_waves(&public, &encoded, SEARX_PUBLIC_VALIDATION_BUDGET, has_time_range).await;

    let res_val = match result {
        Ok(v) => v,
        Err(_) => {
            return Err("No working public SearXNG instance was found. Do not retry immediately.".to_string());
        }
    };

    let has_results = res_val
        .get("results")
        .and_then(|v| v.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);

    if has_results || !has_time_range {
        return Ok(res_val);
    }

    // Relax time_range if public instance returned no results
    let mut relaxed_params = params.clone();
    let requested_recency = relaxed_params.remove("time_range");
    let mut relaxed_query_params = form_urlencoded::Serializer::new(String::new());
    for (k, v) in &relaxed_params {
        relaxed_query_params.append_pair(k, v);
    }
    let relaxed_encoded = relaxed_query_params.finish();
    let relaxed_public = public_searxng_candidates().await;

    if let Ok(mut relaxed) = search_searxng_waves(&relaxed_public, &relaxed_encoded, SEARX_PUBLIC_VALIDATION_BUDGET, false).await {
        if let Some(arr) = relaxed.get("results").and_then(|v| v.as_array()) {
            if !arr.is_empty() {
                if let Value::Object(ref mut map) = relaxed {
                    map.insert("recency_relaxed".to_string(), json!(true));
                    if let Some(rec) = requested_recency {
                        map.insert("requested_recency".to_string(), json!(rec));
                    }
                    map.insert(
                        "filter_notice".to_string(),
                        json!("Public SearXNG instances returned no results with the requested recency filter, so these results are unfiltered by date."),
                    );
                }
                return Ok(relaxed);
            }
        }
    }

    Ok(res_val)
}
