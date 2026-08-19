use std::collections::HashSet;
use std::sync::LazyLock;
use regex::Regex;
use scraper::{Html, Node, Selector};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::net::normalize_url_key;
use crate::tools::torrent::parse_magnet;

static NAV_JUNK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(home|menu|search|about|contact|login|log in|sign in|register|cart|share|follow us|more|back to top|skip to content|privacy policy|terms of service|terms of use|read more|continue reading)$",
    )
    .unwrap()
});

static COOKIE_JUNK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(accept|cookie|privacy|subscribe|newsletter|sign ?up|agree|consent|manage .*preferences|this website uses|by continuing you)",
    )
    .unwrap()
});

pub fn clean_blocks(blocks: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for b in blocks {
        let text = b.trim();
        if text.is_empty() {
            continue;
        }
        if text.len() < 60 && NAV_JUNK.is_match(text) {
            continue;
        }
        if text.len() < 140 && COOKIE_JUNK.is_match(text) {
            continue;
        }
        let key = text.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        out.push(text.to_string());
    }
    out
}

pub fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.is_empty() || text.len() <= max_chars {
        return text.to_string();
    }

    // Safely find char boundary <= max_chars
    let mut end_idx = max_chars;
    while !text.is_char_boundary(end_idx) && end_idx > 0 {
        end_idx -= 1;
    }
    let cut = &text[..end_idx];

    let min_boundary = (max_chars as f64 * 0.5) as usize;
    let mut boundary = None;
    for sep in &["\n\n", ".\n", "\n", ". ", "! ", "? ", ".\""] {
        if let Some(idx) = cut.rfind(sep) {
            if idx >= min_boundary {
                boundary = Some(idx + sep.len());
                break;
            }
        }
    }

    let final_cut = if let Some(b) = boundary {
        &cut[..b]
    } else {
        cut
    };

    format!("{} [content truncated]", final_cut.trim_end())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtractedLink {
    pub text: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedPage {
    pub title: String,
    pub description: String,
    pub content: String,
    pub links: Vec<ExtractedLink>,
    pub final_url: String,
    pub provider: String,
}

const SKIP_TAGS: &[&str] = &[
    "script", "style", "noscript", "template", "nav", "header", "footer",
];
const BREAK_TAGS: &[&str] = &[
    "p", "div", "br", "li", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "blockquote",
    "section", "article", "table", "pre", "hr",
];
const MAX_DOM_DEPTH: usize = 512;

fn normalize_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_whitespace = true;
    for c in s.chars() {
        if c.is_whitespace() {
            if !in_whitespace {
                result.push(' ');
                in_whitespace = true;
            }
        } else {
            result.push(c);
            in_whitespace = false;
        }
    }
    if in_whitespace && !result.is_empty() {
        result.pop();
    }
    result
}

struct PageVisitor {
    title: String,
    description: String,
    raw_links: Vec<ExtractedLink>,
    blocks: Vec<String>,
    current_text: String,
    current_link: Option<(String, String)>, // (href, text)
    skip_depth: usize,
    in_title: bool,
}

