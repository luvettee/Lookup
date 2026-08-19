pub mod brave;
pub mod exa;
pub mod ollama;
pub mod searxng;
pub mod tavily;

use std::collections::HashMap;
use serde_json::{json, Value};
use tracing::debug;

use crate::config::{
    brave_api_key, exa_api_key, ollama_api_key, tavily_api_key,
};
use crate::health::{attempt_provider, health_available};
use crate::html::{parse_html, resolve_links, truncate_text, ExtractedLink, ParsedPage};
use crate::net::{fetch_html, validate_url};

pub fn normalize_results(
    provider: &str,
    query: &str,
    payload: Value,
    count: usize,
) -> Value {
    let metadata = payload.as_object().cloned().unwrap_or_default();
    let raw_arr = payload
        .get("results")
        .or_else(|| payload.get("data"))
        .or_else(|| payload.get("items"))
        .and_then(|v| v.as_array())
        .or_else(|| payload.as_array());

    let mut results = Vec::new();

    if let Some(items) = raw_arr {
        for item in items.iter().take(count) {
            let item_obj = match item.as_object() {
                Some(o) => o,
                None => continue,
            };

            let raw_url = item_obj
                .get("url")
                .or_else(|| item_obj.get("link"))
                .or_else(|| item_obj.get("href"))
                .and_then(|v| v.as_str());

            let url = match raw_url {
                Some(u) => match validate_url(u, false) {
                    Ok(valid) => valid,
                    Err(_) => continue,
                },
                None => continue,
            };

            let mut snippet = String::new();
            for key in &["content", "snippet", "description", "text", "abstract"] {
                if let Some(val) = item_obj.get(*key).and_then(|v| v.as_str()) {
                    let trimmed = val.trim();
                    if !trimmed.is_empty() {
                        snippet = trimmed.to_string();
                        break;
                    }
                }
            }

            let title = item_obj
                .get("title")
                .or_else(|| item_obj.get("name"))
                .or_else(|| item_obj.get("headline"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let mut res_map = serde_json::Map::new();
            res_map.insert("title".to_string(), json!(title));
            res_map.insert("url".to_string(), json!(url));
            res_map.insert("snippet".to_string(), json!(snippet));

            for key in &[
                "published_at",
                "publishedAt",
                "published_date",
                "publishedDate",
                "pubDate",
                "date",
            ] {
                if let Some(pub_val) = item_obj.get(*key) {
                    if let Some(pub_str) = pub_val.as_str() {
                        res_map.insert("published_at".to_string(), json!(pub_str));
                        break;
                    }
                }
            }

            for key in &["source", "site_name", "domain", "engine"] {
                if let Some(src_val) = item_obj.get(*key) {
                    if let Some(src_str) = src_val.as_str() {
                        res_map.insert("source".to_string(), json!(src_str));
                        break;
                    }
                }
            }

            results.push(Value::Object(res_map));
        }
    }

    if provider == "searxng" {
        results = searxng::rank_searxng_results(query, results);
    }

    let mut normalized = serde_json::Map::new();
    normalized.insert("provider".to_string(), json!(provider));
    normalized.insert("query".to_string(), json!(query));
    normalized.insert("results".to_string(), json!(results));

    for key in &["recency_relaxed", "requested_recency", "filter_notice"] {
        if let Some(val) = metadata.get(*key) {
            normalized.insert(key.to_string(), val.clone());
        }
    }

    Value::Object(normalized)
}

pub fn extract_fetch(payload: &Value, url: &str, max_chars: usize) -> ParsedPage {
    let mut title = String::new();
    let mut description = String::new();
    let mut content = String::new();
    let mut raw_links = Vec::new();
    let mut final_url = url.to_string();

    if let Some(obj) = payload.as_object() {
        let results = obj.get("results").or_else(|| obj.get("data")).and_then(|v| v.as_array());
        let matched = results.and_then(|arr| {
            arr.iter()
                .find(|item| item.get("url").and_then(|v| v.as_str()).is_some())
        });

        if let Some(m) = matched {
            if let Some(u) = m.get("url").and_then(|v| v.as_str()) {
                final_url = u.to_string();
            }
            if let Some(t) = m.get("title").and_then(|v| v.as_str()) {
                title = t.to_string();
            }
            if let Some(d) = m.get("description").and_then(|v| v.as_str()) {
                description = d.to_string();
            }
            if let Some(c) = m
                .get("raw_content")
                .or_else(|| m.get("content"))
                .or_else(|| m.get("text"))
                .and_then(|v| v.as_str())
            {
                content = c.to_string();
            }
            if let Some(links_arr) = m.get("links").or_else(|| m.get("anchors")).and_then(|v| v.as_array()) {
                for l in links_arr {
                    if let Some(s) = l.as_str() {
                        raw_links.push(ExtractedLink {
                            text: String::new(),
                            url: s.to_string(),
                        });
                    } else if let Some(lo) = l.as_object() {
                        let l_url = lo
                            .get("url")
                            .or_else(|| lo.get("href"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let l_text = lo.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        raw_links.push(ExtractedLink {
                            text: l_text,
                            url: l_url,
                        });
                    }
                }
            }
        }

        if content.is_empty() {
            if let Some(c) = obj
                .get("raw_content")
                .or_else(|| obj.get("content"))
                .or_else(|| obj.get("text"))
                .and_then(|v| v.as_str())
            {
                content = c.to_string();
            }
        }

        if title.is_empty() {
            if let Some(t) = obj.get("title").and_then(|v| v.as_str()) {
                title = t.to_string();
            }
        }

        if description.is_empty() {
            if let Some(d) = obj.get("description").and_then(|v| v.as_str()) {
                description = d.to_string();
            }
        }
    }

    let cleaned_content = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated = truncate_text(&cleaned_content, max_chars);
    let resolved = resolve_links(&raw_links, &final_url);
    let safe_final_url = validate_url(&final_url, false).unwrap_or_else(|_| url.to_string());

    ParsedPage {
        title,
        description,
        content: truncated,
        links: resolved,
        final_url: safe_final_url,
        provider: "unknown".to_string(),
    }
}

pub async fn direct_fetch(url: &str, max_chars: usize) -> Result<ParsedPage, String> {
    let resp = fetch_html(url, max_chars, None).await?;
    let mut parsed = parse_html(&resp.raw_html, &resp.final_url, max_chars);
    parsed.provider = "direct".to_string();
    Ok(parsed)
}

pub async fn fetch_provider(provider: &str, url: &str, max_chars: usize) -> Result<ParsedPage, String> {
    let safe_url = validate_url(url, false)?;

    if provider == "auto" {
        let mut errors = HashMap::new();

        if ollama_api_key().is_some() && health_available("fetch:ollama") {
            let res = attempt_provider("fetch:ollama", || async {
                let payload = ollama::fetch_ollama(&safe_url).await?;
                let mut page = extract_fetch(&payload, &safe_url, max_chars);
                page.provider = "ollama".to_string();
                Ok(page)
            })
            .await;
            match res {
                Ok(page) => return Ok(page),
                Err(e) => {
                    errors.insert("Ollama", e);
                }
            }
        } else if ollama_api_key().is_none() {
            errors.insert("Ollama", "API key not configured".to_string());
        } else {
            errors.insert("Ollama", "cooling down".to_string());
        }

        if tavily_api_key().is_some() && health_available("fetch:tavily") {
            let res = attempt_provider("fetch:tavily", || async {
                let payload = tavily::extract_tavily(&safe_url).await?;
                let mut page = extract_fetch(&payload, &safe_url, max_chars);
                page.provider = "tavily".to_string();
                Ok(page)
            })
            .await;
            match res {
                Ok(page) => return Ok(page),
                Err(e) => {
                    errors.insert("Tavily", e);
                }
            }
        } else if tavily_api_key().is_none() {
            errors.insert("Tavily", "API key not configured".to_string());
        } else {
            errors.insert("Tavily", "cooling down".to_string());
        }

        match direct_fetch(&safe_url, max_chars).await {
            Ok(page) => return Ok(page),
            Err(e) => {
                errors.insert("Direct", e);
            }
        }

        let mut lines = vec!["All fetch providers failed.".to_string()];
        for (k, v) in errors {
            lines.push(format!("{}: {}", k, v));
        }
        return Err(lines.join("\n"));
    }

    if provider == "ollama" {
        let payload = ollama::fetch_ollama(&safe_url).await?;
        let mut page = extract_fetch(&payload, &safe_url, max_chars);
        page.provider = "ollama".to_string();
        return Ok(page);
    }

    if provider == "tavily" {
        let payload = tavily::extract_tavily(&safe_url).await?;
        let mut page = extract_fetch(&payload, &safe_url, max_chars);
        page.provider = "tavily".to_string();
        return Ok(page);
    }

    direct_fetch(&safe_url, max_chars).await
}

async fn execute_single_provider(
    name: &str,
    query: &str,
    count: usize,
    domain: Option<&str>,
    recency: Option<&str>,
    news: bool,
) -> Result<Value, String> {
    match name {
        "brave" => brave::search_brave(query, count, domain, recency, news).await,
        "exa" => exa::search_exa(query, count, domain, recency, news).await,
        "ollama" => {
            if news && recency.is_some() {
                Err("Ollama search cannot reliably apply the news recency filter".to_string())
            } else {
                ollama::search_ollama(query, count, domain, recency).await
            }
        }
        "tavily" => tavily::search_tavily(query, count, domain, recency, news).await,
        "searxng" => {
            let mut params = HashMap::new();
            let q = searxng::searxng_query_rewrite(query);
            let full_q = if let Some(d) = domain.filter(|s| !s.trim().is_empty()) {
                format!("{} site:{}", q, d.trim())
            } else {
                q
            };
            params.insert("q".to_string(), full_q);
            params.insert("format".to_string(), "json".to_string());
            params.insert("language".to_string(), "en".to_string());
            if news {
                params.insert("categories".to_string(), "news".to_string());
            }
            if let Some(rec) = recency {
                params.insert("time_range".to_string(), rec.to_string());
            }
            searxng::searxng_search(&params).await
        }
        _ => Err(format!("Unknown provider: {}", name)),
    }
}

async fn attempt_single(
    name: &'static str,
    query: &str,
    count: usize,
    domain: Option<&str>,
    recency: Option<&str>,
    news: bool,
) -> Result<Value, String> {
    let payload = if name == "brave" || name == "exa" {
        let health_name = format!("search:{}", name);
        if !health_available(&health_name) {
            return Err(format!("{} search is temporarily cooling down", name));
        }
        attempt_provider(&health_name, || execute_single_provider(name, query, count, domain, recency, news)).await?
    } else {
        execute_single_provider(name, query, count, domain, recency, news).await?
    };

    Ok(normalize_results(name, query, payload, count))
}

pub async fn do_search(
    provider: &str,
    query: &str,
    count: usize,
    domain: Option<&str>,
    recency: Option<&str>,
    news: bool,
) -> Result<Value, String> {
    if provider == "auto" {
        debug!("Auto search: query={:?}", &query[..query.len().min(80)]);
        let mut errors = HashMap::new();
        let mut empty_providers = Vec::new();

        if ollama_api_key().is_some() {
            match attempt_single("ollama", query, count, domain, recency, news).await {
                Ok(res) => {
                    let has_results = res
                        .get("results")
                        .and_then(|v| v.as_array())
                        .map(|a| !a.is_empty())
                        .unwrap_or(false);
                    if has_results {
                        return Ok(res);
                    }
                    empty_providers.push("ollama");
                }
                Err(e) => {
                    errors.insert("Ollama", e);
                }
            }
        } else {
            errors.insert("Ollama", "API key not configured".to_string());
        }

        if brave_api_key().is_some() {
            match attempt_single("brave", query, count, domain, recency, news).await {
                Ok(res) => {
                    let has_results = res
                        .get("results")
                        .and_then(|v| v.as_array())
                        .map(|a| !a.is_empty())
                        .unwrap_or(false);
                    if has_results {
                        return Ok(res);
                    }
                    empty_providers.push("brave");
                }
                Err(e) => {
                    errors.insert("Brave", e);
                }
            }
        }

        if tavily_api_key().is_some() {
            match attempt_single("tavily", query, count, domain, recency, news).await {
                Ok(res) => {
                    let has_results = res
                        .get("results")
                        .and_then(|v| v.as_array())
                        .map(|a| !a.is_empty())
                        .unwrap_or(false);
                    if has_results {
                        return Ok(res);
                    }
                    empty_providers.push("tavily");
                }
                Err(e) => {
                    errors.insert("Tavily", e);
                }
            }
        } else {
            errors.insert("Tavily", "API key not configured".to_string());
        }

        if exa_api_key().is_some() {
            match attempt_single("exa", query, count, domain, recency, news).await {
                Ok(res) => {
                    let has_results = res
                        .get("results")
                        .and_then(|v| v.as_array())
                        .map(|a| !a.is_empty())
                        .unwrap_or(false);
                    if has_results {
                        return Ok(res);
                    }
                    empty_providers.push("exa");
                }
                Err(e) => {
                    errors.insert("Exa", e);
                }
            }
        } else {
            errors.insert("Exa", "API key not configured".to_string());
        }

        match attempt_single("searxng", query, count, domain, recency, news).await {
            Ok(res) => {
                let has_results = res
                    .get("results")
                    .and_then(|v| v.as_array())
                    .map(|a| !a.is_empty())
                    .unwrap_or(false);
                if has_results {
                    return Ok(res);
                }
                empty_providers.push("searxng");
            }
            Err(e) => {
                errors.insert("SearXNG", e);
            }
        }

        if !empty_providers.is_empty() {
            return Ok(json!({
                "provider": "none",
                "query": query,
                "results": [],
                "status": "no_results",
                "providers_checked": empty_providers
            }));
        }

        let mut lines = vec!["All search providers failed.".to_string()];
        for (k, v) in errors {
            lines.push(format!("{}: {}", k, v));
        }
        return Err(lines.join("\n"));
    }

    match provider {
        "brave" => attempt_single("brave", query, count, domain, recency, news).await,
        "exa" => attempt_single("exa", query, count, domain, recency, news).await,
        "ollama" => attempt_single("ollama", query, count, domain, recency, news).await,
        "tavily" => attempt_single("tavily", query, count, domain, recency, news).await,
        "searxng" => attempt_single("searxng", query, count, domain, recency, news).await,
        _ => Err(format!("Unknown provider: {}", provider)),
    }
}
