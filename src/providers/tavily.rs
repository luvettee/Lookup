use serde_json::{json, Value};

use crate::config::{recency_to_days, tavily_api_key};
use crate::net::post_json;

pub fn require_tavily_api_key() -> Result<String, String> {
    tavily_api_key().ok_or_else(|| "Tavily API key not configured".to_string())
}

pub async fn search_tavily(
    query: &str,
    count: usize,
    domain: Option<&str>,
    recency: Option<&str>,
    news: bool,
) -> Result<Value, String> {
    let key = require_tavily_api_key()?;

    let mut body_map = serde_json::Map::new();
    body_map.insert("api_key".to_string(), json!(key));
    body_map.insert("query".to_string(), json!(query));
    body_map.insert("max_results".to_string(), json!(count));

    if let Some(d) = domain.filter(|s| !s.trim().is_empty()) {
        body_map.insert("include_domains".to_string(), json!([d.trim()]));
    }

    if let Some(rec) = recency {
        if let Some(days) = recency_to_days(rec) {
            body_map.insert("days".to_string(), json!(days));
        }
    }

    if news {
        body_map.insert("topic".to_string(), json!("news"));
    }

    post_json(
        "https://api.tavily.com/search",
        &Value::Object(body_map),
        None,
        None,
        None,
    )
    .await
}

pub async fn extract_tavily(url: &str) -> Result<Value, String> {
    let key = require_tavily_api_key()?;

    let mut body_map = serde_json::Map::new();
    body_map.insert("api_key".to_string(), json!(key));
    body_map.insert("urls".to_string(), json!([url]));

    post_json(
        "https://api.tavily.com/extract",
        &Value::Object(body_map),
        None,
        None,
        None,
    )
    .await
}
