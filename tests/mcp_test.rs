use std::collections::HashMap;
use lookup::protocol::*;
use lookup::tools::dispatch_tool;
use serde_json::json;

#[test]
fn test_mcp_initialize() {
    let resp = make_initialize_response(Some(json!(1))).unwrap();
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["serverInfo"]["name"], "lookup");
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
}

#[test]
fn test_mcp_tools_list() {
    let resp = make_tools_list_response(Some(json!(2))).unwrap();
    let tools = resp["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 12);

    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(tool_names.contains(&"web_search"));
    assert!(tool_names.contains(&"search_and_fetch"));
    assert!(tool_names.contains(&"read_url"));
    assert!(tool_names.contains(&"screenshot_url"));
    assert!(tool_names.contains(&"research"));
    assert!(tool_names.contains(&"news_search"));
    assert!(tool_names.contains(&"page_links"));
    assert!(tool_names.contains(&"weather"));
    assert!(tool_names.contains(&"current_time"));
    assert!(tool_names.contains(&"calculate"));
    assert!(tool_names.contains(&"convert_units"));
    assert!(tool_names.contains(&"torrent_search"));
}

#[tokio::test]
async fn test_mcp_dispatch_calculate() {
    let mut args = HashMap::new();
    args.insert("expression".to_string(), json!("sqrt(100) * 5"));

    let res = dispatch_tool("calculate", &args).await.unwrap();
    assert_eq!(res["result"], 50);

    let success_resp = make_tool_success_response(Some(json!(3)), res).unwrap();
    assert_eq!(success_resp["id"], 3);
    assert!(success_resp["result"]["content"][0]["text"].as_str().unwrap().contains("50"));
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
    assert!(res["iso"].as_str().unwrap().contains("+00:00") || res["iso"].as_str().unwrap().contains("Z"));
}
