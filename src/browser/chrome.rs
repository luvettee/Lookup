use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;

use serde::Deserialize;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::time::sleep;
use tracing::{debug, info, warn};
use url::Url;

use crate::config::BrowserConfig;

use super::types::{BrowserError, BrowserResult};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeTarget {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(rename = "type", default)]
    pub target_type: String,
    pub web_socket_debugger_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionInfo {
    web_socket_debugger_url: String,
}

pub struct ChromeProcess {
    pub base_url: String,
    child: Option<Child>,
    _profile: Option<TempDir>,
}

impl ChromeProcess {
    pub async fn connect_or_launch(config: &BrowserConfig) -> BrowserResult<Self> {
        if let Some(debug_url) = &config.debug_url {
            let base_url = validate_debug_url(debug_url)?;
            wait_until_ready(&base_url, config.startup_timeout).await?;
            info!(url = %base_url, "browser connected");
            return Ok(Self {
                base_url,
                child: None,
                _profile: None,
            });
        }

        let executable = detect_browser(config.path.as_deref()).ok_or_else(|| {
            BrowserError::new(
                "browser_not_found",
                "No supported local Chromium browser was found; set LOOKUP_BROWSER_PATH",
            )
        })?;
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .map_err(|error| BrowserError::new("browser_launch_failed", error.to_string()))?;
        let port = listener
            .local_addr()
            .map_err(|error| BrowserError::new("browser_launch_failed", error.to_string()))?
            .port();
        drop(listener);

        let profile = TempDir::new().map_err(|error| {
            BrowserError::new(
                "browser_launch_failed",
                format!("Could not create profile: {error}"),
            )
        })?;
        let mut command = Command::new(&executable);
        command
            .arg(format!("--remote-debugging-port={port}"))
            .arg("--remote-debugging-address=127.0.0.1")
            .arg(format!("--user-data-dir={}", profile.path().display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-sync")
            .arg("--disable-background-networking")
            .arg("--disable-component-update")
            .arg("--disable-default-apps")
            .arg("--disable-extensions")
            .arg("--disable-features=Translate")
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if config.headless {
            command.arg("--headless=new").arg("--disable-gpu");
        }
        #[cfg(target_os = "linux")]
        command.arg("--disable-dev-shm-usage");

        let mut child = command.spawn().map_err(|error| {
            BrowserError::new(
                "browser_launch_failed",
                format!("Could not launch {}: {error}", executable.display()),
            )
        })?;
        let base_url = format!("http://127.0.0.1:{port}");
        if let Err(error) = wait_until_ready(&base_url, config.startup_timeout).await {
            let _ = child.kill().await;
            return Err(error);
        }
        info!(path = %executable.display(), port, headless = config.headless, "browser launched");
        Ok(Self {
            base_url,
            child: Some(child),
            _profile: Some(profile),
        })
    }

    pub async fn browser_websocket(&self) -> BrowserResult<String> {
        let info: VersionInfo = request_json(
            reqwest::Method::GET,
            &format!("{}/json/version", self.base_url),
        )
        .await?;
        ensure_local_websocket(&info.web_socket_debugger_url)?;
        Ok(info.web_socket_debugger_url)
    }

    pub async fn targets(&self) -> BrowserResult<Vec<ChromeTarget>> {
        let targets: Vec<ChromeTarget> = request_json(
            reqwest::Method::GET,
            &format!("{}/json/list", self.base_url),
        )
        .await?;
        Ok(targets
            .into_iter()
            .filter(|target| target.target_type == "page")
            .collect())
    }

    pub async fn new_target(&self, url: &str) -> BrowserResult<ChromeTarget> {
        let encoded: String = url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
        let endpoint = format!("{}/json/new?{}", self.base_url, encoded);
        let target: ChromeTarget = request_json(reqwest::Method::PUT, &endpoint).await?;
        if let Some(websocket) = &target.web_socket_debugger_url {
            ensure_local_websocket(websocket)?;
        }
        Ok(target)
    }

    pub async fn close_target(&self, target_id: &str) -> BrowserResult<()> {
        let endpoint = format!("{}/json/close/{}", self.base_url, target_id);
        let response =
            local_client().get(endpoint).send().await.map_err(|_| {
                BrowserError::new("browser_disconnected", "Browser did not respond")
            })?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(BrowserError::new(
                "page_not_found",
                "Browser tab was not found",
            ))
        }
    }

    pub async fn is_alive(&mut self) -> bool {
        match &mut self.child {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => true,
        }
    }

    pub async fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Err(error) = child.kill().await {
                debug!(%error, "browser process was already stopped");
            }
            let _ = child.wait().await;
            info!("browser shutdown complete");
        }
    }
}

fn local_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

