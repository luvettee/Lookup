use std::env;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;
use url::Url;

use crate::config::{
    chromium_path, CHROMIUM_TIMEOUT, MAX_CHROMIUM_HTML_BYTES, MAX_SCREENSHOT_BYTES,
};
use crate::net::ssrf::{is_global_ip, resolve_host, validate_url};

fn push_unique(candidates: &mut Vec<PathBuf>, candidate: impl Into<PathBuf>) {
    let candidate = candidate.into();
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn chromium_candidates() -> Vec<PathBuf> {
    if let Some(path) = chromium_path() {
        return vec![PathBuf::from(path)];
    }

    let mut candidates = Vec::new();

    #[cfg(target_os = "macos")]
    {
        let app_paths = [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        ];
        for path in app_paths {
            push_unique(&mut candidates, path);
        }

        if let Some(home) = env::var_os("HOME") {
            let applications = PathBuf::from(home).join("Applications");
            for relative in [
                "Google Chrome.app/Contents/MacOS/Google Chrome",
                "Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
                "Chromium.app/Contents/MacOS/Chromium",
                "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                "Brave Browser.app/Contents/MacOS/Brave Browser",
            ] {
                push_unique(&mut candidates, applications.join(relative));
            }
        }

        for executable in ["google-chrome", "chromium", "microsoft-edge", "brave-browser"] {
            push_unique(&mut candidates, executable);
        }
    }

    #[cfg(target_os = "windows")]
    {
        let installations = [
            ("LOCALAPPDATA", "Google/Chrome/Application/chrome.exe"),
            ("LOCALAPPDATA", "Google/Chrome SxS/Application/chrome.exe"),
            ("LOCALAPPDATA", "Chromium/Application/chrome.exe"),
            ("LOCALAPPDATA", "Microsoft/Edge/Application/msedge.exe"),
            ("LOCALAPPDATA", "BraveSoftware/Brave-Browser/Application/brave.exe"),
            ("PROGRAMFILES", "Google/Chrome/Application/chrome.exe"),
            ("PROGRAMFILES", "Chromium/Application/chrome.exe"),
            ("PROGRAMFILES", "Microsoft/Edge/Application/msedge.exe"),
            ("PROGRAMFILES", "BraveSoftware/Brave-Browser/Application/brave.exe"),
            ("PROGRAMFILES(X86)", "Google/Chrome/Application/chrome.exe"),
            ("PROGRAMFILES(X86)", "Microsoft/Edge/Application/msedge.exe"),
        ];
        for (variable, relative) in installations {
            if let Some(root) = env::var_os(variable) {
                push_unique(&mut candidates, PathBuf::from(root).join(relative));
            }
        }

        for executable in ["chrome.exe", "chromium.exe", "msedge.exe", "brave.exe"] {
            push_unique(&mut candidates, executable);
        }
    }

    #[cfg(target_os = "linux")]
    {
        for executable in [
            "google-chrome-stable",
            "google-chrome",
            "chromium",
            "chromium-browser",
            "microsoft-edge-stable",
            "microsoft-edge",
            "brave-browser",
            "/snap/bin/chromium",
        ] {
            push_unique(&mut candidates, executable);
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        for executable in ["chromium", "chromium-browser", "google-chrome"] {
            push_unique(&mut candidates, executable);
        }
    }

    candidates
}

fn pinned_target(url: &str) -> Result<(String, String), String> {
    let safe_url = validate_url(url, true)?;
    let parsed = Url::parse(&safe_url).map_err(|_| "invalid Chromium URL".to_string())?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "invalid Chromium URL".to_string())?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| "invalid Chromium URL port".to_string())?;
    let ips = resolve_host(host, port)?;
    let ip = ips
        .into_iter()
        .find(is_global_ip)
        .ok_or_else(|| "Chromium target did not resolve to a public address".to_string())?;

    // Pin the validated host and fail every other DNS lookup. This prevents the
    // browser process from following cross-host redirects or loading private hosts.
    let resolver_rules = format!("MAP {} {}, MAP * ~NOTFOUND", host, ip);
    Ok((safe_url, resolver_rules))
}

