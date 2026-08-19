pub mod client;
pub mod ssrf;

pub use client::{fetch_bytes, fetch_html, get_client, get_json, post_json, HtmlResponse};
pub use ssrf::{is_global_ip, normalize_url_key, resolve_host, resolve_host_async, validate_url, validate_url_async, validate_url_syntax};
