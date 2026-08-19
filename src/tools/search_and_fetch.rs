use std::collections::{HashMap, HashSet};
use std::time::Duration;
use serde_json::{json, Value};
use tracing::debug;

use crate::budget::enforce_output_budget;
use crate::cache::{cache_get, cache_put};
use crate::config::{MAX_QUERY_CHARS, MAX_TOOL_OUTPUT_CHARS, SEARCH_PROVIDERS};
use crate::guard::{check_search_guard, normalize_query};
use crate::net::normalize_url_key;
use crate::providers::{do_search, fetch_provider};
use crate::tools::torrent::{is_torrent_query, torrent_search};

pub fn collect_sources(search_result: &Value, limit: usize) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut picked = Vec::new();

    if let Some(arr) = search_result.get("results").and_then(|v| v.as_array()) {
        for r in arr {
            if let Some(url) = r.get("url").and_then(|v| v.as_str()) {
                let canonical = normalize_url_key(url);
                if seen.contains(&canonical) {
                    continue;
                }
                seen.insert(canonical);
                picked.push(r.clone());
                if picked.len() >= limit {
                    break;
                }
            }
        }
    }

    picked
}

pub async fn read_sources(results: Vec<Value>, max_chars: usize) -> Vec<Value> {
    if results.is_empty() {
        return Vec::new();
    }

    let count = results.len();
    let per_source = 300.max(max_chars.min((MAX_TOOL_OUTPUT_CHARS.saturating_sub(200)) / count));

    let mut futs = Vec::new();
    for r in results {
        let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let snippet = r.get("snippet").and_then(|v| v.as_str()).unwrap_or("").to_string();

        futs.push(async move {
            match fetch_provider("auto", &url, per_source).await {
                Ok(data) => {
                    let final_title = if !data.title.is_empty() {
                        data.title
                    } else {
                        title
                    };
                    json!({
                        "title": final_title,
                        "url": data.final_url,
                        "snippet": snippet,
                        "content": data.content,
                        "fetch_provider": data.provider
                    })
                }
                Err(e) => {
                    debug!("Fetch failed for {}: {}", url, e);
                    json!({
                        "url": url,
                        "error": e
                    })
                }
            }
        });
    }

    futures::future::join_all(futs).await
}

pub async fn search_and_fetch(args: &HashMap<String, Value>) -> Result<Value, String> {
    let raw_query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) if !q.trim().is_empty() => q.trim(),
        _ => return Err("query must not be empty".to_string()),
    };

    if raw_query.len() > MAX_QUERY_CHARS {
        return Err("query is too long".to_string());
    }

    if is_torrent_query(raw_query) {
        let mut t_args = args.clone();
        t_args.insert("max_results".to_string(), json!(args.get("max_results").and_then(|v| v.as_u64()).unwrap_or(4)));
        t_args.remove("fetch_results");
        t_args.remove("max_chars");
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

    let max_results = match args.get("max_results") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(4).clamp(1, 10) as usize,
        _ => 4,
    };

    let fetch_results = match args.get("fetch_results") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(2).clamp(1, 5) as usize,
        _ => 2,
    };

    let max_chars = match args.get("max_chars") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(4000).clamp(500, 30000) as usize,
        _ => 4000,
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
        "search_and_fetch:{}:{}:{}:{}:{}:{}:{}",
        provider,
        norm_q,
        max_results,
        fetch_results,
        max_chars,
        domain.unwrap_or("").to_lowercase(),
        recency.as_deref().unwrap_or("")
    );

    if let Some(cached) = cache_get(&cache_key) {
        return Ok(cached);
    }

    check_search_guard(scope, "search_and_fetch", raw_query)?;

    let search_res = do_search(
        provider,
        raw_query,
        max_results,
        domain,
        recency.as_deref(),
        false,
    )
    .await?;

    let picked = collect_sources(&search_res, fetch_results);
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
