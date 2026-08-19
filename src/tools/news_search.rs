use std::collections::HashMap;
use std::time::Duration;
use serde_json::Value;

use crate::budget::enforce_output_budget;
use crate::cache::{cache_get, cache_put};
use crate::config::{MAX_QUERY_CHARS, SEARCH_PROVIDERS};
use crate::guard::{check_search_guard, normalize_query};
use crate::providers::do_search;

pub async fn news_search(args: &HashMap<String, Value>) -> Result<Value, String> {
    let raw_query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) if !q.trim().is_empty() => q.trim(),
        _ => return Err("query must not be empty".to_string()),
    };

    if raw_query.len() > MAX_QUERY_CHARS {
        return Err("query is too long".to_string());
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
        Some(Value::Number(n)) => n.as_u64().unwrap_or(5).clamp(1, 10) as usize,
        _ => 5,
    };

    let recency = match args.get("recency").and_then(|v| v.as_str()) {
        Some(r) => {
            let lower = r.to_lowercase();
            if !["day", "week", "month", "year"].contains(&lower.as_str()) {
                return Err("recency must be one of: day, week, month, year".to_string());
            }
            lower
        }
        None => "week".to_string(),
    };

    let scope = args
        .get("__activity_scope")
        .and_then(|v| v.as_str())
        .unwrap_or("stdio");

    let norm_q = normalize_query(raw_query);
    let cache_key = format!("news:{}:{}:{}:{}", provider, norm_q, count, recency);

    if let Some(cached) = cache_get(&cache_key) {
        return Ok(cached);
    }

    check_search_guard(scope, "news_search", raw_query)?;

    let search_res = do_search(
        provider,
        raw_query,
        count,
        None,
        Some(&recency),
        true,
    )
    .await?;

    let budgeted = enforce_output_budget(search_res, None);
    cache_put(&cache_key, Duration::from_secs(180), budgeted.clone());
    Ok(budgeted)
}
