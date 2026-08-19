use serde_json::{json, Map, Value};

use crate::config::{FETCH_PROVIDERS, MAX_QUERY_CHARS, MAX_URL_CHARS, SEARCH_PROVIDERS, VERSION};

pub const JSONRPC_VERSION: &str = "2.0";
pub const LATEST_STABLE_PROTOCOL_VERSION: &str = "2025-11-25";
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[
    "2025-11-25",
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];

pub const RPC_PARSE_ERROR: i32 = -32700;
pub const RPC_INVALID_REQUEST: i32 = -32600;
pub const RPC_METHOD_NOT_FOUND: i32 = -32601;
pub const RPC_INVALID_PARAMS: i32 = -32602;
pub const RPC_INTERNAL_ERROR: i32 = -32603;

const JSON_SCHEMA_DRAFT: &str = "https://json-schema.org/draft/2020-12/schema";

fn read_only_annotations(title: &str, open_world: bool) -> Value {
    json!({
        "title": title,
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": open_world
    })
}

fn tool_meta(category: &str, cost: &str) -> Value {
    json!({
        "lookup/category": category,
        "lookup/cost": cost,
        "lookup/version": VERSION
    })
}

pub fn get_tools_list() -> Value {
    json!([
        {
            "name": "web_search",
            "title": "Web Search",
            "description": "Fast web discovery. Use this when you need webpages, sources, official docs, GitHub pages, downloads, or broad web results but do not need page bodies yet. Prefer one precise query over several vague searches. Reuse returned results instead of repeating an equivalent search. If you already know the exact URL, use read_url. If the user wants both search and page content, use search_and_fetch. Torrent or magnet intent may automatically include torrent-capable sources.",
            "inputSchema": {
                "$schema": JSON_SCHEMA_DRAFT,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "query": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MAX_QUERY_CHARS,
                        "description": "A focused natural-language search query. Include the important product, project, error, version, or topic terms."
                    },
                    "max_results": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 20,
                        "default": 5,
                        "description": "Maximum number of results. Keep this small unless breadth is actually useful."
                    },
                    "provider": {
                        "type": "string",
                        "enum": SEARCH_PROVIDERS,
                        "default": "auto",
                        "description": "Search backend. Use auto unless a specific backend is required for debugging or behavior control."
                    },
                    "domain": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 253,
                        "description": "Optional hostname restriction such as github.com, docs.rs, microsoft.com, or example.org. Do not include a URL path."
                    },
                    "recency": {
                        "type": "string",
                        "enum": ["day", "week", "month", "year"],
                        "description": "Optional freshness filter. Use only when recency matters."
                    }
                },
                "required": ["query"]
            },
            "execution": { "taskSupport": "forbidden" },
            "annotations": read_only_annotations("Web Search", true),
            "_meta": tool_meta("web", "low")
        },
        {
            "name": "search_and_fetch",
            "title": "Search and Read",
            "description": "Search the web and immediately read a few strong results in one call. This is the default choice for factual questions that need source content, documentation lookups, troubleshooting, or a concise answer grounded in webpages. Prefer this over calling web_search and then read_url manually when only a few pages are needed. Do not use it for a URL you already have. Torrent or magnet requests return direct validated links rather than attempting to parse binary torrent files.",
            "inputSchema": {
                "$schema": JSON_SCHEMA_DRAFT,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "query": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MAX_QUERY_CHARS,
                        "description": "Focused search query describing exactly what needs to be found and read."
                    },
                    "max_results": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 10,
                        "default": 4,
                        "description": "Search candidates to return before fetching."
                    },
                    "fetch_results": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 5,
                        "default": 2,
                        "description": "Number of top pages to actually read. Two is usually enough."
                    },
                    "max_chars": {
                        "type": "integer",
                        "minimum": 500,
                        "maximum": 30000,
                        "default": 4000,
                        "description": "Maximum extracted characters per fetched page. Increase only when more context is needed."
                    },
                    "provider": {
                        "type": "string",
                        "enum": SEARCH_PROVIDERS,
                        "default": "auto",
                        "description": "Search backend only. Page fetching still uses Lookup's automatic fetch fallback."
                    },
                    "domain": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 253,
                        "description": "Optional hostname restriction such as docs.rs or github.com."
                    },
                    "recency": {
                        "type": "string",
                        "enum": ["day", "week", "month", "year"],
                        "description": "Optional freshness filter."
                    }
                },
                "required": ["query"]
            },
            "execution": { "taskSupport": "forbidden" },
            "annotations": read_only_annotations("Search and Read", true),
            "_meta": tool_meta("web", "medium")
        },
        {
            "name": "read_url",
            "title": "Read URL",
            "description": "Read and extract useful text from one exact URL that is already known. Use this after a search result when more detail is needed, or when the user directly provides a webpage. Do not use this tool as a search engine and do not repeatedly fetch the same URL unless different extraction options are needed. Binary files, magnets, and torrent payloads should not be treated as normal webpages.",
            "inputSchema": {
                "$schema": JSON_SCHEMA_DRAFT,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "url": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MAX_URL_CHARS,
                        "format": "uri",
                        "description": "Absolute URL to read."
                    },
                    "provider": {
                        "type": "string",
                        "enum": FETCH_PROVIDERS,
                        "default": "auto",
                        "description": "Fetch backend. Prefer auto so fallback and provider health logic remain available."
                    },
                    "max_chars": {
                        "type": "integer",
                        "minimum": 500,
                        "maximum": 30000,
                        "default": 6000,
                        "description": "Maximum number of extracted text characters to return."
                    },
                    "include_links": {
                        "type": "boolean",
                        "default": false,
                        "description": "Include useful hyperlinks discovered on the page."
                    },
                    "include_metadata": {
                        "type": "boolean",
                        "default": false,
                        "description": "Include page metadata such as title, description, canonical URL, or related fields when available."
                    }
                },
                "required": ["url"]
            },
            "execution": { "taskSupport": "forbidden" },
            "annotations": read_only_annotations("Read URL", true),
            "_meta": tool_meta("web", "low")
        },
        {
            "name": "screenshot_url",
            "title": "Screenshot URL",
            "description": "Capture a PNG screenshot of one known webpage using the local Chromium-based browser. Use this when visual layout, rendered UI, charts, or other non-text page content matters. Do not use it for ordinary text extraction; use read_url instead.",
            "inputSchema": {
                "$schema": JSON_SCHEMA_DRAFT,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "url": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MAX_URL_CHARS,
                        "format": "uri",
                        "description": "Absolute public webpage URL to capture."
                    },
                    "width": {
                        "type": "integer",
                        "minimum": 320,
                        "maximum": 1920,
                        "default": 1280,
                        "description": "Screenshot viewport width in pixels."
                    },
                    "height": {
                        "type": "integer",
                        "minimum": 320,
                        "maximum": 1080,
                        "default": 720,
                        "description": "Screenshot viewport height in pixels."
                    }
                },
                "required": ["url"]
            },
            "execution": { "taskSupport": "forbidden" },
            "annotations": read_only_annotations("Screenshot URL", true),
            "_meta": tool_meta("web", "high")
        },
        {
            "name": "research",
            "title": "Multi-Source Research",
            "description": "Gather and read a small, diverse set of strong sources for questions where one page is not enough. Use for comparisons, verification, technical research, disputed facts, or requests that explicitly need multiple sources. Keep max_sources low by default; quality and source diversity are more useful than collecting many near-duplicate pages. Do not use this for a simple lookup that search_and_fetch can answer.",
            "inputSchema": {
                "$schema": JSON_SCHEMA_DRAFT,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "query": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MAX_QUERY_CHARS,
                        "description": "Research question or focused topic. Include the comparison/verification target when applicable."
                    },
                    "max_sources": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 10,
                        "default": 3,
                        "description": "Maximum number of sources to read. Three strong sources is the normal default."
                    },
                    "max_chars_per_source": {
                        "type": "integer",
                        "minimum": 500,
                        "maximum": 50000,
                        "default": 5000,
                        "description": "Maximum extracted characters from each source."
                    },
                    "recency": {
                        "type": "string",
                        "enum": ["day", "week", "month", "year"],
                        "description": "Optional freshness filter when current information matters."
                    },
                    "provider": {
                        "type": "string",
                        "enum": SEARCH_PROVIDERS,
                        "default": "auto",
                        "description": "Search backend only. Fetching uses automatic fallback."
                    }
                },
                "required": ["query"]
            },
            "execution": { "taskSupport": "forbidden" },
            "annotations": read_only_annotations("Multi-Source Research", true),
            "_meta": tool_meta("research", "high")
        },
        {
            "name": "news_search",
            "title": "News Search",
            "description": "Find recent news and time-sensitive developments. Use for current events, releases, outages, announcements, incidents, company changes, or anything where publication date matters. Prefer a narrow recency window and a precise entity/topic query. Use web_search instead for timeless documentation or general discovery.",
            "inputSchema": {
                "$schema": JSON_SCHEMA_DRAFT,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "query": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MAX_QUERY_CHARS,
                        "description": "News topic, entity, product, project, or event."
                    },
                    "max_results": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 10,
                        "default": 5,
                        "description": "Maximum number of news results."
                    },
                    "recency": {
                        "type": "string",
                        "enum": ["day", "week", "month", "year"],
                        "default": "week",
                        "description": "Freshness window. Choose the shortest window that can answer the question."
                    },
                    "provider": {
                        "type": "string",
                        "enum": SEARCH_PROVIDERS,
                        "default": "auto",
                        "description": "Search backend. Prefer auto."
                    }
                },
                "required": ["query"]
            },
            "execution": { "taskSupport": "forbidden" },
            "annotations": read_only_annotations("News Search", true),
            "_meta": tool_meta("news", "low")
        },
        {
            "name": "page_links",
            "title": "Page Links",
            "description": "Extract useful outgoing links from a webpage you already know. Use for site navigation, finding documentation sections, releases, downloads, repository links, pagination, or related pages without re-searching the entire web. Prefer this over another web_search when the target is likely linked from the current page.",
            "inputSchema": {
                "$schema": JSON_SCHEMA_DRAFT,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "url": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MAX_URL_CHARS,
                        "format": "uri",
                        "description": "Absolute webpage URL whose links should be extracted."
                    },
                    "max_links": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 25,
                        "default": 10,
                        "description": "Maximum number of useful links to return."
                    }
                },
                "required": ["url"]
            },
            "execution": { "taskSupport": "forbidden" },
            "annotations": read_only_annotations("Page Links", true),
            "_meta": tool_meta("web", "low")
        },
        {
            "name": "weather",
            "title": "Weather",
            "description": "Get current weather plus a short forecast for a named location. Use this instead of web search for ordinary weather questions. Keep the forecast window limited to what the user needs.",
            "inputSchema": {
                "$schema": JSON_SCHEMA_DRAFT,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "location": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 200,
                        "description": "City, region, or recognizable location such as Vancouver, BC or Tokyo, Japan."
                    },
                    "days": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 7,
                        "default": 3,
                        "description": "Forecast length in days."
                    }
                },
                "required": ["location"]
            },
            "execution": { "taskSupport": "forbidden" },
            "annotations": read_only_annotations("Weather", true),
            "_meta": tool_meta("utility", "low")
        },
        {
            "name": "current_time",
            "title": "Current Time",
            "description": "Get the current date and time for a timezone. Use this for timezone-aware current-time questions instead of estimating from system context. Omit timezone only when local server time is acceptable.",
            "inputSchema": {
                "$schema": JSON_SCHEMA_DRAFT,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "timezone": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 128,
                        "description": "IANA timezone name such as America/Vancouver, Europe/London, or Asia/Tokyo. Defaults to local time when omitted."
                    }
                }
            },
            "execution": { "taskSupport": "forbidden" },
            "annotations": read_only_annotations("Current Time", false),
            "_meta": tool_meta("utility", "low")
        },
        {
            "name": "calculate",
            "title": "Calculator",
            "description": "Evaluate a mathematical expression deterministically. Use for arithmetic and supported mathematical expressions rather than doing mental math. Pass only the expression, not prose.",
            "inputSchema": {
                "$schema": JSON_SCHEMA_DRAFT,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "expression": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 4096,
                        "description": "Mathematical expression to evaluate, for example (1440*0.18)+25."
                    }
                },
                "required": ["expression"]
            },
            "execution": { "taskSupport": "forbidden" },
            "annotations": read_only_annotations("Calculator", false),
            "_meta": tool_meta("utility", "low")
        },
        {
            "name": "convert_units",
            "title": "Unit Converter",
            "description": "Convert a numeric value between supported compatible units. Use this for common distance, mass, temperature, volume, speed, and other unit conversions instead of manually calculating conversion factors.",
            "inputSchema": {
                "$schema": JSON_SCHEMA_DRAFT,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "value": {
                        "type": "number",
                        "description": "Numeric value to convert."
                    },
                    "from_unit": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 64,
                        "description": "Source unit, for example km, mi, c, f, kg, lb, gb, or mb."
                    },
                    "to_unit": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 64,
                        "description": "Destination unit compatible with from_unit."
                    }
                },
                "required": ["value", "from_unit", "to_unit"]
            },
            "execution": { "taskSupport": "forbidden" },
            "annotations": read_only_annotations("Unit Converter", false),
            "_meta": tool_meta("utility", "low")
        },
        {
            "name": "torrent_search",
            "title": "Torrent Search",
            "description": "Search specifically for BitTorrent resources when the user explicitly wants a torrent, magnet, swarm metadata, or a direct .torrent link. Returns direct links plus hash, size, swarm/source metadata, and trust signals when available. Prefer web_search for ordinary web downloads. Validation should normally stay enabled so dead or malformed candidates are filtered.",
            "inputSchema": {
                "$schema": JSON_SCHEMA_DRAFT,
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "query": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MAX_QUERY_CHARS,
                        "description": "Torrent search query. Include exact title/version/edition when known."
                    },
                    "max_results": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 20,
                        "default": 5,
                        "description": "Maximum torrent or magnet candidates to return."
                    },
                    "provider": {
                        "type": "string",
                        "enum": SEARCH_PROVIDERS,
                        "default": "auto",
                        "description": "Search provider. Prefer auto."
                    },
                    "validate": {
                        "type": "boolean",
                        "default": true,
                        "description": "Validate and filter candidates when possible. Disable only for diagnostic or exhaustive searches."
                    }
                },
                "required": ["query"]
            },
            "execution": { "taskSupport": "forbidden" },
            "annotations": read_only_annotations("Torrent Search", true),
            "_meta": tool_meta("torrent", "medium")
        }
    ])
}

