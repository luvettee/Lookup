use std::collections::HashMap;
use serde_json::{json, Value};
use url::form_urlencoded;

use crate::config::{
    brave_api_key, brave_freshness, BRAVE_NEWS_SEARCH_URL, BRAVE_WEB_SEARCH_URL,
};
use crate::net::get_json;

pub fn require_brave_api_key() -> Result<String, String> {
    brave_api_key().ok_or_else(|| "Brave API key not configured".to_string())
}

pub fn parse_brave_items(payload: &Value, news: bool) -> Vec<Value> {
    let raw = if news {
        payload.get("results").and_then(|v| v.as_array())
    } else {
        payload
            .get("web")
            .and_then(|w| w.get("results"))
            .and_then(|v| v.as_array())
    };

    let arr = match raw {
        Some(a) => a,
        None => return Vec::new(),
    };

    let mut items = Vec::new();
    for raw_item in arr {
        let title = raw_item.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let url = raw_item.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let description = raw_item.get("description").and_then(|v| v.as_str()).unwrap_or("");

        let published = raw_item
            .get("page_age")
            .or_else(|| raw_item.get("age"))
            .and_then(|v| v.as_str());

        let profile = raw_item.get("profile").and_then(|v| v.as_object());
        let meta_url = raw_item.get("meta_url").and_then(|v| v.as_object());

        let source = profile
            .and_then(|p| p.get("long_name").or_else(|| p.get("name")))
            .and_then(|v| v.as_str())
            .or_else(|| {
                meta_url
                    .and_then(|m| m.get("hostname").or_else(|| m.get("netloc")))
                    .and_then(|v| v.as_str())
            });

        let mut item_map = serde_json::Map::new();
        item_map.insert("title".to_string(), json!(title));
        item_map.insert("url".to_string(), json!(url));
        item_map.insert("description".to_string(), json!(description));

        if let Some(pub_date) = published {
            item_map.insert("published_at".to_string(), json!(pub_date));
        }
        if let Some(src) = source {
            item_map.insert("source".to_string(), json!(src));
        }

        items.push(Value::Object(item_map));
    }

    items
}

pub async fn search_brave(
    query: &str,
    count: usize,
    domain: Option<&str>,
    recency: Option<&str>,
    news: bool,
) -> Result<Value, String> {
    let key = require_brave_api_key()?;
    let q = if let Some(d) = domain.filter(|s| !s.trim().is_empty()) {
        format!("{} site:{}", query, d.trim())
    } else {
        query.to_string()
    };

    if q.len() > 400 || q.split_whitespace().count() > 50 {
        return Err("Brave queries must be at most 400 characters and 50 words".to_string());
    }

    let mut params = form_urlencoded::Serializer::new(String::new());
    params.append_pair("q", &q);
    params.append_pair("count", &count.to_string());
    params.append_pair("search_lang", "en");
    params.append_pair("safesearch", if news { "strict" } else { "moderate" });

    if let Some(rec) = recency {
        if let Some(freshness) = brave_freshness(rec) {
            params.append_pair("freshness", freshness);
        }
    }

    let endpoint = if news {
        BRAVE_NEWS_SEARCH_URL
    } else {
        BRAVE_WEB_SEARCH_URL
    };
    let url = format!("{}?{}", endpoint, params.finish());

    let mut headers = HashMap::new();
    headers.insert("X-Subscription-Token".to_string(), key);

    let payload = get_json(&url, None, None, Some(headers)).await?;
    let items = parse_brave_items(&payload, news);

    Ok(json!({
        "results": items
    }))
}
