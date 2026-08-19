use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;
use url::Url;

use crate::config::{allow_private_urls, MAX_URL_CHARS};

struct DnsCacheEntry {
    expires_at: Instant,
    ips: Vec<IpAddr>,
}

static DNS_CACHE: LazyLock<Mutex<HashMap<String, DnsCacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const DNS_CACHE_TTL_SECS: u64 = 300;
const DNS_CACHE_MAX_ENTRIES: usize = 512;

pub fn is_global_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => is_global_ipv4(ipv4),
        IpAddr::V6(ipv6) => is_global_ipv6(ipv6),
    }
}

fn is_global_ipv4(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    // 0.0.0.0/8
    if octets[0] == 0 {
        return false;
    }
    // 10.0.0.0/8
    if octets[0] == 10 {
        return false;
    }
    // 127.0.0.0/8 (Loopback)
    if octets[0] == 127 {
        return false;
    }
    // 100.64.0.0/10 (Carrier-grade NAT)
    if octets[0] == 100 && (octets[1] & 0xC0) == 64 {
        return false;
    }
    // 169.254.0.0/16 (Link-local)
    if octets[0] == 169 && octets[1] == 254 {
        return false;
    }
    // 172.16.0.0/12 (Private)
    if octets[0] == 172 && (octets[1] & 0xF0) == 16 {
        return false;
    }
    // 192.0.0.0/24 (IETF Protocol Assignments)
    if octets[0] == 192 && octets[1] == 0 && octets[2] == 0 {
        return false;
    }
    // 192.0.2.0/24 (TEST-NET-1)
    if octets[0] == 192 && octets[1] == 0 && octets[2] == 2 {
        return false;
    }
    // 192.168.0.0/16 (Private)
    if octets[0] == 192 && octets[1] == 168 {
        return false;
    }
    // 198.18.0.0/15 (Network benchmark tests)
    if octets[0] == 198 && (octets[1] & 0xFE) == 18 {
        return false;
    }
    // 198.51.100.0/24 (TEST-NET-2)
    if octets[0] == 198 && octets[1] == 51 && octets[2] == 100 {
        return false;
    }
    // 203.0.113.0/24 (TEST-NET-3)
    if octets[0] == 203 && octets[1] == 0 && octets[2] == 113 {
        return false;
    }
    // 224.0.0.0/4 (Multicast)
    if (octets[0] & 0xF0) == 224 {
        return false;
    }
    // 240.0.0.0/4 (Reserved)
    if (octets[0] & 0xF0) == 240 {
        return false;
    }
    // 255.255.255.255/32 (Broadcast)
    if *ip == Ipv4Addr::BROADCAST {
        return false;
    }
    true
}

fn is_global_ipv6(ip: &Ipv6Addr) -> bool {
    let segments = ip.segments();
    // :: (Unspecified)
    if ip.is_unspecified() {
        return false;
    }
    // ::1 (Loopback)
    if ip.is_loopback() {
        return false;
    }
    // IPv4-mapped IPv6: ::ffff:a.b.c.d
    if let Some(ipv4) = ip.to_ipv4_mapped() {
        return is_global_ipv4(&ipv4);
    }
    // fc00::/7 (Unique local)
    if (segments[0] & 0xFE00) == 0xFC00 {
        return false;
    }
    // fe80::/10 (Link-local unicast)
    if (segments[0] & 0xFFC0) == 0xFE80 {
        return false;
    }
    // ff00::/8 (Multicast)
    if (segments[0] & 0xFF00) == 0xFF00 {
        return false;
    }
    // 2001:db8::/32 (Documentation)
    if segments[0] == 0x2001 && segments[1] == 0x0DB8 {
        return false;
    }
    true
}

pub async fn resolve_host_async(host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
    let now = Instant::now();
    {
        let cache = DNS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = cache.get(host) {
            if entry.expires_at > now {
                return Ok(entry.ips.clone());
            }
        }
    }

    let socket_str = format!("{}:{}", host, port);
    let ips: Vec<IpAddr> = match tokio::net::lookup_host(&socket_str).await {
        Ok(iter) => {
            let mut resolved: Vec<IpAddr> = iter.map(|s| s.ip()).collect();
            resolved.sort();
            resolved.dedup();
            resolved
        }
        Err(_) => return Err("URL hostname could not be resolved".to_string()),
    };

    if ips.is_empty() {
        return Err("URL hostname could not be resolved".to_string());
    }

    {
        let mut cache = DNS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= DNS_CACHE_MAX_ENTRIES {
            let oldest = cache
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                cache.remove(&k);
            }
        }
        cache.insert(
            host.to_string(),
            DnsCacheEntry {
                expires_at: now + std::time::Duration::from_secs(DNS_CACHE_TTL_SECS),
                ips: ips.clone(),
            },
        );
    }

    Ok(ips)
}

