use std::collections::HashMap;
use std::env;
use std::str::FromStr;
use chrono::{Local, Utc};
use chrono_tz::Tz;
use serde_json::{json, Value};

fn local_tz_name() -> String {
    env::var("TZ").unwrap_or_else(|_| "UTC".to_string())
}

pub fn current_time(args: &HashMap<String, Value>) -> Result<Value, String> {
    let zone_str = args
        .get("timezone")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(local_tz_name);

    let (iso, date_str, time_str) = if zone_str.eq_ignore_ascii_case("local") {
        let now = Local::now();
        (
            now.to_rfc3339(),
            now.format("%A, %B %-d, %Y").to_string(),
            now.format("%-I:%M %p").to_string(),
        )
    } else if let Ok(tz) = Tz::from_str(&zone_str) {
        let now = Utc::now().with_timezone(&tz);
        (
            now.to_rfc3339(),
            now.format("%A, %B %-d, %Y").to_string(),
            now.format("%-I:%M %p").to_string(),
        )
    } else {
        return Err(format!("Unknown timezone: {}", zone_str));
    };

    Ok(json!({
        "timezone": zone_str,
        "iso": iso,
        "date": date_str,
        "time": time_str
    }))
}
