use data_encoding::{BASE32, HEXLOWER};
use regex::Regex;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use url::form_urlencoded;
use url::Url;

use crate::budget::enforce_output_budget;
use crate::config::{
    torrent_site_urls, torznab_urls, MAX_TORRENT_BYTES, MAX_URL_CHARS, NETWORK_TIMEOUT,
    TRUSTED_TORRENT_DOMAINS,
};
use crate::guard::check_search_guard;
use crate::net::{fetch_bytes, fetch_html, get_json, validate_url};
use crate::providers::do_search;

pub static TORRENT_INTENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:\b(?:bit(?:\s|-)?torrent|torrent|magnet\s+link|info\s*hash|btih)\b|\.torrent(?:\b|$)|magnet:\?xt=)",
    )
    .unwrap()
});

static MAGNET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)magnet:\?[^\s<>"']+"#).unwrap());

static SIZE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(\d+(?:\.\d+)?)\s*(KiB|MiB|GiB|TiB|KB|MB|GB|TB)\b").unwrap()
});

static SEED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:seeders?|seeds?)\s*[:=]?\s*(\d[\d,]*)").unwrap());

static LEECH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:leechers?|leeches|peers?)\s*[:=]?\s*(\d[\d,]*)").unwrap()
});

pub fn is_torrent_query(query: &str) -> bool {
    TORRENT_INTENT_RE.is_match(query)
}

#[derive(Clone, Debug)]
pub struct MagnetInfo {
    pub url: String,
    pub info_hash: String,
    pub name: String,
    pub trackers: Vec<String>,
}

pub fn parse_magnet(raw: &str) -> Result<MagnetInfo, String> {
    let link = raw.trim().trim_end_matches(['.', ',', ';', ')']);
    if link.len() > MAX_URL_CHARS {
        return Err("magnet link is too long".to_string());
    }

    let parsed = Url::parse(link).map_err(|_| "link must use the magnet scheme".to_string())?;
    if parsed.scheme().to_lowercase() != "magnet" {
        return Err("link must use the magnet scheme".to_string());
    }

    let mut exact_topics = Vec::new();
    let mut display_name = String::new();
    let mut trackers = Vec::new();

    for (k, v) in parsed.query_pairs() {
        if k == "xt" {
            exact_topics.push(v.to_string());
        } else if k == "dn" && display_name.is_empty() {
            display_name = v.to_string();
        } else if k == "tr" {
            trackers.push(v.to_string());
        }
    }

    let btih = exact_topics
        .iter()
        .find(|s| s.to_lowercase().starts_with("urn:btih:"))
        .map(|s| &s[9..])
        .ok_or_else(|| "magnet link must contain a 40-hex or 32-base32 BTIH hash".to_string())?;

    let info_hash = if btih.len() == 40 && btih.chars().all(|c| c.is_ascii_hexdigit()) {
        btih.to_lowercase()
    } else if btih.len() == 32 {
        let bytes = BASE32
            .decode(btih.to_uppercase().as_bytes())
            .map_err(|_| "magnet link has an invalid BTIH hash".to_string())?;
        HEXLOWER.encode(&bytes)
    } else {
        return Err("magnet link must contain a 40-hex or 32-base32 BTIH hash".to_string());
    };

    Ok(MagnetInfo {
        url: link.to_string(),
        info_hash,
        name: display_name,
        trackers,
    })
}

// ---------------- Bencode parser ----------------