impl PageVisitor {
    fn new() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            raw_links: Vec::new(),
            blocks: Vec::new(),
            current_text: String::new(),
            current_link: None,
            skip_depth: 0,
            in_title: false,
        }
    }

    fn flush(&mut self) {
        if !self.current_text.is_empty() {
            let cleaned = normalize_whitespace(&self.current_text);
            if !cleaned.is_empty() {
                self.blocks.push(cleaned);
            }
            self.current_text.clear();
        }
    }

    fn finish_link(&mut self) {
        if let Some((href, text_raw)) = self.current_link.take() {
            let text = normalize_whitespace(&text_raw);
            self.raw_links.push(ExtractedLink { text, url: href });
        }
    }

    fn walk_node(&mut self, node_ref: scraper::ElementRef, depth: usize) {
        if depth >= MAX_DOM_DEPTH {
            return;
        }

        for child in node_ref.children() {
            match child.value() {
                Node::Element(el) => {
                    let tag_name = el.name().to_lowercase();
                    if SKIP_TAGS.contains(&tag_name.as_str()) {
                        self.skip_depth += 1;
                        if let Some(sub_elem) = scraper::ElementRef::wrap(child) {
                            self.walk_node(sub_elem, depth + 1);
                        }
                        self.skip_depth = self.skip_depth.saturating_sub(1);
                        continue;
                    }

                    if tag_name == "title" {
                        self.in_title = true;
                        if let Some(sub_elem) = scraper::ElementRef::wrap(child) {
                            self.walk_node(sub_elem, depth + 1);
                        }
                        self.in_title = false;
                        continue;
                    }

                    if tag_name == "meta" {
                        let name = el.attr("name").or_else(|| el.attr("property")).unwrap_or("").to_lowercase();
                        if (name == "description" || name == "og:description") && self.description.is_empty() {
                            if let Some(content) = el.attr("content") {
                                self.description = content.trim().to_string();
                            }
                        }
                    }

                    let is_break = BREAK_TAGS.contains(&tag_name.as_str());
                    if is_break {
                        self.flush();
                    }

                    let is_link = tag_name == "a";
                    if is_link {
                        if let Some(href) = el.attr("href") {
                            self.current_link = Some((href.to_string(), String::new()));
                        }
                    }

                    if let Some(sub_elem) = scraper::ElementRef::wrap(child) {
                        self.walk_node(sub_elem, depth + 1);
                    }

                    if is_link {
                        self.finish_link();
                    }
                    if is_break {
                        self.flush();
                    }
                }
                Node::Text(txt) => {
                    if self.skip_depth > 0 {
                        continue;
                    }
                    let data = &txt.text;
                    if self.in_title {
                        self.title.push_str(data);
                    } else if let Some((_, text_acc)) = &mut self.current_link {
                        text_acc.push_str(data);
                        self.current_text.push_str(data);
                    } else {
                        self.current_text.push_str(data);
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn resolve_links(raw_links: &[ExtractedLink], base_url: &str) -> Vec<ExtractedLink> {
    let mut seen = HashSet::new();
    let mut links = Vec::new();
    let parsed_base = Url::parse(base_url).ok();

    for link in raw_links {
        let href = link.url.trim();
        let text = link.text.trim().to_string();
        if href.is_empty() {
            continue;
        }
        let lower = href.to_lowercase();
        if lower.starts_with("javascript:")
            || lower.starts_with("mailto:")
            || lower.starts_with("tel:")
            || lower.starts_with("data:")
            || lower.starts_with("ftp:")
        {
            continue;
        }

        if lower.starts_with("magnet:") {
            if let Ok(magnet) = parse_magnet(href) {
                let canonical = format!("magnet:{}", magnet.info_hash);
                if seen.contains(&canonical) {
                    continue;
                }
                seen.insert(canonical);
                links.push(ExtractedLink {
                    text,
                    url: magnet.url,
                });
                if links.len() >= 50 {
                    break;
                }
            }
            continue;
        }

        let resolved = if let Some(base) = &parsed_base {
            match base.join(href) {
                Ok(u) => u.to_string(),
                Err(_) => continue,
            }
        } else {
            href.to_string()
        };

        if !resolved.starts_with("http://") && !resolved.starts_with("https://") {
            continue;
        }

        let canonical = normalize_url_key(&resolved);
        if seen.contains(&canonical) {
            continue;
        }
        seen.insert(canonical);

        links.push(ExtractedLink {
            text,
            url: resolved,
        });

        if links.len() >= 50 {
            break;
        }
    }

    links
}

pub fn parse_html(html_str: &str, base_url: &str, max_chars: usize) -> ParsedPage {
    let document = Html::parse_document(html_str);
    let mut visitor = PageVisitor::new();

    // Fallback selectors for title / meta if body traversal misses them
    let title_selector = Selector::parse("title").ok();
    let meta_desc_selector = Selector::parse("meta[name='description'], meta[property='og:description']").ok();

    visitor.walk_node(document.root_element(), 0);
    visitor.flush();

    let mut title = normalize_whitespace(&visitor.title);
    if title.is_empty() {
        if let Some(sel) = &title_selector {
            if let Some(el) = document.select(sel).next() {
                title = normalize_whitespace(&el.text().collect::<Vec<_>>().join(" "));
            }
        }
    }

    let mut description = visitor.description;
    if description.is_empty() {
        if let Some(sel) = &meta_desc_selector {
            for el in document.select(sel) {
                if let Some(c) = el.value().attr("content") {
                    description = c.trim().to_string();
                    break;
                }
            }
        }
    }

    let cleaned_blocks = clean_blocks(&visitor.blocks);
    let joined_content = cleaned_blocks.join("\n\n");
    let content = truncate_text(&joined_content, max_chars);
    let links = resolve_links(&visitor.raw_links, base_url);

    ParsedPage {
        title,
        description,
        content,
        links,
        final_url: base_url.to_string(),
        provider: "direct".to_string(),
    }
}
