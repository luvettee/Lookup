use std::collections::HashMap;
use std::time::Duration;
use serde_json::{json, Value};

use crate::budget::enforce_output_budget;
use crate::cache::{cache_get, cache_put};
use crate::config::FETCH_PROVIDERS;
use crate::net::{normalize_url_key, validate_url};
use crate::providers::fetch_provider;

pub async fn read_url(args: &HashMap<String, Value>) -> Result<Value, String> {
    let raw_url = match args.get("url").and_then(|v| v.as_str()) {
        Some(u) if !u.trim().is_empty() => u.trim(),
        _ => return Err("url must not be empty".to_string()),
    };

    let valid_url = validate_url(raw_url, false)?;

    let provider = match args.get("provider").and_then(|v| v.as_str()) {
        Some(p) => {
            if !FETCH_PROVIDERS.contains(&p) {
                return Err(format!("provider must be one of: {}", FETCH_PROVIDERS.join(", ")));
            }
            p
        }
        None => "auto",
    };

    let max_chars = match args.get("max_chars") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(6000).clamp(500, 30000) as usize,
        _ => 6000,
    };

    let include_links = match args.get("include_links") {
        Some(Value::Bool(b)) => *b,
        _ => false,
    };

    let include_metadata = match args.get("include_metadata") {
        Some(Value::Bool(b)) => *b,
        _ => false,
    };

    let cache_key = format!(
        "fetch:{}:{}:{}:{}:{}",
        provider,
        normalize_url_key(&valid_url),
        max_chars,
        include_links,
        include_metadata
    );

    if let Some(cached) = cache_get(&cache_key) {
        return Ok(cached);
    }

    let data = fetch_provider(provider, &valid_url, max_chars).await?;

    let mut out_map = serde_json::Map::new();
    out_map.insert("url".to_string(), json!(valid_url));
    out_map.insert("final_url".to_string(), json!(data.final_url));
    out_map.insert("content".to_string(), json!(data.content));
    out_map.insert("fetch_provider".to_string(), json!(data.provider));

    if include_metadata {
        out_map.insert("title".to_string(), json!(data.title));
        out_map.insert("description".to_string(), json!(data.description));
    }
    if include_links {
        out_map.insert("links".to_string(), json!(data.links));
    }

    let budgeted = enforce_output_budget(Value::Object(out_map), None);
    cache_put(&cache_key, Duration::from_secs(300), budgeted.clone());
    Ok(budgeted)
}
