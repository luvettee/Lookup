use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use crate::config::{
    MAX_ACTIVITY_SCOPES, MAX_SIMILAR_WEB_ACTIVITY, MAX_WEB_ACTIVITY, SEARCH_FAILURE_COOLDOWN,
    WEB_ACTIVITY_WINDOW,
};

pub fn normalize_query(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

static TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z0-9]+").unwrap());

fn query_tokens(query: &str) -> HashSet<String> {
    let norm = normalize_query(query);
    TOKEN_RE
        .find_iter(&norm)
        .map(|m| m.as_str().to_string())
        .collect()
}

fn are_queries_similar(left: &str, right: &str) -> bool {
    let a = query_tokens(left);
    let b = query_tokens(right);
    if a.is_empty() || b.is_empty() {
        return left == right;
    }
    let intersection_count = a.intersection(&b).count();
    let union_count = a.union(&b).count();
    let min_count = a.len().min(b.len());

    (intersection_count as f64 / union_count as f64 >= 0.8)
        || (intersection_count as f64 / min_count as f64 >= 0.7)
}

struct SearchGuard {
    activity: HashMap<String, Vec<(Instant, String, String)>>, // scope -> Vec<(time, tool, norm_query)>
    failure_until: HashMap<String, Instant>,
}

impl SearchGuard {
    fn new() -> Self {
        Self {
            activity: HashMap::new(),
            failure_until: HashMap::new(),
        }
    }

    fn before_search(&mut self, scope: &str, tool: &str, query: &str) -> Result<(), String> {
        let now = Instant::now();

        // Clean up expired activities
        self.activity.retain(|_, items| {
            items.retain(|(t, _, _)| now.duration_since(*t) < WEB_ACTIVITY_WINDOW);
            !items.is_empty()
        });

        // Clean up expired failure cool-downs
        self.failure_until.retain(|_, until| *until > now);

        // Enforce max scopes
        if !self.activity.contains_key(scope) && self.activity.len() >= MAX_ACTIVITY_SCOPES {
            if let Some(oldest_scope) = self
                .activity
                .iter()
                .filter_map(|(k, items)| items.last().map(|(t, _, _)| (k.clone(), *t)))
                .min_by_key(|(_, t)| *t)
                .map(|(k, _)| k)
            {
                self.activity.remove(&oldest_scope);
                self.failure_until.remove(&oldest_scope);
            }
        }

        let recent = self.activity.entry(scope.to_string()).or_default();
        recent.retain(|(t, _, _)| now.duration_since(*t) < WEB_ACTIVITY_WINDOW);

        if let Some(until) = self.failure_until.get(scope) {
            if now < *until {
                return Err(
                    "Web search is temporarily unavailable because all providers recently failed. Do not retry immediately."
                        .to_string(),
                );
            }
        }

        let norm = normalize_query(query);
        let similar = recent
            .iter()
            .filter(|(_, _, previous)| are_queries_similar(&norm, previous))
            .count();

        if similar >= MAX_SIMILAR_WEB_ACTIVITY {
            return Err(
                "Similar web searches were already performed recently. Use the results already gathered before searching again."
                    .to_string(),
            );
        }

        if recent.len() >= MAX_WEB_ACTIVITY {
            return Err(
                "Web activity limit reached. Use the results already gathered before calling another search tool."
                    .to_string(),
            );
        }

        recent.push((now, tool.to_string(), norm));
        Ok(())
    }

    fn mark_failure(&mut self, scope: &str) {
        let now = Instant::now();
        self.failure_until
            .insert(scope.to_string(), now + SEARCH_FAILURE_COOLDOWN);
    }
}

static SEARCH_GUARD: LazyLock<Mutex<SearchGuard>> =
    LazyLock::new(|| Mutex::new(SearchGuard::new()));

pub fn check_search_guard(scope: &str, tool: &str, query: &str) -> Result<(), String> {
    let mut lock = SEARCH_GUARD.lock().unwrap();
    lock.before_search(scope, tool, query)
}

pub fn mark_search_guard_failure(scope: &str) {
    let mut lock = SEARCH_GUARD.lock().unwrap();
    lock.mark_failure(scope);
}
