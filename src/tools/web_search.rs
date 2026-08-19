use std::collections::HashMap;
use std::time::Duration;
use serde_json::Value;

use crate::budget::enforce_output_budget;
use crate::cache::{cache_get, cache_put};
use crate::config::{SEARCH_PROVIDERS, MAX_QUERY_CHARS};
use crate::guard::{check_search_guard, mark_search_guard_failure, normalize_query};
use crate::providers::do_search;
use crate::tools::torrent::{is_torrent_query, torrent_search};

pub async fn web_search(args: &HashMap<String, Value>) -> Result<Value, String> {
    let raw_query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) if !q.trim().is_empty() => q.trim(),
        _ => return Err("query must not be empty".to_string()),
    };

    if raw_query.len() > MAX_QUERY_CHARS {
        return Err("query is too long".to_string());
    }

    if is_torrent_query(raw_query) {
        let mut t_args = args.clone();
        t_args.remove("domain");
        t_args.remove("recency");
        return torrent_search(&t_args).await;
    }

    let provider = match args.get("provider").and_then(|v| v.as_str()) {
        Some(p) => {
            if !SEARCH_PROVIDERS.contains(&p) {
                return Err(format!("provider must be one of: {}", SEARCH_PROVIDERS.join(", ")));
            }
            p
        }
        None => "auto",
    };

    let count = match args.get("max_results") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(5).clamp(1, 20) as usize,
        _ => 5,
    };

    let domain = args
        .get("domain")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let recency = match args.get("recency").and_then(|v| v.as_str()) {
        Some(r) => {
            let lower = r.to_lowercase();
            if !["day", "week", "month", "year"].contains(&lower.as_str()) {
                return Err("recency must be one of: day, week, month, year".to_string());
            }
            Some(lower)
        }
        None => None,
    };

    let scope = args
        .get("__activity_scope")
        .and_then(|v| v.as_str())
        .unwrap_or("stdio");

    let norm_q = normalize_query(raw_query);
    let cache_key = format!(
        "search:{}:{}:{}:{}:{}",
        provider,
        norm_q,
        count,
        domain.unwrap_or("").to_lowercase(),
        recency.as_deref().unwrap_or("")
    );

    if let Some(cached) = cache_get(&cache_key) {
        return Ok(cached);
    }

    check_search_guard(scope, "web_search", raw_query)?;

    let search_res = do_search(
        provider,
        raw_query,
        count,
        domain,
        recency.as_deref(),
        false,
    )
    .await;

    match search_res {
        Ok(val) => {
            let budgeted = enforce_output_budget(val, None);
            cache_put(&cache_key, Duration::from_secs(300), budgeted.clone());
            Ok(budgeted)
        }
        Err(err) => {
            if provider == "auto" && err.starts_with("All search providers failed") {
                mark_search_guard_failure(scope);
            }
            Err(err)
        }
    }
}
