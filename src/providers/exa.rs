use std::collections::HashMap;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::{json, Value};
use url::Url;

use crate::config::{exa_api_key, recency_to_days, EXA_SEARCH_URL};
use crate::net::post_json;

pub fn require_exa_api_key() -> Result<String, String> {
    exa_api_key().ok_or_else(|| "Exa API key not configured".to_string())
}

pub fn parse_exa_items(payload: &Value) -> Vec<Value> {
    let raw = match payload.get("results").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };

    let mut items = Vec::new();
    for raw_item in raw {
        let title = raw_item.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let url = raw_item.get("url").and_then(|v| v.as_str()).unwrap_or("");

        let summary = raw_item.get("summary").and_then(|v| v.as_str());
        let text = raw_item.get("text").and_then(|v| v.as_str());
        let highlights = raw_item.get("highlights").and_then(|v| v.as_array());

        let snippet = summary
            .or(text)
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                if let Some(h_arr) = highlights {
                    h_arr
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    String::new()
                }
            });

        let source = Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .unwrap_or_default();

        let mut item_map = serde_json::Map::new();
        item_map.insert("title".to_string(), json!(title));
        item_map.insert("url".to_string(), json!(url));
        item_map.insert("description".to_string(), json!(snippet));

        if let Some(pub_date) = raw_item.get("publishedDate").and_then(|v| v.as_str()) {
            item_map.insert("publishedDate".to_string(), json!(pub_date));
        }
        if !source.is_empty() {
            item_map.insert("source".to_string(), json!(source));
        }

        items.push(Value::Object(item_map));
    }

    items
}

pub async fn search_exa(
    query: &str,
    count: usize,
    domain: Option<&str>,
    recency: Option<&str>,
    news: bool,
) -> Result<Value, String> {
    let key = require_exa_api_key()?;

    let mut body_map = serde_json::Map::new();
    body_map.insert("query".to_string(), json!(query));
    body_map.insert("numResults".to_string(), json!(count));
    body_map.insert("type".to_string(), json!("auto"));
    body_map.insert("contents".to_string(), json!({"highlights": true}));

    if let Some(d) = domain.filter(|s| !s.trim().is_empty()) {
        body_map.insert("includeDomains".to_string(), json!([d.trim()]));
    }

    if let Some(rec) = recency {
        if let Some(days) = recency_to_days(rec) {
            let start = Utc::now() - ChronoDuration::days(days as i64);
            let start_iso = start.format("%Y-%m-%dT%H:%M:%SZ").to_string();
            body_map.insert("startPublishedDate".to_string(), json!(start_iso));
        }
    }

    if news {
        body_map.insert("category".to_string(), json!("news"));
    }

    let mut headers = HashMap::new();
    headers.insert("x-api-key".to_string(), key);

    let payload = post_json(
        EXA_SEARCH_URL,
        &Value::Object(body_map),
        None,
        None,
        Some(headers),
    )
    .await?;

    let items = parse_exa_items(&payload);
    Ok(json!({
        "results": items
    }))
}
