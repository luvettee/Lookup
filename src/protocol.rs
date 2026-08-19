use serde_json::{json, Value};

use crate::config::{FETCH_PROVIDERS, MAX_QUERY_CHARS, MAX_URL_CHARS, SEARCH_PROVIDERS, VERSION};

pub fn get_tools_list() -> Value {
    json!([
        {
            "name": "web_search",
            "description": "Find webpages about a topic. Natural requests for torrents or magnets automatically include torrent-capable sources and return direct links. Do not repeatedly search the same question; use previous results when available.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "minLength": 1, "maxLength": MAX_QUERY_CHARS },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 20, "default": 5 },
                    "provider": { "type": "string", "enum": SEARCH_PROVIDERS, "default": "auto" },
                    "domain": { "type": "string", "description": "Restrict to a domain like github.com" },
                    "recency": { "type": "string", "enum": ["day", "week", "month", "year"] }
                },
                "required": ["query"]
            }
        },
        {
            "name": "search_and_fetch",
            "description": "Search for a topic and read a few of the best results. Torrent/magnet requests automatically return validated direct links instead of trying to read binary files. The provider option controls search only; page fetching uses automatic fallback and reports the fetch provider.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "minLength": 1, "maxLength": MAX_QUERY_CHARS },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 10, "default": 4 },
                    "fetch_results": { "type": "integer", "minimum": 1, "maximum": 5, "default": 2 },
                    "max_chars": { "type": "integer", "minimum": 500, "maximum": 30000, "default": 4000 },
                    "provider": { "type": "string", "enum": SEARCH_PROVIDERS, "default": "auto", "description": "Search provider only. Page fetching uses automatic fallback." },
                    "domain": { "type": "string" },
                    "recency": { "type": "string", "enum": ["day", "week", "month", "year"] }
                },
                "required": ["query"]
            }
        },
        {
            "name": "read_url",
            "description": "Read one specific URL that is already known. Do not use this to search the web.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "maxLength": MAX_URL_CHARS },
                    "provider": { "type": "string", "enum": FETCH_PROVIDERS, "default": "auto" },
                    "max_chars": { "type": "integer", "minimum": 500, "maximum": 30000, "default": 6000 },
                    "include_links": { "type": "boolean", "default": false },
                    "include_metadata": { "type": "boolean", "default": false }
                },
                "required": ["url"]
            }
        },
        {
            "name": "research",
            "description": "Gather a small set of strong sources about a topic. Use when multiple sources are needed. The provider option controls search only; page fetching uses automatic fallback and reports the fetch provider.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "minLength": 1, "maxLength": MAX_QUERY_CHARS },
                    "max_sources": { "type": "integer", "minimum": 1, "maximum": 10, "default": 3 },
                    "max_chars_per_source": { "type": "integer", "minimum": 500, "maximum": 50000, "default": 5000 },
                    "recency": { "type": "string", "enum": ["day", "week", "month", "year"] },
                    "provider": { "type": "string", "enum": SEARCH_PROVIDERS, "default": "auto", "description": "Search provider only. Page fetching uses automatic fallback." }
                },
                "required": ["query"]
            }
        },
        {
            "name": "news_search",
            "description": "Find recent news articles about a topic. Use this for current events and recent developments.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "minLength": 1, "maxLength": MAX_QUERY_CHARS },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 10, "default": 5 },
                    "recency": { "type": "string", "enum": ["day", "week", "month", "year"], "default": "week" },
                    "provider": { "type": "string", "enum": SEARCH_PROVIDERS, "default": "auto" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "page_links",
            "description": "List useful links from a specific webpage. Use this to navigate from a page you already have.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "maxLength": MAX_URL_CHARS },
                    "max_links": { "type": "integer", "minimum": 1, "maximum": 25, "default": 10 }
                },
                "required": ["url"]
            }
        },
        {
            "name": "weather",
            "description": "Get current weather and a forecast for a location.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "location": { "type": "string" },
                    "days": { "type": "integer", "minimum": 1, "maximum": 7, "default": 3 }
                },
                "required": ["location"]
            }
        },
        {
            "name": "current_time",
            "description": "Get the current date and time in a timezone.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "timezone": { "type": "string", "description": "For example America/Vancouver. Defaults to local time." }
                }
            }
        },
        {
            "name": "calculate",
            "description": "Calculate a mathematical expression.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "expression": { "type": "string" }
                },
                "required": ["expression"]
            }
        },
        {
            "name": "convert_units",
            "description": "Convert common units such as kilometers to miles, Celsius to Fahrenheit, or kilograms to pounds.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "value": { "type": "number" },
                    "from_unit": { "type": "string" },
                    "to_unit": { "type": "string" }
                },
                "required": ["value", "from_unit", "to_unit"]
            }
        },
        {
            "name": "torrent_search",
            "description": "Find direct BitTorrent downloads. Returns direct .torrent or magnet links with hash, size, swarm metadata, source, and trust signals when available. Use for natural requests mentioning torrents or magnets.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "minLength": 1, "maxLength": MAX_QUERY_CHARS },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 20, "default": 5 },
                    "provider": { "type": "string", "enum": SEARCH_PROVIDERS, "default": "auto" },
                    "validate": { "type": "boolean", "default": true }
                },
                "required": ["query"]
            }
        }
    ])
}

pub fn make_initialize_response(id: Option<Value>) -> Option<Value> {
    id.map(|req_id| {
        json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "lookup",
                    "version": VERSION
                }
            }
        })
    })
}

pub fn make_tools_list_response(id: Option<Value>) -> Option<Value> {
    id.map(|req_id| {
        json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "tools": get_tools_list()
            }
        })
    })
}

pub fn make_ping_response(id: Option<Value>) -> Option<Value> {
    id.map(|req_id| {
        json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {}
        })
    })
}

pub fn make_tool_success_response(id: Option<Value>, result: Value) -> Option<Value> {
    id.map(|req_id| {
        let serialized = serde_json::to_string(&result).unwrap_or_default();
        json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": serialized
                    }
                ]
            }
        })
    })
}

pub fn make_tool_error_response(id: Option<Value>, error_msg: &str) -> Option<Value> {
    id.map(|req_id| {
        json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": format!("Error: {}", error_msg)
                    }
                ],
                "isError": true
            }
        })
    })
}

pub fn make_rpc_error_response(id: Option<Value>, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}