pub fn get_server_instructions() -> &'static str {
    "Lookup is a fast, read-only research and utility server. Choose the narrowest tool that solves the request. Use read_url for text from a known URL; screenshot_url when visual page layout matters; web_search for discovery; search_and_fetch for discovery plus a small amount of page content; research only when multiple independent sources materially improve the answer; news_search for recent developments; and page_links to navigate within a known site. Reuse prior results and URLs instead of repeating equivalent searches. Prefer one precise call over several speculative calls. Keep result counts and fetched text small unless the task actually needs breadth or depth. Use weather, current_time, calculate, and convert_units instead of web search for those utilities. Use torrent_search only when the user explicitly asks for torrents, magnet links, or swarm metadata."
}

pub fn negotiate_protocol_version(requested: Option<&str>) -> &'static str {
    requested
        .and_then(|version| {
            SUPPORTED_PROTOCOL_VERSIONS
                .iter()
                .copied()
                .find(|candidate| *candidate == version)
        })
        .unwrap_or(LATEST_STABLE_PROTOCOL_VERSION)
}

pub fn make_initialize_response(id: Option<Value>) -> Option<Value> {
    make_initialize_response_for_protocol(id, None)
}

pub fn make_initialize_response_for_protocol(
    id: Option<Value>,
    requested_protocol_version: Option<&str>,
) -> Option<Value> {
    let protocol_version = negotiate_protocol_version(requested_protocol_version);

    id.map(|req_id| {
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": req_id,
            "result": {
                "protocolVersion": protocol_version,
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": "lookup",
                    "title": "Lookup",
                    "version": VERSION
                },
                "instructions": get_server_instructions(),
                "_meta": {
                    "lookup/readOnly": true,
                    "lookup/toolCount": 12,
                    "lookup/version": VERSION
                }
            }
        })
    })
}

