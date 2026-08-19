use std::collections::HashMap;
use std::env;
use std::str::FromStr;
use chrono::{Local, Utc};
use chrono_tz::Tz;
use serde_json::{json, Value};

pub fn current_time(args: &HashMap<String, Value>) -> Result<Value, String> {
    let zone_arg = args
        .get("timezone")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let (zone_name, iso, date_str, time_str) = match zone_arg {
        None | Some("local") | Some("LOCAL") | Some("Local") => {
            let now = Local::now();
            let name = env::var("TZ").unwrap_or_else(|_| "local".to_string());
            (
                name,
                now.to_rfc3339(),
                now.format("%A, %B %-d, %Y").to_string(),
                now.format("%-I:%M %p").to_string(),
            )
        }
        Some(zone_str) => {
            if let Ok(tz) = Tz::from_str(zone_str) {
                let now = Utc::now().with_timezone(&tz);
                (
                    zone_str.to_string(),
                    now.to_rfc3339(),
                    now.format("%A, %B %-d, %Y").to_string(),
                    now.format("%-I:%M %p").to_string(),
                )
            } else {
                return Err(format!("Unknown timezone: {}", zone_str));
            }
        }
    };

    Ok(json!({
        "timezone": zone_name,
        "iso": iso,
        "date": date_str,
        "time": time_str
    }))
}
