pub mod browser;
pub mod budget;
pub mod cache;
pub mod config;
pub mod guard;
pub mod health;
pub mod html;
pub mod net;
pub mod protocol;
pub mod providers;
pub mod tools;

use std::collections::HashMap;
use std::env;
use std::io::Write;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, error, info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::config::VERSION;
use crate::protocol::*;
use crate::tools::dispatch_tool;

fn send_payload(payload: &Value) {
    if let Ok(serialized) = serde_json::to_string(payload) {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        let _ = writeln!(handle, "{}", serialized);
        let _ = handle.flush();
    }
}

async fn handle_request(request: Value) {
    let req_obj = match request.as_object() {
        Some(obj) => obj,
        None => {
            send_payload(&make_rpc_error_response(None, -32600, "Invalid Request"));
            return;
        }
    };

    if req_obj.get("jsonrpc").and_then(|v| v.as_str()) != Some("2.0") {
        send_payload(&make_rpc_error_response(req_obj.get("id").cloned(), -32600, "Invalid Request"));
        return;
    }

    let method = match req_obj.get("method").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => {
            send_payload(&make_rpc_error_response(req_obj.get("id").cloned(), -32600, "Invalid Request"));
            return;
        }
    };

    let req_id = req_obj.get("id").cloned();
    let has_id = req_id.is_some();

    match method {
        "initialize" => {
            if let Some(resp) = make_initialize_response(req_id) {
                send_payload(&resp);
            }
        }
        "notifications/initialized" => {
            debug!("Client initialized");
        }
        "tools/list" => {
            if let Some(resp) = make_tools_list_response(req_id) {
                send_payload(&resp);
            }
        }
        "ping" => {
            if let Some(resp) = make_ping_response(req_id) {
                send_payload(&resp);
            }
        }
        "tools/call" => {
            let params = match req_obj.get("params").and_then(|v| v.as_object()) {
                Some(p) => p,
                None => {
                    if has_id {
                        send_payload(&make_rpc_error_response(req_id, -32602, "Invalid params"));
                    }
                    return;
                }
            };

            let tool_name = match params.get("name").and_then(|v| v.as_str()) {
                Some(n) => n,
                None => {
                    if has_id {
                        send_payload(&make_rpc_error_response(req_id, -32602, "Missing tool name"));
                    }
                    return;
                }
            };

            let mut arguments: HashMap<String, Value> = match params.get("arguments").and_then(|v| v.as_object()) {
                Some(args) => args.clone().into_iter().collect(),
                None => {
                    if has_id {
                        send_payload(&make_rpc_error_response(req_id, -32602, "Tool arguments must be an object"));
                    }
                    return;
                }
            };

            arguments.remove("__activity_scope");
            if let Some(meta) = params.get("_meta").and_then(|v| v.as_object()) {
                if let Some(scope) = meta.get("sessionId").or_else(|| meta.get("clientId")).and_then(|v| v.as_str()) {
                    arguments.insert("__activity_scope".to_string(), Value::String(scope.to_string()));
                }
            }

            match dispatch_tool(tool_name, &arguments).await {
                Ok(result) => {
                    if let Some(resp) = make_tool_success_response(req_id, result) {
                        send_payload(&resp);
                    }
                }
                Err(err) => {
                    if let Some(resp) = make_tool_error_response(req_id, &err) {
                        send_payload(&resp);
                    }
                }
            }
        }
        _ => {
            if has_id {
                send_payload(&make_rpc_error_response(req_id, -32601, "Method not found"));
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let log_level_str = env::var("LOOKUP_LOG_LEVEL")
        .unwrap_or_else(|_| "WARNING".to_string())
        .to_uppercase();

    let max_level = match log_level_str.as_str() {
        "DEBUG" => Level::DEBUG,
        "INFO" => Level::INFO,
        "WARN" | "WARNING" => Level::WARN,
        "ERROR" => Level::ERROR,
        _ => Level::WARN,
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(max_level)
        .with_writer(std::io::stderr)
        .finish();

    let _ = tracing::subscriber::set_global_default(subscriber);

    info!("Lookup MCP server v{} starting", VERSION);

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    loop {
        line.clear();
        tokio::select! {
            res = reader.read_line(&mut line) => {
                match res {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<Value>(trimmed) {
                            Ok(req) => {
                                handle_request(req).await;
                            }
                            Err(_) => {
                                send_payload(&make_rpc_error_response(None, -32700, "Parse error"));
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error reading stdin: {}", e);
                        break;
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Received interrupt signal, shutting down");
                break;
            }
        }
    }

    info!("Lookup MCP server shutting down");
}