pub fn make_tools_list_response(id: Option<Value>) -> Option<Value> {
    id.map(|req_id| {
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": req_id,
            "result": {
                "tools": get_tools_list(),
                "_meta": {
                    "lookup/version": VERSION,
                    "lookup/readOnly": true
                }
            }
        })
    })
}

pub fn make_ping_response(id: Option<Value>) -> Option<Value> {
    id.map(|req_id| make_result_response(req_id, json!({})))
}

fn tool_success_parts(mut result: Value) -> (Value, Value) {
    let image = result
        .as_object_mut()
        .and_then(|object| object.remove("_mcp_image"));
    let serialized = serde_json::to_string(&result)
        .unwrap_or_else(|_| "{\"error\":\"failed to serialize tool result\"}".to_string());
    let structured = normalize_structured_content(result);
    let mut content = vec![json!({
        "type": "text",
        "text": serialized
    })];

    if let Some(image) = image {
        if let (Some(data), Some(mime_type)) = (
            image.get("data").and_then(Value::as_str),
            image.get("mime_type").and_then(Value::as_str),
        ) {
            content.push(json!({
                "type": "image",
                "data": data,
                "mimeType": mime_type
            }));
        }
    }

    (Value::Array(content), structured)
}

pub fn make_tool_success_response(id: Option<Value>, result: Value) -> Option<Value> {
    id.map(|req_id| {
        let (content, structured) = tool_success_parts(result);
        make_result_response(
            req_id,
            json!({
                "content": content,
                "structuredContent": structured,
                "isError": false,
                "_meta": {
                    "lookup/version": VERSION
                }
            }),
        )
    })
}

