use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;

use crate::config::ollama_api_key;
use crate::net::post_json;

pub fn require_ollama_api_key() -> Result<String, String> {
    ollama_api_key().ok_or_else(|| "Ollama API key not configured".to_string())
}

fn ollama_host() -> String {
    env::var("OLLAMA_HOST")
        .unwrap_or_else(|_| "https://api.ollama.com".to_string())
        .trim_end_matches('/')
        .to_string()
}

pub async fn search_ollama(
    query: &str,
    count: usize,
    domain: Option<&str>,
    recency: Option<&str>,
) -> Result<Value, String> {
    let key = require_ollama_api_key()?;
    if domain.is_some() || recency.is_some() {
        return Err("Ollama search does not support domain or recency filters".to_string());
    }

    let url = format!("{}/api/web_search", ollama_host());
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), format!("Bearer {}", key));

    let body = json!({
        "query": query,
        "max_results": count
    });

    post_json(&url, &body, None, None, Some(headers)).await
}

pub async fn fetch_ollama(url_target: &str) -> Result<Value, String> {
    let key = require_ollama_api_key()?;
    let url = format!("{}/api/web_fetch", ollama_host());
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), format!("Bearer {}", key));

    let body = json!({
        "url": url_target
    });

    post_json(&url, &body, None, None, Some(headers)).await
}
