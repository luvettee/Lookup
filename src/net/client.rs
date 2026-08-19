use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, ACCEPT_ENCODING, CONTENT_TYPE};
use reqwest::redirect::Policy;
use serde_json::Value;

use crate::config::{MAX_HTML_RESPONSE_BYTES, MAX_JSON_RESPONSE_BYTES, NETWORK_TIMEOUT, USER_AGENT as APP_USER_AGENT};
use crate::net::ssrf::validate_url;

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    let redirect_policy = Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            return attempt.error("too many redirects");
        }
        let next_url = attempt.url().as_str();
        if let Err(e) = validate_url(next_url, true) {
            return attempt.error(e);
        }
        attempt.follow()
    });

    reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .redirect(redirect_policy)
        .timeout(NETWORK_TIMEOUT)
        .build()
        .expect("Failed to build HTTP client")
});

pub fn get_client() -> &'static reqwest::Client {
    &CLIENT
}

pub async fn get_json(
    url: &str,
    timeout: Option<Duration>,
    max_bytes: Option<usize>,
    headers: Option<HashMap<String, String>>,
) -> Result<Value, String> {
    let safe_url = validate_url(url, true)?;
    let client = get_client();
    let max_b = max_bytes.unwrap_or(MAX_JSON_RESPONSE_BYTES);

    let mut req = client.get(&safe_url);
    if let Some(t) = timeout {
        req = req.timeout(t);
    }

    let mut header_map = HeaderMap::new();
    header_map.insert(ACCEPT, HeaderValue::from_static("application/json"));
    header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

    if let Some(hdrs) = headers {
        for (k, v) in hdrs {
            if let (Ok(name), Ok(val)) = (k.parse::<HeaderName>(), v.parse::<HeaderValue>()) {
                header_map.insert(name, val);
            }
        }
    }
    req = req.headers(header_map);

    let resp = req.send().await.map_err(|e| {
        if e.is_timeout() {
            "timeout".to_string()
        } else if let Some(status) = e.status() {
            format!("HTTP {} from JSON provider", status.as_u16())
        } else {
            "JSON request failed".to_string()
        }
    })?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {} from JSON provider", status.as_u16()));
    }

    if let Some(ct) = resp.headers().get(CONTENT_TYPE) {
        let ct_str = ct.to_str().unwrap_or("").to_lowercase();
        if !ct_str.contains("json") {
            return Err("Expected a JSON response".to_string());
        }
    }

    if let Some(cl) = resp.content_length() {
        if cl as usize > max_b {
            return Err("JSON response is too large".to_string());
        }
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|_| "JSON request failed".to_string())?;

    if bytes.len() > max_b {
        return Err("JSON response is too large".to_string());
    }

    let val: Value = serde_json::from_slice(&bytes).map_err(|_| "JSON response must be an object".to_string())?;
    if !val.is_object() {
        return Err("JSON response must be an object".to_string());
    }

    Ok(val)
}

pub async fn post_json(
    url: &str,
    body: &Value,
    timeout: Option<Duration>,
    max_bytes: Option<usize>,
    headers: Option<HashMap<String, String>>,
) -> Result<Value, String> {
    let safe_url = validate_url(url, true)?;
    let client = get_client();
    let max_b = max_bytes.unwrap_or(MAX_JSON_RESPONSE_BYTES);

    let mut req = client.post(&safe_url);
    if let Some(t) = timeout {
        req = req.timeout(t);
    }

    let mut header_map = HeaderMap::new();
    header_map.insert(ACCEPT, HeaderValue::from_static("application/json"));
    header_map.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

    if let Some(hdrs) = headers {
        for (k, v) in hdrs {
            if let (Ok(name), Ok(val)) = (k.parse::<HeaderName>(), v.parse::<HeaderValue>()) {
                header_map.insert(name, val);
            }
        }
    }
    req = req.headers(header_map).json(body);

    let resp = req.send().await.map_err(|e| {
        if e.is_timeout() {
            "timeout".to_string()
        } else if let Some(status) = e.status() {
            format!("HTTP {} from JSON provider", status.as_u16())
        } else {
            "JSON request failed".to_string()
        }
    })?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {} from JSON provider", status.as_u16()));
    }

    if let Some(ct) = resp.headers().get(CONTENT_TYPE) {
        let ct_str = ct.to_str().unwrap_or("").to_lowercase();
        if !ct_str.contains("json") {
            return Err("Expected a JSON response".to_string());
        }
    }

    if let Some(cl) = resp.content_length() {
        if cl as usize > max_b {
            return Err("JSON response is too large".to_string());
        }
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|_| "JSON request failed".to_string())?;

    if bytes.len() > max_b {
        return Err("JSON response is too large".to_string());
    }

    let val: Value = serde_json::from_slice(&bytes).map_err(|_| "JSON response must be an object".to_string())?;
    if !val.is_object() {
        return Err("JSON response must be an object".to_string());
    }

    Ok(val)
}

