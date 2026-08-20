use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

use crate::budget::enforce_output_budget;
use crate::cache::{cache_get, cache_put};
use crate::config::{MAX_QUERY_CHARS, SEARCH_PROVIDERS};
use crate::guard::{check_search_guard, normalize_query};
use crate::providers::do_search;
use crate::tools::search_and_fetch::{collect_sources, read_sources};

pub async fn research(args: &HashMap<String, Value>) -> Result<Value, String> {
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
                return Err(format!(
                    "provider must be one of: {}",
                    SEARCH_PROVIDERS.join(", ")
                ));
            }
            p
        }
        None => "auto",
    };

    let max_sources = match args.get("max_sources") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(3).clamp(1, 10) as usize,
        _ => 3,
    };

    let max_chars = match args.get("max_chars_per_source") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(5000).clamp(500, 50000) as usize,
        _ => 5000,
    };

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
        "research:{}:{}:{}:{}:{}",
        provider,
        norm_q,
        max_sources,
        max_chars,
        recency.as_deref().unwrap_or("")
    );

    if let Some(cached) = cache_get(&cache_key) {
        return Ok(cached);
    }

    check_search_guard(scope, "research", raw_query)?;

    let search_res = do_search(
        provider,
        raw_query,
        max_sources * 3,
        None,
        recency.as_deref(),
        false,
    )
    .await?;

    let picked = collect_sources(&search_res, max_sources);
    let read_res = read_sources(picked, max_chars).await;

    let output = json!({
        "query": raw_query,
        "search_provider": search_res.get("provider").and_then(|v| v.as_str()).unwrap_or(provider),
        "sources": read_res
    });

    let budgeted = enforce_output_budget(output, None);
    cache_put(&cache_key, Duration::from_secs(300), budgeted.clone());
    Ok(budgeted)
}
