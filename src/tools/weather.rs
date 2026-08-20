use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use url::form_urlencoded;

use crate::cache::{cache_get, cache_put};
use crate::config::weather_code_description;
use crate::net::get_json;

pub async fn geocode(location: &str) -> Result<Value, String> {
    let clean_loc = location
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let cache_key = format!("geocode:{}", clean_loc);
    if let Some(cached) = cache_get(&cache_key) {
        return Ok(cached);
    }

    let encoded_name = form_urlencoded::byte_serialize(location.as_bytes()).collect::<String>();
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
        encoded_name
    );

    let data = get_json(&url, None, None, None).await?;
    let results = data.get("results").and_then(|v| v.as_array());

    if let Some(arr) = results {
        if let Some(first) = arr.first() {
            cache_put(
                &cache_key,
                Duration::from_secs(30 * 24 * 60 * 60),
                first.clone(),
            );
            return Ok(first.clone());
        }
    }

    Err(format!("Location not found: {}", location))
}

pub async fn weather(args: &HashMap<String, Value>) -> Result<Value, String> {
    let location = match args.get("location") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim(),
        _ => return Err("location must not be empty".to_string()),
    };

    let days = match args.get("days") {
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                if !(1..=7).contains(&i) {
                    return Err("days must be an integer from 1 to 7".to_string());
                }
                i as usize
            } else if let Some(f) = n.as_f64() {
                if f.fract() != 0.0 || !(1.0..=7.0).contains(&f) {
                    return Err("days must be an integer from 1 to 7".to_string());
                }
                f as usize
            } else {
                return Err("days must be an integer from 1 to 7".to_string());
            }
        }
        Some(Value::Bool(_)) => return Err("days must be an integer from 1 to 7".to_string()),
        None => 3,
        _ => return Err("days must be an integer from 1 to 7".to_string()),
    };

    let clean_loc = location
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let cache_key = format!("weather:{}:{}", clean_loc, days);
    if let Some(cached) = cache_get(&cache_key) {
        return Ok(cached);
    }

    let place = geocode(location).await?;
    let lat = place
        .get("latitude")
        .and_then(|v| v.as_f64())
        .ok_or("Missing latitude")?;
    let lon = place
        .get("longitude")
        .and_then(|v| v.as_f64())
        .ok_or("Missing longitude")?;

    let place_name = place.get("name").and_then(|v| v.as_str());
    let admin1 = place.get("admin1").and_then(|v| v.as_str());
    let country = place.get("country").and_then(|v| v.as_str());

    let loc_parts: Vec<&str> = [place_name, admin1, country]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect();
    let formatted_loc = loc_parts.join(", ");

    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&timezone=auto&forecast_days={}&current=temperature_2m,apparent_temperature,relative_humidity_2m,precipitation,weather_code,wind_speed_10m&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max",
        lat, lon, days
    );

    let data = get_json(&url, None, None, None).await?;
    let current = data.get("current").ok_or("Missing current weather")?;
    let daily = data.get("daily").ok_or("Missing daily forecast")?;
    let tz = data
        .get("timezone")
        .and_then(|v| v.as_str())
        .unwrap_or("UTC");

    let current_weather_code = current
        .get("weather_code")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    let current_condition = weather_code_description(current_weather_code);

    let daily_times = daily
        .get("time")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let daily_codes = daily
        .get("weather_code")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let daily_max = daily
        .get("temperature_2m_max")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let daily_min = daily
        .get("temperature_2m_min")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let daily_rain = daily
        .get("precipitation_probability_max")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut forecast = Vec::new();
    for i in 0..days {
        let date_str = daily_times.get(i).and_then(|v| v.as_str()).unwrap_or("");
        let code = daily_codes.get(i).and_then(|v| v.as_i64()).unwrap_or(-1);
        let cond = weather_code_description(code);
        let high = daily_max.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let low = daily_min.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let rain = daily_rain.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0);

        forecast.push(json!({
            "date": date_str,
            "condition": cond,
            "high_c": high,
            "low_c": low,
            "rain_chance_percent": rain
        }));
    }

    let result = json!({
        "location": formatted_loc,
        "timezone": tz,
        "current": {
            "temperature_c": current.get("temperature_2m").and_then(|v| v.as_f64()).unwrap_or(0.0),
            "feels_like_c": current.get("apparent_temperature").and_then(|v| v.as_f64()).unwrap_or(0.0),
            "humidity_percent": current.get("relative_humidity_2m").and_then(|v| v.as_f64()).unwrap_or(0.0),
            "wind_kmh": current.get("wind_speed_10m").and_then(|v| v.as_f64()).unwrap_or(0.0),
            "precipitation_mm": current.get("precipitation").and_then(|v| v.as_f64()).unwrap_or(0.0),
            "condition": current_condition
        },
        "forecast": forecast
    });

    cache_put(&cache_key, Duration::from_secs(600), result.clone());
    Ok(result)
}