pub fn make_tool_success_text_response(id: Option<Value>, text: impl Into<String>) -> Option<Value> {
    let text = text.into();
    id.map(|req_id| {
        make_result_response(
            req_id,
            json!({
                "content": [
                    {
                        "type": "text",
                        "text": text
                    }
                ],
                "isError": false,
                "_meta": {
                    "lookup/version": VERSION
                }
            }),
        )
    })
}

pub fn make_tool_success_response_with_meta(
    id: Option<Value>,
    result: Value,
    meta: Value,
) -> Option<Value> {
    id.map(|req_id| {
        let (content, structured) = tool_success_parts(result);
        make_result_response(
            req_id,
            json!({
                "content": content,
                "structuredContent": structured,
                "isError": false,
                "_meta": merge_meta(meta)
            }),
        )
    })
}

pub fn make_tool_error_response(id: Option<Value>, error_msg: &str) -> Option<Value> {
    make_tool_error_response_with_data(id, error_msg, None)
}

pub fn make_tool_error_response_with_data(
    id: Option<Value>,
    error_msg: &str,
    data: Option<Value>,
) -> Option<Value> {
    id.map(|req_id| {
        let structured = match data {
            Some(data) => json!({
                "error": error_msg,
                "data": data
            }),
            None => json!({
                "error": error_msg
            }),
        };

        make_result_response(
            req_id,
            json!({
                "content": [
                    {
                        "type": "text",
                        "text": format!("Error: {error_msg}")
                    }
                ],
                "structuredContent": structured,
                "isError": true,
                "_meta": {
                    "lookup/version": VERSION
                }
            }),
        )
    })
}

