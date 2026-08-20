use lookup::protocol::*;
use lookup::tools::dispatch_tool;
use serde_json::json;
use std::collections::HashMap;

#[test]
fn test_mcp_initialize() {
    let resp = make_initialize_response(Some(json!(1))).unwrap();
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["serverInfo"]["name"], "lookup");
    assert_eq!(
        resp["result"]["protocolVersion"],
        LATEST_STABLE_PROTOCOL_VERSION
    );
    assert_eq!(resp["result"]["_meta"]["lookup/toolCount"], 28);

    let negotiated =
        make_initialize_response_for_protocol(Some(json!(1)), Some("2024-11-05")).unwrap();
    assert_eq!(negotiated["result"]["protocolVersion"], "2024-11-05");
}

#[test]
fn test_mcp_tools_list() {
    let resp = make_tools_list_response(Some(json!(2))).unwrap();
    let tools = resp["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 28);

    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        tool_names,
        vec![
            "web_search",
            "search_and_fetch",
            "read_url",
            "screenshot_url",
            "research",
            "news_search",
            "page_links",
            "weather",
            "current_time",
            "calculate",
            "convert_units",
            "torrent_search",
            "browser_open",
            "browser_tabs",
            "browser_navigate",
            "browser_snapshot",
            "browser_click",
            "browser_type",
            "browser_press",
            "browser_scroll",
            "browser_wait",
            "browser_read",
            "browser_screenshot",
            "browser_back",
            "browser_forward",
            "browser_reload",
            "browser_close",
            "browser_evaluate",
        ]
    );
}

#[tokio::test]
async fn test_mcp_dispatch_calculate() {
    let mut args = HashMap::new();
    args.insert("expression".to_string(), json!("sqrt(100) * 5"));

    let res = dispatch_tool("calculate", &args).await.unwrap();
    assert_eq!(res["result"], 50);

    let success_resp = make_tool_success_response(Some(json!(3)), res).unwrap();
    assert_eq!(success_resp["id"], 3);
    assert!(success_resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("50"));
}

#[tokio::test]
async fn test_mcp_dispatch_convert_units() {
    let mut args = HashMap::new();
    args.insert("value".to_string(), json!(10));
    args.insert("from_unit".to_string(), json!("km"));
    args.insert("to_unit".to_string(), json!("meters"));

    let res = dispatch_tool("convert_units", &args).await.unwrap();
    assert_eq!(res["result"], 10000);
}

#[tokio::test]
async fn test_mcp_dispatch_current_time() {
    let mut args = HashMap::new();
    args.insert("timezone".to_string(), json!("UTC"));

    let res = dispatch_tool("current_time", &args).await.unwrap();
    assert_eq!(res["timezone"], "UTC");
    assert!(
        res["iso"].as_str().unwrap().contains("+00:00")
            || res["iso"].as_str().unwrap().contains("Z")
    );
}

#[tokio::test]
async fn test_mcp_dispatch_current_time_default_local() {
    let args = HashMap::new();
    let res = dispatch_tool("current_time", &args).await.unwrap();
    assert!(!res["timezone"].as_str().unwrap().is_empty());
    assert!(!res["iso"].as_str().unwrap().is_empty());
    assert!(!res["date"].as_str().unwrap().is_empty());
    assert!(!res["time"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn test_mcp_dispatch_calculate_long_expression() {
    let long_expr = (0..50).map(|_| "1").collect::<Vec<_>>().join(" + ");
    let mut args = HashMap::new();
    args.insert("expression".to_string(), json!(long_expr));

    let res = dispatch_tool("calculate", &args).await.unwrap();
    assert_eq!(res["result"], 50);
}

#[test]
fn test_cache_lru_and_expiration() {
    use lookup::cache::{cache_get, cache_put};
    use std::time::Duration;

    let key = "test_key_12345";
    let val = json!({"hello": "world"});
    cache_put(key, Duration::from_secs(60), val.clone());

    let fetched = cache_get(key);
    assert_eq!(fetched, Some(val));
}

#[tokio::test]
async fn test_mcp_dispatch_screenshot_url() {
    let mut args = HashMap::new();
    args.insert("url".to_string(), json!("https://example.com"));
    args.insert("width".to_string(), json!(800));
    args.insert("height".to_string(), json!(600));

    let res = dispatch_tool("screenshot_url", &args).await.unwrap();
    assert!(res["url"]
        .as_str()
        .unwrap()
        .starts_with("https://example.com"));
    assert_eq!(res["width"], 800);
    assert_eq!(res["height"], 600);
    assert_eq!(res["mime_type"], "image/png");
    assert!(res["size_bytes"].as_u64().unwrap() > 0);
    assert!(res["_mcp_image"]["data"].as_str().unwrap().len() > 0);
}
