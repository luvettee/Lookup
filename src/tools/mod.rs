pub mod calculate;
pub mod convert_units;
pub mod news_search;
pub mod page_links;
pub mod read_url;
pub mod research;
pub mod search_and_fetch;
pub mod screenshot_url;
pub mod time;
pub mod torrent;
pub mod weather;
pub mod web_search;

use std::collections::HashMap;
use serde_json::Value;

pub async fn dispatch_tool(name: &str, args: &HashMap<String, Value>) -> Result<Value, String> {
    match name {
        "web_search" => web_search::web_search(args).await,
        "search_and_fetch" => search_and_fetch::search_and_fetch(args).await,
        "read_url" => read_url::read_url(args).await,
        "screenshot_url" => screenshot_url::screenshot_url(args).await,
        "research" => research::research(args).await,
        "news_search" => news_search::news_search(args).await,
        "page_links" => page_links::page_links(args).await,
        "weather" => weather::weather(args).await,
        "current_time" => time::current_time(args),
        "calculate" => calculate::calculate(args),
        "convert_units" => convert_units::convert_units(args),
        "torrent_search" => torrent::torrent_search(args).await,
        _ => Err(format!("Unknown tool: {}", name)),
    }
}