async fn request_json<T: serde::de::DeserializeOwned>(
    method: reqwest::Method,
    endpoint: &str,
) -> BrowserResult<T> {
    let response = local_client()
        .request(method, endpoint)
        .send()
        .await
        .map_err(|_| BrowserError::new("browser_disconnected", "Browser did not respond"))?;
    if !response.status().is_success() {
        return Err(BrowserError::new(
            "browser_disconnected",
            format!("Browser returned HTTP {}", response.status()),
        ));
    }
    response.json::<T>().await.map_err(|_| {
        BrowserError::new(
            "browser_disconnected",
            "Browser returned invalid CDP metadata",
        )
    })
}

async fn wait_until_ready(base_url: &str, timeout: std::time::Duration) -> BrowserResult<()> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        let endpoint = format!("{base_url}/json/version");
        if local_client().get(endpoint).send().await.is_ok() {
            return Ok(());
        }
        sleep(std::time::Duration::from_millis(75)).await;
    }
    Err(BrowserError::new(
        "browser_launch_failed",
        "Chromium did not expose its local debugging endpoint before the startup timeout",
    ))
}

fn validate_debug_url(raw: &str) -> BrowserResult<String> {
    let parsed = Url::parse(raw).map_err(|_| {
        BrowserError::new(
            "invalid_configuration",
            "LOOKUP_BROWSER_DEBUG_URL is invalid",
        )
    })?;
    let host = parsed.host_str().unwrap_or_default();
    if parsed.scheme() != "http" || !matches!(host, "127.0.0.1" | "localhost" | "::1") {
        return Err(BrowserError::new(
            "invalid_configuration",
            "LOOKUP_BROWSER_DEBUG_URL must be an http:// localhost address",
        ));
    }
    Ok(raw.trim_end_matches('/').to_string())
}

fn ensure_local_websocket(raw: &str) -> BrowserResult<()> {
    let parsed = Url::parse(raw).map_err(|_| {
        BrowserError::new(
            "browser_disconnected",
            "Browser returned an invalid WebSocket URL",
        )
    })?;
    let host = parsed.host_str().unwrap_or_default();
    if !matches!(host, "127.0.0.1" | "localhost" | "::1") {
        return Err(BrowserError::new(
            "browser_disconnected",
            "Refusing a non-local Chrome DevTools endpoint",
        ));
    }
    Ok(())
}

pub fn detect_browser(configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = configured {
        return Some(path.to_path_buf());
    }
    if let Some(legacy) = crate::config::chromium_path() {
        return Some(PathBuf::from(legacy));
    }

    for candidate in browser_candidates() {
        if candidate.components().count() > 1 && candidate.is_file() {
            return Some(candidate);
        }
        if candidate.components().count() == 1 && command_exists(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn command_exists(command: &Path) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&paths).any(|directory| {
        let path = directory.join(command);
        if path.is_file() {
            return true;
        }
        #[cfg(target_os = "windows")]
        if path.with_extension("exe").is_file() {
            return true;
        }
        false
    })
}

fn browser_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(target_os = "macos")]
    for path in [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ] {
        candidates.push(PathBuf::from(path));
    }
    #[cfg(target_os = "linux")]
    for name in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "brave-browser",
        "microsoft-edge",
        "microsoft-edge-stable",
    ] {
        candidates.push(PathBuf::from(name));
    }
    #[cfg(target_os = "windows")]
    {
        for (variable, relative) in [
            ("LOCALAPPDATA", "Google/Chrome/Application/chrome.exe"),
            ("LOCALAPPDATA", "Google/Chrome SxS/Application/chrome.exe"),
            ("LOCALAPPDATA", "Chromium/Application/chrome.exe"),
            (
                "LOCALAPPDATA",
                "BraveSoftware/Brave-Browser/Application/brave.exe",
            ),
            ("LOCALAPPDATA", "Microsoft/Edge/Application/msedge.exe"),
            ("PROGRAMFILES", "Google/Chrome/Application/chrome.exe"),
            (
                "PROGRAMFILES",
                "BraveSoftware/Brave-Browser/Application/brave.exe",
            ),
            ("PROGRAMFILES", "Microsoft/Edge/Application/msedge.exe"),
            ("PROGRAMFILES(X86)", "Google/Chrome/Application/chrome.exe"),
            ("PROGRAMFILES(X86)", "Microsoft/Edge/Application/msedge.exe"),
        ] {
            if let Some(root) = env::var_os(variable) {
                candidates.push(PathBuf::from(root).join(relative));
            }
        }
        candidates
            .extend(["chrome.exe", "chromium.exe", "brave.exe", "msedge.exe"].map(PathBuf::from));
    }
    if candidates.is_empty() {
        warn!("browser auto-detection is not defined for this platform");
        candidates.extend(["chromium", "google-chrome"].map(PathBuf::from));
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_remote_debug_url() {
        assert!(validate_debug_url("http://0.0.0.0:9222").is_err());
        assert!(validate_debug_url("http://192.168.1.4:9222").is_err());
        assert!(validate_debug_url("http://127.0.0.1:9222").is_ok());
    }
}