pub fn resolve_host(host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
    let now = Instant::now();
    {
        let cache = DNS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = cache.get(host) {
            if entry.expires_at > now {
                return Ok(entry.ips.clone());
            }
        }
    }

    let socket_str = format!("{}:{}", host, port);
    let addrs: Vec<SocketAddr> = socket_str
        .to_socket_addrs()
        .map_err(|_| "URL hostname could not be resolved".to_string())?
        .collect();

    if addrs.is_empty() {
        return Err("URL hostname could not be resolved".to_string());
    }

    let mut ips: Vec<IpAddr> = addrs.into_iter().map(|s| s.ip()).collect();
    ips.sort();
    ips.dedup();

    {
        let mut cache = DNS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= DNS_CACHE_MAX_ENTRIES {
            let oldest = cache
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                cache.remove(&k);
            }
        }
        cache.insert(
            host.to_string(),
            DnsCacheEntry {
                expires_at: now + std::time::Duration::from_secs(DNS_CACHE_TTL_SECS),
                ips: ips.clone(),
            },
        );
    }

    Ok(ips)
}

pub fn validate_url_syntax(raw: &str) -> Result<String, String> {
    let url_str = raw.trim();
    if url_str.is_empty() {
        return Err("url must not be empty".to_string());
    }
    if url_str.len() > MAX_URL_CHARS {
        return Err("url is too long".to_string());
    }

    let parsed = Url::parse(url_str).map_err(|_| "url must be a valid http or https URL".to_string())?;

    let scheme = parsed.scheme().to_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err("url must be a valid http or https URL".to_string());
    }

    let host = match parsed.host_str() {
        Some(h) if !h.is_empty() => h.trim_end_matches('.').to_lowercase(),
        _ => return Err("url must be a valid http or https URL".to_string()),
    };

    if parsed.username() != "" || parsed.password().is_some() {
        return Err("URLs containing credentials are not allowed".to_string());
    }

    if allow_private_urls() {
        return Ok(parsed.to_string());
    }

    if host == "localhost" || host.ends_with(".localhost") {
        return Err("private or local URLs are not allowed".to_string());
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if !is_global_ip(&ip) {
            return Err("private or local URLs are not allowed".to_string());
        }
    }

    Ok(parsed.to_string())
}

pub async fn validate_url_async(raw: &str, resolve_dns: bool) -> Result<String, String> {
    let parsed_str = validate_url_syntax(raw)?;
    if !resolve_dns || allow_private_urls() {
        return Ok(parsed_str);
    }

    let parsed = Url::parse(&parsed_str).map_err(|_| "url must be a valid http or https URL".to_string())?;
    let host = parsed.host_str().unwrap_or("").trim_end_matches('.').to_lowercase();

    if let Ok(ip) = host.parse::<IpAddr>() {
        if !is_global_ip(&ip) {
            return Err("private or local URLs are not allowed".to_string());
        }
    } else {
        let port = parsed.port_or_known_default().unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });
        let ips = resolve_host_async(&host, port).await?;
        for ip in &ips {
            if !is_global_ip(ip) {
                return Err("private or local URLs are not allowed".to_string());
            }
        }
    }

    Ok(parsed_str)
}

pub fn validate_url(raw: &str, resolve_dns: bool) -> Result<String, String> {
    let parsed_str = validate_url_syntax(raw)?;
    if !resolve_dns || allow_private_urls() {
        return Ok(parsed_str);
    }

    let parsed = Url::parse(&parsed_str).map_err(|_| "url must be a valid http or https URL".to_string())?;
    let host = parsed.host_str().unwrap_or("").trim_end_matches('.').to_lowercase();

    if let Ok(ip) = host.parse::<IpAddr>() {
        if !is_global_ip(&ip) {
            return Err("private or local URLs are not allowed".to_string());
        }
    } else {
        let port = parsed.port_or_known_default().unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });
        let ips = resolve_host(&host, port)?;
        for ip in &ips {
            if !is_global_ip(ip) {
                return Err("private or local URLs are not allowed".to_string());
            }
        }
    }

    Ok(parsed_str)
}

pub fn normalize_url_key(url_str: &str) -> String {
    if let Ok(parsed) = Url::parse(url_str) {
        let scheme = parsed.scheme().to_lowercase();
        let host = parsed.host_str().unwrap_or("").to_lowercase();
        let port_str = match parsed.port() {
            Some(p) => format!(":{}", p),
            None => "".to_string(),
        };
        let path = parsed.path();
        let query = parsed.query().map(|q| format!("?{}", q)).unwrap_or_default();
        format!("{}://{}{}{}{}", scheme, host, port_str, path, query)
    } else {
        url_str.to_lowercase()
    }
}
