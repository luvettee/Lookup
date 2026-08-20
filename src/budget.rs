use serde_json::{json, Value};

use crate::config::MAX_TOOL_OUTPUT_CHARS;

pub fn serialized_chars(val: &Value) -> usize {
    serde_json::to_string(val).map(|s| s.len()).unwrap_or(0)
}

#[derive(Clone, Debug)]
enum PathStep {
    Key(String),
    Index(usize),
}

fn find_largest_string_path(
    val: &Value,
    key_name: &str,
    current_path: Vec<PathStep>,
    minimum: usize,
    best: &mut Option<(Vec<PathStep>, usize)>,
) {
    match val {
        Value::Object(map) => {
            for (k, v) in map {
                let mut next_path = current_path.clone();
                next_path.push(PathStep::Key(k.clone()));
                if k == key_name {
                    if let Value::String(s) = v {
                        if s.len() > minimum {
                            if let Some((_, best_len)) = best {
                                if s.len() > *best_len {
                                    *best = Some((next_path.clone(), s.len()));
                                }
                            } else {
                                *best = Some((next_path.clone(), s.len()));
                            }
                        }
                    }
                }
                find_largest_string_path(v, key_name, next_path, minimum, best);
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let mut next_path = current_path.clone();
                next_path.push(PathStep::Index(i));
                find_largest_string_path(v, key_name, next_path, minimum, best);
            }
        }
        _ => {}
    }
}

fn get_string_mut<'a>(val: &'a mut Value, path: &[PathStep]) -> Option<&'a mut String> {
    if path.is_empty() {
        return match val {
            Value::String(s) => Some(s),
            _ => None,
        };
    }

    match &path[0] {
        PathStep::Key(k) => match val {
            Value::Object(map) => {
                let child = map.get_mut(k)?;
                get_string_mut(child, &path[1..])
            }
            _ => None,
        },
        PathStep::Index(i) => match val {
            Value::Array(arr) => {
                let child = arr.get_mut(*i)?;
                get_string_mut(child, &path[1..])
            }
            _ => None,
        },
    }
}

fn shrink_field(payload: &mut Value, key: &str, minimum: usize, target: usize) -> bool {
    let mut best = None;
    find_largest_string_path(payload, key, Vec::new(), minimum, &mut best);

    let (path, _) = match best {
        Some(p) => p,
        None => return false,
    };

    let curr_serialized = serialized_chars(payload);
    let overflow = curr_serialized.saturating_sub(target).max(1);

    if let Some(s) = get_string_mut(payload, &path) {
        let old_len = s.len();
        let new_size = (old_len.saturating_sub(overflow + 24)).max(minimum);
        let marker = " [truncated]";

        if new_size > marker.len() {
            let keep = new_size - marker.len();
            let mut cut_idx = keep.min(s.len());
            while !s.is_char_boundary(cut_idx) && cut_idx > 0 {
                cut_idx -= 1;
            }
            let truncated_str = format!("{}{}", s[..cut_idx].trim_end(), marker);
            *s = truncated_str;
        } else {
            *s = String::new();
        }
        true
    } else {
        false
    }
}

fn prune_last_list_item(val: &mut Value) -> bool {
    match val {
        Value::Object(map) => {
            for key in &["links", "results", "sources"] {
                if let Some(Value::Array(arr)) = map.get_mut(*key) {
                    if arr.len() > 1 {
                        arr.pop();
                        return true;
                    }
                }
            }
            for (_, v) in map.iter_mut() {
                if prune_last_list_item(v) {
                    return true;
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                if prune_last_list_item(item) {
                    return true;
                }
            }
        }
        _ => {}
    }
    false
}

pub fn enforce_output_budget(mut payload: Value, max_chars: Option<usize>) -> Value {
    let budget = max_chars.unwrap_or(MAX_TOOL_OUTPUT_CHARS);
    if serialized_chars(&payload) <= budget {
        return payload;
    }

    // Step 1: Shrink large metadata and bulk content
    let step1_fields = [
        ("description", 0),
        ("content", 256),
        ("snippet", 120),
        ("error", 80),
        ("text", 40),
    ];
    for (key, minimum) in step1_fields {
        while serialized_chars(&payload) > budget
            && shrink_field(&mut payload, key, minimum, budget)
        {}
    }

    // Step 2: Prune list items from results/sources/links
    while serialized_chars(&payload) > budget && prune_last_list_item(&mut payload) {}

    // Step 3: Shrink remaining fields further if still over budget
    let step3_fields = [
        ("content", 0),
        ("snippet", 0),
        ("title", 40),
        ("url", 80),
        ("query", 40),
    ];
    for (key, minimum) in step3_fields {
        while serialized_chars(&payload) > budget
            && shrink_field(&mut payload, key, minimum, budget)
        {}
    }

    if serialized_chars(&payload) > budget {
        return json!({"error": "Tool output exceeded the configured character budget"});
    }

    payload
}