async fn run_chromium(executable: &Path, url: &str, resolver_rules: &str) -> Result<String, String> {
    let mut child = Command::new(executable)
        .args([
            "--headless=new",
            "--disable-gpu",
            "--disable-extensions",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-sync",
            "--no-first-run",
            "--no-default-browser-check",
            "--incognito",
            &format!("--host-resolver-rules={resolver_rules}"),
            "--dump-dom",
            url,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Chromium executable was not available".to_string())?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Could not capture Chromium output".to_string())?;
    let mut limited = stdout.take((MAX_CHROMIUM_HTML_BYTES + 1) as u64);
    let mut output = Vec::new();

    let read_result = timeout(CHROMIUM_TIMEOUT, limited.read_to_end(&mut output)).await;
    match read_result {
        Ok(Ok(_)) => {}
        Ok(Err(_)) => {
            let _ = child.kill().await;
            return Err("Could not read Chromium output".to_string());
        }
        Err(_) => {
            let _ = child.kill().await;
            return Err("Chromium rendering timed out".to_string());
        }
    }

    if output.len() > MAX_CHROMIUM_HTML_BYTES {
        let _ = child.kill().await;
        return Err("Chromium output is too large".to_string());
    }

    let status = timeout(CHROMIUM_TIMEOUT, child.wait())
        .await
        .map_err(|_| "Chromium rendering timed out".to_string())?
        .map_err(|_| "Chromium process failed".to_string())?;
    if !status.success() {
        return Err("Chromium failed to render the page".to_string());
    }

    String::from_utf8(output).map_err(|_| "Chromium returned invalid UTF-8".to_string())
}

pub async fn render_html(url: &str) -> Result<String, String> {
    let (safe_url, resolver_rules) = pinned_target(url)?;
    let mut last_error = "Chromium executable was not available".to_string();

    for executable in chromium_candidates() {
        match run_chromium(&executable, &safe_url, &resolver_rules).await {
            Ok(html) => return Ok(html),
            Err(error) => last_error = error,
        }
    }

    Err(last_error)
}

async fn run_screenshot(
    executable: &Path,
    url: &str,
    resolver_rules: &str,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    let temp_dir = TempDir::new().map_err(|_| "Could not create screenshot directory".to_string())?;
    let screenshot_path = temp_dir.path().join("screenshot.png");
    let screenshot_arg = format!("--screenshot={}", screenshot_path.display());
    let window_arg = format!("--window-size={width},{height}");
    let resolver_arg = format!("--host-resolver-rules={resolver_rules}");

    let mut child = Command::new(executable)
        .args([
            "--headless=new",
            "--disable-gpu",
            "--disable-extensions",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-sync",
            "--no-first-run",
            "--no-default-browser-check",
            "--incognito",
            "--hide-scrollbars",
            "--virtual-time-budget=5000",
        ])
        .arg(resolver_arg)
        .arg(window_arg)
        .arg(screenshot_arg)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Chromium executable was not available".to_string())?;

    let status = match timeout(CHROMIUM_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(_)) => return Err("Chromium screenshot process failed".to_string()),
        Err(_) => {
            let _ = child.kill().await;
            return Err("Chromium screenshot timed out".to_string());
        }
    };
    if !status.success() {
        return Err("Chromium failed to capture the page".to_string());
    }

    let metadata = std::fs::metadata(&screenshot_path)
        .map_err(|_| "Chromium did not produce a screenshot".to_string())?;
    if metadata.len() == 0 || metadata.len() > MAX_SCREENSHOT_BYTES {
        return Err("Chromium screenshot is empty or too large".to_string());
    }

    let png = std::fs::read(&screenshot_path)
        .map_err(|_| "Could not read Chromium screenshot".to_string())?;
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("Chromium produced an invalid PNG screenshot".to_string());
    }
    Ok(png)
}

pub async fn screenshot_png(url: &str, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let (safe_url, resolver_rules) = pinned_target(url)?;
    let mut last_error = "Chromium executable was not available".to_string();

    for executable in chromium_candidates() {
        match run_screenshot(
            &executable,
            &safe_url,
            &resolver_rules,
            width,
            height,
        )
        .await
        {
            Ok(png) => return Ok(png),
            Err(error) => last_error = error,
        }
    }

    Err(last_error)
}