#[derive(Debug, PartialEq, Clone)]
pub enum BencodeValue<'a> {
    Int(i64),
    Bytes(&'a [u8]),
    List(Vec<BencodeValue<'a>>),
    Dict(Vec<(&'a [u8], BencodeValue<'a>)>),
}

fn bdecode_internal<'a>(
    data: &'a [u8],
    mut index: usize,
    depth: usize,
) -> Result<(BencodeValue<'a>, usize), String> {
    if depth > 64 || index >= data.len() {
        return Err("invalid torrent metainfo".to_string());
    }

    let marker = data[index];
    if marker == b'i' {
        index += 1;
        let start = index;
        while index < data.len() && data[index] != b'e' {
            index += 1;
        }
        if index >= data.len() {
            return Err("invalid torrent integer".to_string());
        }
        let int_str = std::str::from_utf8(&data[start..index])
            .map_err(|_| "invalid torrent integer".to_string())?;
        let num: i64 = int_str
            .parse()
            .map_err(|_| "invalid torrent integer".to_string())?;
        return Ok((BencodeValue::Int(num), index + 1));
    }

    if marker == b'l' {
        index += 1;
        let mut list = Vec::new();
        while index < data.len() && data[index] != b'e' {
            let (val, next_idx) = bdecode_internal(data, index, depth + 1)?;
            list.push(val);
            index = next_idx;
        }
        if index >= data.len() {
            return Err("invalid torrent list".to_string());
        }
        return Ok((BencodeValue::List(list), index + 1));
    }

    if marker == b'd' {
        index += 1;
        let mut dict = Vec::new();
        while index < data.len() && data[index] != b'e' {
            let (key_val, next_idx) = bdecode_internal(data, index, depth + 1)?;
            let key_bytes = match key_val {
                BencodeValue::Bytes(b) => b,
                _ => return Err("invalid torrent dictionary key".to_string()),
            };
            let (val, val_idx) = bdecode_internal(data, next_idx, depth + 1)?;
            dict.push((key_bytes, val));
            index = val_idx;
        }
        if index >= data.len() {
            return Err("invalid torrent dictionary".to_string());
        }
        return Ok((BencodeValue::Dict(dict), index + 1));
    }

    // Byte string: <len>:<bytes>
    let mut colon_idx = index;
    while colon_idx < data.len() && colon_idx < index + 24 && data[colon_idx] != b':' {
        colon_idx += 1;
    }
    if colon_idx >= data.len() || data[colon_idx] != b':' {
        return Err("invalid torrent string".to_string());
    }
    let len_str = std::str::from_utf8(&data[index..colon_idx])
        .map_err(|_| "invalid torrent string length".to_string())?;
    let len: usize = len_str
        .parse()
        .map_err(|_| "invalid torrent string length".to_string())?;

    let str_start = colon_idx + 1;
    let str_end = str_start
        .checked_add(len)
        .ok_or_else(|| "invalid torrent string length".to_string())?;
    if str_end > data.len() {
        return Err("invalid torrent string length".to_string());
    }

    Ok((BencodeValue::Bytes(&data[str_start..str_end]), str_end))
}

#[derive(Clone, Debug)]
pub struct TorrentInfo {
    pub info_hash: String,
    pub name: String,
    pub size_bytes: u64,
}