pub fn make_rpc_error_response(id: Option<Value>, code: i32, message: &str) -> Value {
    make_rpc_error_response_with_data(id, code, message, None)
}

pub fn make_rpc_error_response_with_data(
    id: Option<Value>,
    code: i32,
    message: &str,
    data: Option<Value>,
) -> Value {
    let mut error = Map::new();
    error.insert("code".to_string(), json!(code));
    error.insert("message".to_string(), json!(message));
    if let Some(data) = data {
        error.insert("data".to_string(), data);
    }

    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.unwrap_or(Value::Null),
        "error": Value::Object(error)
    })
}

pub fn make_parse_error_response(message: &str) -> Value {
    make_rpc_error_response(None, RPC_PARSE_ERROR, message)
}

pub fn make_invalid_request_response(id: Option<Value>, message: &str) -> Value {
    make_rpc_error_response(id, RPC_INVALID_REQUEST, message)
}

pub fn make_method_not_found_response(id: Option<Value>, method: &str) -> Value {
    make_rpc_error_response_with_data(
        id,
        RPC_METHOD_NOT_FOUND,
        "Method not found",
        Some(json!({ "method": method })),
    )
}

pub fn make_invalid_params_response(
    id: Option<Value>,
    message: &str,
    details: Option<Value>,
) -> Value {
    make_rpc_error_response_with_data(id, RPC_INVALID_PARAMS, message, details)
}

pub fn make_internal_error_response(id: Option<Value>, message: &str) -> Value {
    make_rpc_error_response(id, RPC_INTERNAL_ERROR, message)
}

pub fn make_progress_notification(
    progress_token: Value,
    progress: f64,
    total: Option<f64>,
    message: Option<&str>,
) -> Value {
    let mut params = Map::new();
    params.insert("progressToken".to_string(), progress_token);
    params.insert("progress".to_string(), json!(progress.max(0.0)));

    if let Some(total) = total {
        params.insert("total".to_string(), json!(total.max(0.0)));
    }
    if let Some(message) = message {
        params.insert("message".to_string(), json!(message));
    }

    make_notification("notifications/progress", Value::Object(params))
}

