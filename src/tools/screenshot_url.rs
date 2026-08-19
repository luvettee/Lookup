use std::collections::HashMap;

use data_encoding::BASE64;
use serde_json::{json, Value};

use crate::browser::screenshot_png;
use crate::config::{MAX_SCREENSHOT_HEIGHT, MAX_SCREENSHOT_WIDTH};
use crate::net::validate_url;

fn dimension(
    args: &HashMap<String, Value>,
    name: &str,
    default: u32,
    maximum: u32,
) -> Result<u32, String> {
    match args.get(name) {
        None => Ok(default),
        Some(Value::Number(number)) => number
            .as_u64()
            .filter(|value| (320..=u64::from(maximum)).contains(value))
            .map(|value| value as u32)
            .ok_or_else(|| format!("{name} must be an integer from 320 to {maximum}")),
        _ => Err(format!("{name} must be an integer from 320 to {maximum}")),
    }
}

pub async fn screenshot_url(args: &HashMap<String, Value>) -> Result<Value, String> {
    let raw_url = args
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .ok_or_else(|| "url must not be empty".to_string())?;
    let url = validate_url(raw_url, true)?;
    let width = dimension(args, "width", 1280, MAX_SCREENSHOT_WIDTH)?;
    let height = dimension(args, "height", 720, MAX_SCREENSHOT_HEIGHT)?;
    let png = screenshot_png(&url, width, height).await?;

    Ok(json!({
        "url": url,
        "width": width,
        "height": height,
        "mime_type": "image/png",
        "size_bytes": png.len(),
        "_mcp_image": {
            "mime_type": "image/png",
            "data": BASE64.encode(&png)
        }
    }))
}
