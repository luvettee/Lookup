use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use crate::browser::manager::global_manager;
use crate::browser::types::{BrowserError, SnapshotMode};

pub async fn dispatch(name: &str, args: &HashMap<String, Value>) -> Result<Value, String> {
    let result = match name {
        "browser_open" => global_manager().open(required_string(args, "url")?).await,
        "browser_tabs" => global_manager().tabs().await,
        "browser_navigate" => {
            global_manager()
                .navigate(
                    optional_string(args, "page_id"),
                    required_string(args, "url")?,
                )
                .await
        }
        "browser_snapshot" => {
            let mode = SnapshotMode::parse(optional_string(args, "mode")).map_err(format_error)?;
            let max_elements = optional_usize(args, "max_elements", 200)?;
            global_manager()
                .snapshot(optional_string(args, "page_id"), mode, max_elements)
                .await
        }
        "browser_click" => {
            let element_id = optional_element_id(args)?;
            let selector = optional_string(args, "selector");
            let coordinates = match (optional_f64(args, "x")?, optional_f64(args, "y")?) {
                (Some(x), Some(y)) => Some((x, y)),
                (None, None) => None,
                _ => {
                    return Err(format_error(BrowserError::invalid(
                        "x and y must be provided together",
                    )))
                }
            };
            global_manager()
                .click(
                    optional_string(args, "page_id"),
                    element_id,
                    selector,
                    coordinates,
                )
                .await
        }
        "browser_type" => {
            global_manager()
                .type_text(
                    optional_string(args, "page_id"),
                    optional_element_id(args)?,
                    optional_string(args, "selector"),
                    required_string(args, "text")?,
                    optional_bool(args, "clear", true)?,
                )
                .await
        }
        "browser_press" => {
            global_manager()
                .press(
                    optional_string(args, "page_id"),
                    required_string(args, "key")?,
                )
                .await
        }
        "browser_scroll" => {
            global_manager()
                .scroll(
                    optional_string(args, "page_id"),
                    optional_string(args, "direction"),
                    optional_i64(args, "amount", 600)?,
                    optional_element_id(args)?,
                )
                .await
        }
        "browser_wait" => {
            let timeout_ms = optional_u64(args, "timeout_ms", 10_000)?.min(30_000);
            global_manager()
                .wait(
                    optional_string(args, "page_id"),
                    required_string(args, "condition")?,
                    optional_string(args, "value"),
                    Duration::from_millis(timeout_ms),
                )
                .await
        }
        "browser_read" => {
            global_manager()
                .read(
                    optional_string(args, "page_id"),
                    optional_usize(args, "max_chars", 12_000)?,
                )
                .await
        }
        "browser_screenshot" => {
            global_manager()
                .screenshot(
                    optional_string(args, "page_id"),
                    optional_bool(args, "full_page", false)?,
                )
                .await
        }
        "browser_back" => {
            global_manager()
                .history(optional_string(args, "page_id"), -1)
                .await
        }
        "browser_forward" => {
            global_manager()
                .history(optional_string(args, "page_id"), 1)
                .await
        }
        "browser_reload" => {
            global_manager()
                .reload(optional_string(args, "page_id"))
                .await
        }
        "browser_close" => {
            global_manager()
                .close(optional_string(args, "page_id"))
                .await
        }
        "browser_evaluate" => {
            global_manager()
                .evaluate(
                    optional_string(args, "page_id"),
                    required_string(args, "script")?,
                )
                .await
        }
        _ => return Err(format!("Unknown browser tool: {name}")),
    };
    result.map_err(format_error)
}

fn format_error(error: BrowserError) -> String {
    error.as_json().to_string()
}

fn required_string<'a>(args: &'a HashMap<String, Value>, name: &str) -> Result<&'a str, String> {
    optional_string(args, name)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format_error(BrowserError::invalid(format!("{name} is required"))))
}

fn optional_string<'a>(args: &'a HashMap<String, Value>, name: &str) -> Option<&'a str> {
    args.get(name).and_then(Value::as_str)
}

fn optional_bool(args: &HashMap<String, Value>, name: &str, default: bool) -> Result<bool, String> {
    match args.get(name) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(format_error(BrowserError::invalid(format!(
            "{name} must be a boolean"
        )))),
    }
}

fn optional_u64(args: &HashMap<String, Value>, name: &str, default: u64) -> Result<u64, String> {
    match args.get(name) {
        None => Ok(default),
        Some(value) => value.as_u64().ok_or_else(|| {
            format_error(BrowserError::invalid(format!(
                "{name} must be a non-negative integer"
            )))
        }),
    }
}

fn optional_usize(
    args: &HashMap<String, Value>,
    name: &str,
    default: usize,
) -> Result<usize, String> {
    optional_u64(args, name, default as u64).map(|value| value as usize)
}

fn optional_i64(args: &HashMap<String, Value>, name: &str, default: i64) -> Result<i64, String> {
    match args.get(name) {
        None => Ok(default),
        Some(value) => value.as_i64().ok_or_else(|| {
            format_error(BrowserError::invalid(format!("{name} must be an integer")))
        }),
    }
}

fn optional_f64(args: &HashMap<String, Value>, name: &str) -> Result<Option<f64>, String> {
    match args.get(name) {
        None => Ok(None),
        Some(value) => value
            .as_f64()
            .map(Some)
            .ok_or_else(|| format_error(BrowserError::invalid(format!("{name} must be a number")))),
    }
}

fn optional_element_id(args: &HashMap<String, Value>) -> Result<Option<u64>, String> {
    match args.get("element_id") {
        None => Ok(None),
        Some(value) if value.is_u64() => Ok(value.as_u64()),
        Some(Value::String(value)) => value.parse::<u64>().map(Some).map_err(|_| {
            format_error(BrowserError::invalid(
                "element_id must be a snapshot integer",
            ))
        }),
        _ => Err(format_error(BrowserError::invalid(
            "element_id must be a snapshot integer",
        ))),
    }
}