pub struct HtmlResponse {
    pub raw_html: String,
    pub final_url: String,
}

pub async fn fetch_html(
    url: &str,
    max_chars: usize,
    timeout: Option<Duration>,
) -> Result<HtmlResponse, String> {
    let safe_url = validate_url(url, true)?;
    let client = get_client();
    let max_bytes = MAX_HTML_RESPONSE_BYTES.min(64_000.max(max_chars.saturating_mul(8)));

    let mut req = client.get(&safe_url);
    if let Some(t) = timeout {
        req = req.timeout(t);
    } else {
        req = req.timeout(Duration::from_secs(10));
    }

    let mut header_map = HeaderMap::new();
    header_map.insert(ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,*/*"));
    header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    req = req.headers(header_map);

    let resp = req.send().await.map_err(|e| {
        if let Some(status) = e.status() {
            format!("HTTP error {} fetching {}", status.as_u16(), url)
        } else {
            format!("Could not reach {}", url)
        }
    })?;

    let final_url = validate_url(resp.url().as_str(), true)
        .unwrap_or_else(|_| safe_url.clone());

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP error {} fetching {}", status.as_u16(), url));
    }

    if let Some(ct) = resp.headers().get(CONTENT_TYPE) {
        let ct_str = ct.to_str().unwrap_or("").to_lowercase();
        if !ct_str.is_empty()
            && !ct_str.contains("text/html")
            && !ct_str.contains("application/xhtml+xml")
            && !ct_str.contains("text/plain")
        {
            return Err(format!("Unsupported content type: {}", ct_str));
        }
    }

    if let Some(cl) = resp.content_length() {
        if cl as usize > max_bytes {
            return Err("HTML response is too large".to_string());
        }
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Could not reach {}: {}", url, e))?;

    if bytes.len() > max_bytes {
        return Err("HTML response is too large".to_string());
    }

    let raw_html = String::from_utf8_lossy(&bytes).to_string();

    Ok(HtmlResponse {
        raw_html,
        final_url,
    })
}

pub async fn fetch_bytes(
    url: &str,
    max_bytes: usize,
    timeout: Option<Duration>,
    headers: Option<HashMap<String, String>>,
) -> Result<Vec<u8>, String> {
    let safe_url = validate_url(url, true)?;
    let client = get_client();

    let mut req = client.get(&safe_url);
    if let Some(t) = timeout {
        req = req.timeout(t);
    }

    let mut header_map = HeaderMap::new();
    if let Some(hdrs) = headers {
        for (k, v) in hdrs {
            if let (Ok(name), Ok(val)) = (k.parse::<HeaderName>(), v.parse::<HeaderValue>()) {
                header_map.insert(name, val);
            }
        }
    }
    req = req.headers(header_map);

    let resp = req.send().await.map_err(|e| {
        if e.is_timeout() {
            "timeout".to_string()
        } else if let Some(status) = e.status() {
            format!("HTTP {}", status.as_u16())
        } else {
            "Request failed".to_string()
        }
    })?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }

    if let Some(cl) = resp.content_length() {
        if cl as usize > max_bytes {
            return Err("Response is too large".to_string());
        }
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|_| "Request failed".to_string())?;

    if bytes.len() > max_bytes {
        return Err("Response is too large".to_string());
    }

    Ok(bytes.to_vec())
}
