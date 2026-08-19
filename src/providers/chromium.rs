use std::collections::HashSet;

use scraper::{Html, Selector};
use serde_json::{json, Value};
use url::Url;

use crate::browser::render_html;
use crate::net::{normalize_url_key, validate_url};

const SEARCH_URL: &str = "https://html.duckduckgo.com/html/";

fn clean_text(text: impl Iterator<Item = String>) -> String {
    text.collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn result_url(href: &str) -> Option<String> {
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        Url::parse(SEARCH_URL).ok()?.join(href).ok()?.to_string()
    };
    let parsed = Url::parse(&absolute).ok()?;

    if parsed
        .host_str()
        .is_some_and(|host| host == "duckduckgo.com" || host.ends_with(".duckduckgo.com"))
    {
        if let Some((_, target)) = parsed.query_pairs().find(|(key, _)| key == "uddg") {
            return validate_url(&target, false).ok();
        }
    }

    validate_url(&absolute, false).ok()
}

pub fn parse_search_results(html: &str, count: usize) -> Result<Vec<Value>, String> {
    let lowercase = html.to_lowercase();
    if lowercase.contains("unusual traffic")
        || lowercase.contains("verify you are a human")
        || lowercase.contains("captcha")
    {
        return Err("Chromium search was blocked by an anti-bot challenge".to_string());
    }

    let document = Html::parse_document(html);
    let result_selector = Selector::parse(".result").map_err(|_| "Invalid search selector".to_string())?;
    let title_selector = Selector::parse("a.result__a").map_err(|_| "Invalid search selector".to_string())?;
    let snippet_selector = Selector::parse(".result__snippet").map_err(|_| "Invalid search selector".to_string())?;
    let mut seen = HashSet::new();
    let mut results = Vec::new();

    for result in document.select(&result_selector) {
        let Some(anchor) = result.select(&title_selector).next() else {
            continue;
        };
        let Some(href) = anchor.value().attr("href") else {
            continue;
        };
        let Some(url) = result_url(href) else {
            continue;
        };
        let key = normalize_url_key(&url);
        if !seen.insert(key) {
            continue;
        }

        let title = clean_text(anchor.text().map(str::to_string));
        if title.is_empty() {
            continue;
        }
        let snippet = result
            .select(&snippet_selector)
            .next()
            .map(|element| clean_text(element.text().map(str::to_string)))
            .unwrap_or_default();
        let source = Url::parse(&url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(str::to_string))
            .unwrap_or_default();

        results.push(json!({
            "title": title,
            "url": url,
            "description": snippet,
            "source": source
        }));
        if results.len() >= count {
            break;
        }
    }

    Ok(results)
}

pub async fn search_chromium(
    query: &str,
    count: usize,
    domain: Option<&str>,
    recency: Option<&str>,
    news: bool,
) -> Result<Value, String> {
    let mut search_query = query.to_string();
    if let Some(domain) = domain.filter(|value| !value.trim().is_empty()) {
        search_query.push_str(" site:");
        search_query.push_str(domain.trim());
    }
    if news {
        search_query.push_str(" news");
    }

    let mut url = Url::parse(SEARCH_URL).map_err(|_| "Invalid Chromium search URL".to_string())?;
    {
        let mut params = url.query_pairs_mut();
        params.append_pair("q", &search_query);
        if let Some(filter) = match recency {
            Some("day") => Some("d"),
            Some("week") => Some("w"),
            Some("month") => Some("m"),
            Some("year") => Some("y"),
            _ => None,
        } {
            params.append_pair("df", filter);
        }
    }

    let html = render_html(url.as_str()).await?;
    let results = parse_search_results(&html, count)?;
    Ok(json!({ "results": results }))
}

#[cfg(test)]
mod tests {
    use super::parse_search_results;

    #[test]
    fn parses_and_decodes_duckduckgo_results() {
        let html = r#"
            <div class="result">
              <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Ffacts"> Example Facts </a>
              <div class="result__snippet">Useful <b>facts</b> here.</div>
            </div>
        "#;
        let results = parse_search_results(html, 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["url"], "https://example.com/facts");
        assert_eq!(results[0]["title"], "Example Facts");
        assert_eq!(results[0]["description"], "Useful facts here.");
    }

    #[test]
    fn rejects_challenge_pages() {
        assert!(parse_search_results("Verify you are a human with CAPTCHA", 5).is_err());
    }
}