pub fn parse_torrent(data: &[u8]) -> Result<TorrentInfo, String> {
    if data.is_empty() || data.len() > MAX_TORRENT_BYTES {
        return Err("torrent file is empty or too large".to_string());
    }
    if data[0] != b'd' {
        return Err("torrent metainfo must be a bencoded dictionary".to_string());
    }

    let mut index = 1;
    let mut info_span: Option<(usize, usize)> = None;
    let mut info_val: Option<BencodeValue> = None;

    while index < data.len() && data[index] != b'e' {
        let (key_val, next_idx) = bdecode_internal(data, index, 1)?;
        let key_bytes = match key_val {
            BencodeValue::Bytes(b) => b,
            _ => return Err("invalid torrent dictionary key".to_string()),
        };
        let val_start = next_idx;
        let (val, val_end) = bdecode_internal(data, next_idx, 1)?;
        if key_bytes == b"info" {
            info_span = Some((val_start, val_end));
            info_val = Some(val);
        }
        index = val_end;
    }

    let (start, end) =
        info_span.ok_or_else(|| "torrent metainfo has no valid info dictionary".to_string())?;
    let mut hasher = Sha1::new();
    hasher.update(&data[start..end]);
    let info_hash = HEXLOWER.encode(&hasher.finalize());

    let mut name = String::new();
    let mut size_bytes = 0u64;

    if let Some(BencodeValue::Dict(dict)) = info_val {
        for (k, v) in dict {
            if (k == b"name.utf-8" || k == b"name") && name.is_empty() {
                if let BencodeValue::Bytes(b) = v {
                    name = String::from_utf8_lossy(b).to_string();
                }
            } else if k == b"length" {
                if let BencodeValue::Int(n) = v {
                    if n > 0 {
                        size_bytes = n as u64;
                    }
                }
            } else if k == b"files" {
                if let BencodeValue::List(files) = v {
                    let mut total = 0u64;
                    for f in files {
                        if let BencodeValue::Dict(fdict) = f {
                            for (fk, fv) in fdict {
                                if fk == b"length" {
                                    if let BencodeValue::Int(flen) = fv {
                                        if flen > 0 {
                                            total = total.checked_add(flen as u64).ok_or_else(
                                                || "invalid torrent size".to_string(),
                                            )?;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if size_bytes == 0 {
                        size_bytes = total;
                    }
                }
            }
        }
    }

    Ok(TorrentInfo {
        info_hash,
        name,
        size_bytes,
    })
}

pub fn source_domain(url_str: &str) -> String {
    if let Ok(parsed) = Url::parse(url_str) {
        let host = parsed.host_str().unwrap_or("").to_lowercase();
        if let Some(stripped) = host.strip_prefix("www.") {
            stripped.to_string()
        } else {
            host
        }
    } else {
        String::new()
    }
}

pub fn is_trusted_torrent_source(url_str: &str) -> bool {
    let domain = source_domain(url_str);
    if domain.is_empty() {
        return false;
    }
    TRUSTED_TORRENT_DOMAINS
        .iter()
        .any(|&d| domain == d || domain.ends_with(&format!(".{}", d)))
}

fn number_match(re: &Regex, text: &str) -> Option<i64> {
    re.captures(text)
        .and_then(|cap| cap.get(1))
        .and_then(|m| m.as_str().replace(',', "").parse().ok())
}

fn size_match(text: &str) -> Option<String> {
    SIZE_RE.captures(text).map(|cap| {
        format!(
            "{} {}",
            cap.get(1).map(|m| m.as_str()).unwrap_or(""),
            cap.get(2).map(|m| m.as_str()).unwrap_or("")
        )
    })
}

pub fn candidate_from_link(
    link: &str,
    title: &str,
    source_url: &str,
    text: &str,
    extra: Option<HashMap<String, Value>>,
) -> Option<Value> {
    let lower = link.trim().to_lowercase();
    let (c_type, c_url, c_hash, c_name) = if lower.starts_with("magnet:") {
        let mag = parse_magnet(link).ok()?;
        ("magnet", mag.url, Some(mag.info_hash), mag.name)
    } else {
        let valid_url = validate_url(link, false).ok()?;
        let basename = Url::parse(&valid_url)
            .ok()
            .and_then(|u| {
                u.path_segments()
                    .and_then(|s| s.last().map(|seg| seg.to_string()))
            })
            .unwrap_or_default();
        ("torrent", valid_url, None, basename)
    };

    let name = if !c_name.is_empty() {
        c_name
    } else if !title.trim().is_empty() {
        title.trim().to_string()
    } else {
        "torrent".to_string()
    };

    let s_domain = source_domain(source_url);
    let src = if !s_domain.is_empty() {
        s_domain
    } else {
        source_url.to_string()
    };
    let trusted = is_trusted_torrent_source(source_url);

    let mut map = serde_json::Map::new();
    map.insert("name".to_string(), json!(name));
    map.insert("url".to_string(), json!(c_url));
    map.insert("type".to_string(), json!(c_type));
    map.insert("source".to_string(), json!(src));
    map.insert("source_url".to_string(), json!(source_url));
    map.insert("trusted".to_string(), json!(trusted));
    map.insert("verified".to_string(), json!(trusted));
    map.insert(
        "validation".to_string(),
        json!(if c_type == "magnet" {
            "magnet_syntax"
        } else {
            "not_checked"
        }),
    );

    if let Some(h) = c_hash {
        map.insert("info_hash".to_string(), json!(h));
    }

    if let Some(sz) = size_match(text) {
        map.insert("size".to_string(), json!(sz));
    }
    if let Some(seeders) = number_match(&SEED_RE, text) {
        map.insert("seeders".to_string(), json!(seeders));
    }
    if let Some(leechers) = number_match(&LEECH_RE, text) {
        map.insert("leechers".to_string(), json!(leechers));
    }

    if let Some(ext) = extra {
        for (k, v) in ext {
            if !v.is_null() {
                map.insert(k, v);
            }
        }
    }

    Some(Value::Object(map))
}

pub fn torrent_links_in_text(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    for cap in MAGNET_RE.find_iter(text) {
        links.push(
            cap.as_str()
                .trim_end_matches(['.', ',', ';', ')'])
                .to_string(),
        );
    }

    static HTTP_TORRENT_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?i)https?://[^\s<>"']+?\.torrent(?:\?[^\s<>"']*)?"#).unwrap()
    });

    for cap in HTTP_TORRENT_RE.find_iter(text) {
        links.push(
            cap.as_str()
                .trim_end_matches(['.', ',', ';', ')'])
                .to_string(),
        );
    }

    links.dedup();
    links
}

#[allow(dead_code)]
struct TorrentSiteDef {
    name: &'static str,
    base: &'static str,
    search: &'static str,
    slug: bool,
}

const TORRENT_SITE_DEFS: &[TorrentSiteDef] = &[
    TorrentSiteDef {
        name: "1337x",
        base: "https://www.1337x.tw",
        search: "https://www.1337x.tw/search/{query}/1/",
        slug: false,
    },
    TorrentSiteDef {
        name: "YTS",
        base: "https://yts.proxyninja.org",
        search: "https://yts.proxyninja.org/browse-movies/{query}/all/all/0/latest/latest/0/",
        slug: true,
    },
    TorrentSiteDef {
        name: "ThePirateBay",
        base: "https://thepiratebay.org",
        search: "https://thepiratebay.org/search/{query}/1/99/0",
        slug: true,
    },
    TorrentSiteDef {
        name: "EZTV",
        base: "https://eztvx.to",
        search: "https://eztvx.to/search/{query}",
        slug: true,
    },
    TorrentSiteDef {
        name: "LimeTorrents",
        base: "https://www.limetorrents.lol",
        search: "https://www.limetorrents.lol/search/all/{query}",
        slug: true,
    },
    TorrentSiteDef {
        name: "Nyaa",
        base: "https://nyaa.si",
        search: "https://nyaa.si/?f=0&c=0_0&q={query}",
        slug: false,
    },
];

async fn torznab_search(query: &str, count: usize) -> Vec<Value> {
    let endpoints = torznab_urls();
    let mut results = Vec::new();

    for endpoint in endpoints {
        let sep = if endpoint.contains('?') { "&" } else { "?" };
        let encoded_q = form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
        let url = format!(
            "{}{}t=search&q={}&limit={}",
            endpoint, sep, encoded_q, count
        );

        let mut headers = HashMap::new();
        headers.insert("Accept".to_string(), "application/xml".to_string());

        if let Ok(bytes) = fetch_bytes(&url, 512_000, Some(NETWORK_TIMEOUT), Some(headers)).await {
            let xml_str = String::from_utf8_lossy(&bytes);
            // Parse XML items
            let item_re = Regex::new(r"(?s)<item>(.*?)</item>").unwrap();
            let title_re = Regex::new(r"<title>(.*?)</title>").unwrap();
            let link_re = Regex::new(r"<link>(.*?)</link>").unwrap();
            let enclosure_re = Regex::new(r#"<enclosure\s+url="([^"]+)""#).unwrap();
            let attr_re =
                Regex::new(r#"<torznab:attr\s+name="([^"]+)"\s+value="([^"]+)""#).unwrap();

            for cap in item_re.captures_iter(&xml_str) {
                let item_xml = &cap[1];
                let title = title_re
                    .captures(item_xml)
                    .map(|c| c[1].to_string())
                    .unwrap_or_default();
                let enclosure = enclosure_re.captures(item_xml).map(|c| c[1].to_string());
                let link_elem = link_re.captures(item_xml).map(|c| c[1].to_string());

                let mut attrs = HashMap::new();
                for attr_cap in attr_re.captures_iter(item_xml) {
                    attrs.insert(attr_cap[1].to_string(), attr_cap[2].to_string());
                }

                let link = attrs
                    .get("magneturl")
                    .cloned()
                    .or(enclosure)
                    .or(link_elem)
                    .unwrap_or_default();

                if link.is_empty() {
                    continue;
                }

                let size_b = attrs.get("size").and_then(|s| s.parse::<u64>().ok());
                let seeders = attrs.get("seeders").and_then(|s| s.parse::<i64>().ok());
                let peers = attrs.get("peers").and_then(|s| s.parse::<i64>().ok());
                let leechers = match (peers, seeders) {
                    (Some(p), Some(s)) => Some((p - s).max(0)),
                    _ => None,
                };

                let mut extra = HashMap::new();
                if let Some(sz) = size_b {
                    extra.insert("size_bytes".to_string(), json!(sz));
                }
                if let Some(s) = seeders {
                    extra.insert("seeders".to_string(), json!(s));
                }
                if let Some(l) = leechers {
                    extra.insert("leechers".to_string(), json!(l));
                }

                if let Some(mut cand) =
                    candidate_from_link(&link, &title, &endpoint, &title, Some(extra))
                {
                    if let Value::Object(ref mut map) = cand {
                        let s_domain = source_domain(&endpoint);
                        map.insert(
                            "source".to_string(),
                            json!(if !s_domain.is_empty() {
                                s_domain
                            } else {
                                "Torznab".to_string()
                            }),
                        );
                        map.insert(
                            "verified".to_string(),
                            json!(attrs
                                .get("downloadvolumefactor")
                                .map(|v| v == "0")
                                .unwrap_or(false)),
                        );
                    }
                    results.push(cand);
                }
            }
        }
    }

    results.truncate(count);
    results
}

async fn internet_archive_torrents(query: &str, count: usize) -> Vec<Value> {
    let query_encoded = {
        let mut params = form_urlencoded::Serializer::new(String::new());
        params.append_pair(
            "q",
            &format!(
                "({}) AND mediatype:(software OR texts OR audio OR movies)",
                query
            ),
        );
        params.append_pair("fl[]", "identifier");
        params.append_pair("fl[]", "title");
        params.append_pair("fl[]", "item_size");
        params.append_pair("rows", &count.to_string());
        params.append_pair("page", "1");
        params.append_pair("output", "json");
        params.finish()
    };

    let url = format!("https://archive.org/advancedsearch.php?{}", query_encoded);
    let mut results = Vec::new();

    if let Ok(data) = get_json(&url, Some(NETWORK_TIMEOUT), None, None).await {
        if let Some(docs) = data
            .get("response")
            .and_then(|r| r.get("docs"))
            .and_then(|d| d.as_array())
        {
            for doc in docs {
                if let Some(id) = doc.get("identifier").and_then(|v| v.as_str()) {
                    let title = doc.get("title").and_then(|v| v.as_str()).unwrap_or(id);
                    let item_size = doc.get("item_size").and_then(|v| v.as_u64());

                    let encoded_id =
                        form_urlencoded::byte_serialize(id.as_bytes()).collect::<String>();
                    let item_url = format!("https://archive.org/details/{}", encoded_id);
                    let torrent_url = format!(
                        "https://archive.org/download/{}/{}_archive.torrent",
                        encoded_id, encoded_id
                    );

                    let mut extra = HashMap::new();
                    if let Some(sz) = item_size {
                        extra.insert("size_bytes".to_string(), json!(sz));
                    }

                    if let Some(mut cand) =
                        candidate_from_link(&torrent_url, title, &item_url, "", Some(extra))
                    {
                        if let Value::Object(ref mut map) = cand {
                            map.insert("trusted".to_string(), json!(true));
                            map.insert("verified".to_string(), json!(false));
                            map.insert(
                                "trust_note".to_string(),
                                json!("Trusted host; uploader/content not independently verified"),
                            );
                        }
                        results.push(cand);
                    }
                }
            }
        }
    }

    results
}

async fn torrent_site_search(query: &str, count: usize) -> Vec<Value> {
    let mut results = Vec::new();
    let query_slug = query
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    let extra_templates = torrent_site_urls();

    for def in TORRENT_SITE_DEFS {
        let rendered = if def.slug {
            &query_slug
        } else {
            &form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>()
        };
        let search_url = def.search.replace("{query}", rendered);

        if let Ok(page_resp) = fetch_html(&search_url, 40_000, Some(NETWORK_TIMEOUT)).await {
            let parsed = crate::html::parse_html(&page_resp.raw_html, &page_resp.final_url, 40_000);
            let page_text = format!("{} {} {}", parsed.title, parsed.description, parsed.content);

            for link in &parsed.links {
                let l_url = &link.url;
                if l_url.to_lowercase().starts_with("magnet:")
                    || l_url.split('?').next().unwrap_or("").ends_with(".torrent")
                {
                    if let Some(cand) =
                        candidate_from_link(l_url, &link.text, &search_url, &page_text, None)
                    {
                        results.push(cand);
                    }
                }
            }

            for t_link in torrent_links_in_text(&page_text) {
                if t_link.to_lowercase().starts_with("magnet:") {
                    if let Some(cand) =
                        candidate_from_link(&t_link, &parsed.title, &search_url, &page_text, None)
                    {
                        results.push(cand);
                    }
                }
            }
        }

        if results.len() >= count * 2 {
            break;
        }
    }

    for template in extra_templates {
        let encoded_q = form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
        let search_url = template.replace("{query}", &encoded_q);

        if let Ok(page_resp) = fetch_html(&search_url, 40_000, Some(NETWORK_TIMEOUT)).await {
            let parsed = crate::html::parse_html(&page_resp.raw_html, &page_resp.final_url, 40_000);
            let page_text = format!("{} {} {}", parsed.title, parsed.description, parsed.content);

            for link in &parsed.links {
                let l_url = &link.url;
                if l_url.to_lowercase().starts_with("magnet:")
                    || l_url.split('?').next().unwrap_or("").ends_with(".torrent")
                {
                    if let Some(cand) =
                        candidate_from_link(l_url, &link.text, &search_url, &page_text, None)
                    {
                        results.push(cand);
                    }
                }
            }
        }
    }

    dedupe_torrents(results, count * 2)
}

pub fn dedupe_torrents(results: Vec<Value>, limit: usize) -> Vec<Value> {
    let mut seen_hashes = HashSet::new();
    let mut seen_urls = HashSet::new();
    let mut unique = Vec::new();

    for res in results {
        let hash = res
            .get("info_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let url = res
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        if (!hash.is_empty() && seen_hashes.contains(&hash)) || seen_urls.contains(&url) {
            continue;
        }

        if !hash.is_empty() {
            seen_hashes.insert(hash);
        }
        seen_urls.insert(url);
        unique.push(res);

        if unique.len() >= limit {
            break;
        }
    }

    unique
}

pub fn rank_torrents(mut results: Vec<Value>, query: &str) -> Vec<Value> {
    let terms: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 2)
        .map(|s| s.to_string())
        .collect();

    results.sort_by(|a, b| {
        let score_a = score_torrent(a, &terms);
        let score_b = score_torrent(b, &terms);
        score_b.cmp(&score_a)
    });

    results
}

fn score_torrent(res: &Value, terms: &[String]) -> (usize, i64, usize, usize, usize) {
    let name = res
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let source = res
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let text = format!("{} {}", name, source);

    let relevance = terms.iter().filter(|&t| text.contains(t)).count();
    let swarm = res.get("seeders").and_then(|v| v.as_i64()).unwrap_or(0);
    let validation = if res
        .get("validation")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .starts_with("valid")
    {
        1
    } else {
        0
    };
    let trusted = if res
        .get("trusted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        1
    } else {
        0
    };
    let verified = if res
        .get("verified")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        1
    } else {
        0
    };

    (relevance, swarm, validation, trusted, verified)
}

pub async fn validate_torrent_candidate(mut cand: Value) -> Value {
    let c_type = cand.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if c_type == "magnet" {
        if let Value::Object(ref mut map) = cand {
            map.insert(
                "validation".to_string(),
                json!("valid_syntax_and_info_hash"),
            );
        }
        return cand;
    }

    let url = match cand.get("url").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => return cand,
    };

    let mut headers = HashMap::new();
    headers.insert(
        "Accept".to_string(),
        "application/x-bittorrent, application/octet-stream;q=0.8".to_string(),
    );

    match fetch_bytes(url, MAX_TORRENT_BYTES, Some(NETWORK_TIMEOUT), Some(headers)).await {
        Ok(bytes) => match parse_torrent(&bytes) {
            Ok(info) => {
                if let Value::Object(ref mut map) = cand {
                    map.insert("info_hash".to_string(), json!(info.info_hash));
                    if !info.name.is_empty() {
                        map.insert("name".to_string(), json!(info.name));
                    }
                    if info.size_bytes > 0 {
                        map.insert("size_bytes".to_string(), json!(info.size_bytes));
                    }
                    map.insert("validation".to_string(), json!("valid_torrent_metainfo"));
                }
            }
            Err(e) => {
                if let Value::Object(ref mut map) = cand {
                    map.insert("validation".to_string(), json!("unverified"));
                    map.insert("validation_error".to_string(), json!(e));
                }
            }
        },
        Err(e) => {
            if let Value::Object(ref mut map) = cand {
                map.insert("validation".to_string(), json!("unverified"));
                map.insert("validation_error".to_string(), json!(e));
            }
        }
    }

    cand
}

pub async fn torrent_search(args: &HashMap<String, Value>) -> Result<Value, String> {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) if !q.trim().is_empty() => q.trim(),
        _ => return Err("query must not be empty".to_string()),
    };

    let count = match args.get("max_results") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(5).clamp(1, 20) as usize,
        _ => 5,
    };

    let provider = match args.get("provider").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => "auto",
    };

    let should_validate = match args.get("validate") {
        Some(Value::Bool(b)) => *b,
        _ => true,
    };

    let scope = args
        .get("__activity_scope")
        .and_then(|v| v.as_str())
        .unwrap_or("stdio");

    check_search_guard(scope, "torrent_search", query)?;

    static CLEAN_WORDS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(?:find|search|download|get|me|for|a|an|the|torrent|magnet|link)\b")
            .unwrap()
    });

    let mut clean_query = CLEAN_WORDS.replace_all(query, " ").to_string();
    clean_query = clean_query.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean_query.is_empty() {
        clean_query = query.to_string();
    }

    let mut candidates = Vec::new();

    // Torznab
    candidates.extend(torznab_search(&clean_query, count).await);
    // Indexers
    candidates.extend(torrent_site_search(&clean_query, count).await);
    // Archive.org
    candidates.extend(internet_archive_torrents(&clean_query, count).await);

    // Normal web search
    let search_q = format!("{} torrent", clean_query);
    if let Ok(normal) = do_search(provider, &search_q, count * 2, None, None, false).await {
        if let Some(res_arr) = normal.get("results").and_then(|v| v.as_array()) {
            for item in res_arr {
                let item_url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let snippet = item.get("snippet").and_then(|v| v.as_str()).unwrap_or("");
                let text = format!("{} {}", item_url, snippet);

                for link in torrent_links_in_text(&text) {
                    if let Some(cand) = candidate_from_link(
                        &link,
                        item.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                        item_url,
                        &text,
                        None,
                    ) {
                        candidates.push(cand);
                    }
                }

                if item_url
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .to_lowercase()
                    .ends_with(".torrent")
                {
                    if let Some(cand) = candidate_from_link(
                        item_url,
                        item.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                        item_url,
                        &text,
                        None,
                    ) {
                        candidates.push(cand);
                    }
                }
            }

            // Crawl top pages
            let crawl_items = &res_arr[..res_arr.len().min(4.min(count))];
            let mut crawl_futs = Vec::new();
            for item in crawl_items {
                if let Some(page_url) = item.get("url").and_then(|v| v.as_str()) {
                    if !page_url.to_lowercase().ends_with(".torrent") {
                        crawl_futs.push(async move {
                            if let Ok(page_resp) =
                                fetch_html(page_url, 12000, Some(NETWORK_TIMEOUT)).await
                            {
                                let parsed = crate::html::parse_html(
                                    &page_resp.raw_html,
                                    &page_resp.final_url,
                                    12000,
                                );
                                let p_text = format!(
                                    "{} {} {}",
                                    parsed.title, parsed.description, parsed.content
                                );
                                let mut page_cands = Vec::new();
                                for link in &parsed.links {
                                    if link.url.to_lowercase().starts_with("magnet:")
                                        || link
                                            .url
                                            .split('?')
                                            .next()
                                            .unwrap_or("")
                                            .ends_with(".torrent")
                                    {
                                        if let Some(cand) = candidate_from_link(
                                            &link.url, &link.text, page_url, &p_text, None,
                                        ) {
                                            page_cands.push(cand);
                                        }
                                    }
                                }
                                page_cands
                            } else {
                                Vec::new()
                            }
                        });
                    }
                }
            }

            let crawled = futures::future::join_all(crawl_futs).await;
            for page_cands in crawled {
                candidates.extend(page_cands);
            }
        }
    }

    candidates = dedupe_torrents(candidates, usize::MAX);
    let mut ranked = rank_torrents(candidates, &clean_query);
    ranked.truncate(count * 2);

    if should_validate && !ranked.is_empty() {
        let val_futs: Vec<_> = ranked
            .into_iter()
            .map(|cand| validate_torrent_candidate(cand))
            .collect();
        ranked = futures::future::join_all(val_futs).await;
    }

    ranked = rank_torrents(dedupe_torrents(ranked, count * 2), &clean_query);
    ranked.truncate(count);

    let output = json!({
        "query": query,
        "torrent_intent": true,
        "results": ranked
    });

    Ok(enforce_output_budget(output, None))
}
