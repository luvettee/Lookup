use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

pub const VERSION: &str = "2.2.0";
pub const USER_AGENT: &str = "Lookup-MCP/2.2.0";

pub const SEARCH_PROVIDERS: &[&str] = &[
    "auto", "brave", "chromium", "exa", "ollama", "tavily", "searxng",
];
pub const FETCH_PROVIDERS: &[&str] = &["auto", "ollama", "tavily", "direct", "chromium"];

pub const DEFAULT_SEARXNG_INSTANCES: &[&str] = &["https://search.mectov.my.id"];
pub const SEARX_SPACE_DIRECTORY_URL: &str = "https://searx.space/data/instances.json";
pub const SEARX_DIRECTORY_TTL: Duration = Duration::from_secs(3600);
pub const SEARX_DIRECTORY_STALE_TTL: Duration = Duration::from_secs(86400);
pub const SEARX_DIRECTORY_MAX_BYTES: usize = 1_500_000;
pub const MAX_DISCOVERED_INSTANCES: usize = 32;
pub const SEARX_RACE_SIZE: usize = 3;
pub const SEARX_SEARCH_TIMEOUT: Duration = Duration::from_millis(4000);
pub const SEARX_PREFERRED_TIMEOUT: Duration = Duration::from_millis(2500);
pub const SEARX_PUBLIC_VALIDATION_BUDGET: Duration = Duration::from_millis(8000);
pub const MAX_SEARX_VALIDATION_WAVES: usize = 8;

pub const BRAVE_WEB_SEARCH_URL: &str = "https://api.search.brave.com/res/v1/web/search";
pub const BRAVE_NEWS_SEARCH_URL: &str = "https://api.search.brave.com/res/v1/news/search";
pub const EXA_SEARCH_URL: &str = "https://api.exa.ai/search";

pub const SEARCH_FAILURE_COOLDOWN: Duration = Duration::from_secs(30);
pub const PROVIDER_FAILURE_COOLDOWN: Duration = Duration::from_secs(90);
pub const AUTH_FAILURE_COOLDOWN: Duration = Duration::from_secs(600);
pub const RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(120);
pub const TIMEOUT_FAILURE_COOLDOWN: Duration = Duration::from_secs(30);
pub const CONNECTION_FAILURE_COOLDOWN: Duration = Duration::from_secs(45);
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(5);

pub const MAX_TOOL_OUTPUT_CHARS: usize = 20000;
pub const MAX_CACHE_ENTRIES: usize = 256;
pub const MAX_JSON_RESPONSE_BYTES: usize = 512_000;
pub const MAX_HTML_RESPONSE_BYTES: usize = 512_000;
pub const MAX_URL_CHARS: usize = 4096;
pub const MAX_QUERY_CHARS: usize = 1000;
pub const MAX_TORRENT_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_CHROMIUM_HTML_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SCREENSHOT_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_SCREENSHOT_WIDTH: u32 = 1920;
pub const MAX_SCREENSHOT_HEIGHT: u32 = 1080;
pub const CHROMIUM_TIMEOUT: Duration = Duration::from_secs(15);

pub const WEB_ACTIVITY_WINDOW: Duration = Duration::from_secs(60);
pub const MAX_WEB_ACTIVITY: usize = 5;
pub const MAX_SIMILAR_WEB_ACTIVITY: usize = 2;
pub const MAX_ACTIVITY_SCOPES: usize = 128;

pub static TRUSTED_TORRENT_DOMAINS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    let mut s = HashSet::new();
    s.insert("archive.org");
    s.insert("ubuntu.com");
    s.insert("debian.org");
    s.insert("fedoraproject.org");
    s.insert("linuxmint.com");
    s.insert("opensuse.org");
    s.insert("kali.org");
    s.insert("freebsd.org");
    s.insert("alpinelinux.org");
    s.insert("raspberrypi.com");
    s
});

pub fn allow_private_urls() -> bool {
    let val = env::var("LOOKUP_ALLOW_PRIVATE_URLS")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    val == "1" || val == "true" || val == "yes"
}

pub fn get_env_trimmed(name: &str) -> Option<String> {
    env::var(name).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn cache_db_path() -> Option<PathBuf> {
    match get_env_trimmed("LOOKUP_CACHE_DB") {
        Some(value) if value.eq_ignore_ascii_case("off") || value.eq_ignore_ascii_case("memory") => None,
        Some(value) => Some(PathBuf::from(value)),
        None => Some(PathBuf::from(".lookup-cache.sqlite3")),
    }
}

pub fn chromium_path() -> Option<String> {
    get_env_trimmed("LOOKUP_CHROMIUM_PATH")
}

pub fn brave_api_key() -> Option<String> {
    get_env_trimmed("BRAVE_API_KEY")
}

pub fn exa_api_key() -> Option<String> {
    get_env_trimmed("EXA_API_KEY")
}

pub fn ollama_api_key() -> Option<String> {
    get_env_trimmed("OLLAMA_API_KEY")
}

pub fn tavily_api_key() -> Option<String> {
    get_env_trimmed("TAVILY_API_KEY")
}

pub fn searxng_urls() -> Vec<String> {
    get_env_trimmed("SEARXNG_URL")
        .map(|s| {
            let mut list: Vec<String> = s
                .split(',')
                .map(|item| item.trim().trim_end_matches('/').to_string())
                .filter(|item| !item.is_empty())
                .collect();
            list.dedup();
            list
        })
        .unwrap_or_default()
}

pub fn torznab_urls() -> Vec<String> {
    get_env_trimmed("TORZNAB_URLS")
        .map(|s| {
            s.split(',')
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn torrent_site_urls() -> Vec<String> {
    get_env_trimmed("TORRENT_SITE_URLS")
        .map(|s| {
            s.split(',')
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty() && item.contains("{query}"))
                .collect()
        })
        .unwrap_or_default()
}

pub fn recency_to_days(recency: &str) -> Option<u32> {
    match recency.to_lowercase().as_str() {
        "day" => Some(1),
        "week" => Some(7),
        "month" => Some(30),
        "year" => Some(365),
        _ => None,
    }
}

pub fn brave_freshness(recency: &str) -> Option<&'static str> {
    match recency.to_lowercase().as_str() {
        "day" => Some("pd"),
        "week" => Some("pw"),
        "month" => Some("pm"),
        "year" => Some("py"),
        _ => None,
    }
}

pub fn weather_code_description(code: i64) -> &'static str {
    match code {
        0 => "clear",
        1 => "mostly clear",
        2 => "partly cloudy",
        3 => "overcast",
        45 => "fog",
        48 => "rime fog",
        51 => "light drizzle",
        53 => "drizzle",
        55 => "heavy drizzle",
        56 => "light freezing drizzle",
        57 => "freezing drizzle",
        61 => "light rain",
        63 => "rain",
        65 => "heavy rain",
        66 => "light freezing rain",
        67 => "freezing rain",
        71 => "light snow",
        73 => "snow",
        75 => "heavy snow",
        77 => "snow grains",
        80 => "light rain showers",
        81 => "rain showers",
        82 => "heavy rain showers",
        85 => "light snow showers",
        86 => "heavy snow showers",
        95 => "thunderstorm",
        96 => "thunderstorm with hail",
        99 => "thunderstorm with heavy hail",
        _ => "unknown",
    }
}
