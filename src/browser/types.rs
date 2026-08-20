use std::fmt;

use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct PageId(String);

impl PageId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PageInfo {
    pub page_id: PageId,
    pub title: String,
    pub url: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotMode {
    Compact,
    Interactive,
    Full,
}

impl SnapshotMode {
    pub fn parse(value: Option<&str>) -> Result<Self, BrowserError> {
        match value.unwrap_or("interactive") {
            "compact" => Ok(Self::Compact),
            "interactive" => Ok(Self::Interactive),
            "full" => Ok(Self::Full),
            _ => Err(BrowserError::new(
                "invalid_argument",
                "mode must be compact, interactive, or full",
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Interactive => "interactive",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BrowserError {
    pub code: &'static str,
    pub message: String,
}

impl BrowserError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_argument", message)
    }

    pub fn as_json(&self) -> Value {
        json!({ "code": self.code, "message": self.message })
    }
}

impl fmt::Display for BrowserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BrowserError {}

pub type BrowserResult<T> = Result<T, BrowserError>;