pub fn make_cancelled_notification(request_id: Value, reason: Option<&str>) -> Value {
    let mut params = Map::new();
    params.insert("requestId".to_string(), request_id);
    if let Some(reason) = reason {
        params.insert("reason".to_string(), json!(reason));
    }

    make_notification("notifications/cancelled", Value::Object(params))
}

pub fn make_tools_list_changed_notification() -> Value {
    make_notification("notifications/tools/list_changed", json!({}))
}

pub fn make_log_notification(level: &str, logger: Option<&str>, data: Value) -> Value {
    let mut params = Map::new();
    params.insert("level".to_string(), json!(level));
    params.insert("data".to_string(), data);
    if let Some(logger) = logger {
        params.insert("logger".to_string(), json!(logger));
    }

    make_notification("notifications/message", Value::Object(params))
}

pub fn make_empty_resources_list_response(id: Option<Value>) -> Option<Value> {
    id.map(|req_id| {
        make_result_response(
            req_id,
            json!({
                "resources": []
            }),
        )
    })
}

pub fn make_empty_resource_templates_list_response(id: Option<Value>) -> Option<Value> {
    id.map(|req_id| {
        make_result_response(
            req_id,
            json!({
                "resourceTemplates": []
            }),
        )
    })
}

pub fn make_empty_prompts_list_response(id: Option<Value>) -> Option<Value> {
    id.map(|req_id| {
        make_result_response(
            req_id,
            json!({
                "prompts": []
            }),
        )
    })
}

fn make_result_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "result": result
    })
}

fn make_notification(method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "method": method,
        "params": params
    })
}

fn normalize_structured_content(result: Value) -> Value {
    match result {
        Value::Object(_) => result,
        other => json!({ "value": other }),
    }
}

fn merge_meta(meta: Value) -> Value {
    let mut merged = match meta {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        other => {
            let mut map = Map::new();
            map.insert("lookup/data".to_string(), other);
            map
        }
    };

    merged
        .entry("lookup/version".to_string())
        .or_insert_with(|| json!(VERSION));

    Value::Object(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_list_contains_expected_tools() {
        let tools = get_tools_list();
        let tools = tools.as_array().expect("tools must be an array");
        assert_eq!(tools.len(), 12);
        assert!(tools.iter().any(|tool| tool["name"] == "web_search"));
        assert!(tools.iter().any(|tool| tool["name"] == "screenshot_url"));
        assert!(tools.iter().any(|tool| tool["name"] == "research"));
        assert!(tools.iter().any(|tool| tool["name"] == "torrent_search"));
    }

    #[test]
    fn all_tool_schemas_are_closed_objects() {
        let tools = get_tools_list();
        for tool in tools.as_array().expect("tools must be an array") {
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        }
    }

    #[test]
    fn protocol_negotiation_prefers_supported_requested_version() {
        assert_eq!(
            negotiate_protocol_version(Some("2024-11-05")),
            "2024-11-05"
        );
        assert_eq!(
            negotiate_protocol_version(Some("not-a-real-version")),
            LATEST_STABLE_PROTOCOL_VERSION
        );
    }

    #[test]
    fn tool_success_contains_text_and_structured_content() {
        let response = make_tool_success_response(Some(json!(1)), json!({ "ok": true }))
            .expect("request id should produce a response");

        assert_eq!(response["result"]["isError"], false);
        assert_eq!(response["result"]["structuredContent"]["ok"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .expect("text content")
            .contains("ok"));
    }

    #[test]
    fn tool_success_emits_image_content_without_base64_in_structured_content() {
        let response = make_tool_success_response(
            Some(json!(1)),
            json!({
                "url": "https://example.com/",
                "_mcp_image": {
                    "mime_type": "image/png",
                    "data": "cG5n"
                }
            }),
        )
        .expect("request id should produce a response");

        assert_eq!(response["result"]["content"][1]["type"], "image");
        assert_eq!(response["result"]["content"][1]["mimeType"], "image/png");
        assert_eq!(response["result"]["content"][1]["data"], "cG5n");
        assert!(response["result"]["structuredContent"]
            .get("_mcp_image")
            .is_none());
    }

    #[test]
    fn rpc_error_uses_null_id_when_request_id_is_unknown() {
        let response = make_parse_error_response("invalid json");
        assert_eq!(response["id"], Value::Null);
        assert_eq!(response["error"]["code"], RPC_PARSE_ERROR);
    }
}
