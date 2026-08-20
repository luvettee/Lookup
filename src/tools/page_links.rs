use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

use crate::budget::enforce_output_budget;
use crate::cache::{cache_get, cache_put};
use crate::net::{normalize_url_key, validate_url};
use crate::providers::direct_fetch;

pub async fn page_links(args: &HashMap<String, Value>) -> Result<Value, String> {
    let raw_url = match args.get("url").and_then(|v| v.as_str()) {
        Some(u) if !u.trim().is_empty() => u.trim(),
        _ => return Err("url must not be empty".to_string()),
    };

    let valid_url = validate_url(raw_url, false)?;

    let max_links = match args.get("max_links") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(10).clamp(1, 25) as usize,
        _ => 10,
    };

    let cache_key = format!("links:{}:{}", normalize_url_key(&valid_url), max_links);

    if let Some(cached) = cache_get(&cache_key) {
        return Ok(cached);
    }

    let data = direct_fetch(&valid_url, 8000).await?;
    let mut links = data.links;
    links.truncate(max_links);

    let output = json!({
        "url": valid_url,
        "links": links
    });

    let budgeted = enforce_output_budget(output, None);
    cache_put(&cache_key, Duration::from_secs(300), budgeted.clone());
    Ok(budgeted)
}
