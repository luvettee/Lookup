use std::env;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;

use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use url::Url;

use crate::config::{
    chromium_path, CHROMIUM_TIMEOUT, MAX_CHROMIUM_HTML_BYTES, MAX_SCREENSHOT_BYTES,
};
use crate::net::ssrf::{is_global_ip, resolve_host, validate_url};

const FULL_PAGE_HEIGHT: u32 = 12_000;

const MAX_STDERR_BYTES: usize = 4096;

static WORKING_CHROMIUM: Mutex<Option<PathBuf>> = Mutex::new(None);

fn cached_chromium() -> Option<PathBuf> {
    WORKING_CHROMIUM.lock().ok().and_then(|guard| guard.clone())
}

fn set_cached_chromium(path: PathBuf) {
    if let Ok(mut guard) = WORKING_CHROMIUM.lock() {
        *guard = Some(path);
    }
}

fn clear_cached_chromium() {
    if let Ok(mut guard) = WORKING_CHROMIUM.lock() {
        *guard = None;
    }
}

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

    if let Some(cached) = cached_chromium() {
        push_unique(&mut candidates, cached);
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

impl ImageFormat {
    fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
        }
    }

    fn magic_bytes_ok(self, data: &[u8]) -> bool {
        match self {
            ImageFormat::Png => data.starts_with(b"\x89PNG\r\n\x1a\n"),
            ImageFormat::Jpeg => data.starts_with(&[0xFF, 0xD8, 0xFF]),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub timeout: Duration,
    pub wait_after_load: Duration,
    pub user_agent: Option<String>,
    pub viewport: (u32, u32),
    pub device_scale_factor: f32,
    pub extra_args: Vec<String>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            timeout: CHROMIUM_TIMEOUT,
            wait_after_load: Duration::from_millis(1500),
            user_agent: None,
            viewport: (1280, 800),
            device_scale_factor: 1.0,
            extra_args: Vec::new(),
        }
    }
}

impl RenderOptions {
    pub fn mobile() -> Self {
        Self {
            viewport: (390, 844),
            device_scale_factor: 3.0,
            user_agent: Some(
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1"
                    .to_string(),
            ),
            ..Self::default()
        }
    }

    fn common_args(&self) -> Vec<String> {
        let mut args = vec![
            "--headless=new".to_string(),
            "--disable-gpu".to_string(),
            "--disable-extensions".to_string(),
            "--disable-background-networking".to_string(),
            "--disable-component-update".to_string(),
            "--disable-sync".to_string(),
            "--no-first-run".to_string(),
            "--no-default-browser-check".to_string(),
            "--incognito".to_string(),
            format!(
                "--virtual-time-budget={}",
                self.wait_after_load.as_millis().max(1)
            ),
            format!("--force-device-scale-factor={}", self.device_scale_factor),
        ];
        if let Some(ua) = &self.user_agent {
            args.push(format!("--user-agent={ua}"));
        }
        args.extend(self.extra_args.iter().cloned());
        args
    }
}

#[derive(Debug, Clone)]
pub struct ScreenshotOptions {
    pub render: RenderOptions,
    pub format: ImageFormat,
    pub full_page: bool,
}

impl Default for ScreenshotOptions {
    fn default() -> Self {
        Self {
            render: RenderOptions::default(),
            format: ImageFormat::Png,
            full_page: false,
        }
    }
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

    let resolver_rules = format!("MAP {} {}, MAP * ~NOTFOUND", host, ip);
    Ok((safe_url, resolver_rules))
}

async fn read_capped<R: tokio::io::AsyncRead + Unpin>(
    reader: R,
    max_bytes: usize,
) -> std::io::Result<Vec<u8>> {
    let mut limited = reader.take((max_bytes + 1) as u64);
    let mut buf = Vec::new();
    limited.read_to_end(&mut buf).await?;
    Ok(buf)
}

fn stderr_tail(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).trim().to_string()
}

fn with_context(base: &str, stderr: &[u8]) -> String {
    let tail = stderr_tail(stderr);
    if tail.is_empty() {
        base.to_string()
    } else {
        format!("{base}: {tail}")
    }
}

async fn run_chromium(
    executable: &Path,
    url: &str,
    resolver_rules: &str,
    options: &RenderOptions,
) -> Result<String, String> {
    let mut child = Command::new(executable)
        .args(options.common_args())
        .arg(format!("--window-size={},{}", options.viewport.0, options.viewport.1))
        .arg(format!("--host-resolver-rules={resolver_rules}"))
        .arg("--dump-dom")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Chromium executable was not available".to_string())?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Could not capture Chromium output".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Could not capture Chromium diagnostics".to_string())?;

    let read = timeout(options.timeout, async {
        tokio::join!(
            read_capped(stdout, MAX_CHROMIUM_HTML_BYTES),
            read_capped(stderr, MAX_STDERR_BYTES),
        )
    })
    .await;

    let (out_result, err_result) = match read {
        Ok(results) => results,
        Err(_) => {
            let _ = child.kill().await;
            return Err("Chromium rendering timed out".to_string());
        }
    };

    let output = out_result.map_err(|_| "Could not read Chromium output".to_string())?;
    let stderr_bytes = err_result.unwrap_or_default();

    if output.len() > MAX_CHROMIUM_HTML_BYTES {
        let _ = child.kill().await;
        return Err("Chromium output is too large".to_string());
    }

    let status = timeout(options.timeout, child.wait())
        .await
        .map_err(|_| "Chromium rendering timed out".to_string())?
        .map_err(|_| "Chromium process failed".to_string())?;
    if !status.success() {
        return Err(with_context("Chromium failed to render the page", &stderr_bytes));
    }

    String::from_utf8(output).map_err(|_| "Chromium returned invalid UTF-8".to_string())
}

pub async fn render_html(url: &str) -> Result<String, String> {
    render_html_with_options(url, &RenderOptions::default()).await
}

pub async fn render_html_with_options(url: &str, options: &RenderOptions) -> Result<String, String> {
    let (safe_url, resolver_rules) = pinned_target(url)?;
    let mut last_error = "Chromium executable was not available".to_string();

    for executable in chromium_candidates() {
        match run_chromium(&executable, &safe_url, &resolver_rules, options).await {
            Ok(html) => {
                set_cached_chromium(executable);
                return Ok(html);
            }
            Err(error) => {
                clear_cached_chromium();
                last_error = error;
            }
        }
    }

    Err(last_error)
}

pub async fn extract_text(url: &str, options: &RenderOptions) -> Result<String, String> {
    let html = render_html_with_options(url, options).await?;
    Ok(html_to_text(&html))
}

fn html_to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut skip_depth = 0usize;
    let mut chars = html.char_indices().peekable();
    let bytes = html.as_bytes();

    while let Some((i, c)) = chars.next() {
        if c == '<' {
            in_tag = true;
            let rest = &html[i..];
            let lower_start = rest.to_ascii_lowercase();
            if lower_start.starts_with("<script") || lower_start.starts_with("<style") {
                skip_depth += 1;
            } else if lower_start.starts_with("</script") || lower_start.starts_with("</style") {
                skip_depth = skip_depth.saturating_sub(1);
            }
            continue;
        }
        if c == '>' {
            in_tag = false;
            continue;
        }
        if in_tag {
            continue;
        }
        if skip_depth > 0 {
            continue;
        }
        text.push(c);
        let _ = bytes;
    }

    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

async fn run_screenshot(
    executable: &Path,
    url: &str,
    resolver_rules: &str,
    options: &ScreenshotOptions,
) -> Result<Vec<u8>, String> {
    let temp_dir = TempDir::new().map_err(|_| "Could not create screenshot directory".to_string())?;
    let screenshot_path = temp_dir
        .path()
        .join(format!("screenshot.{}", options.format.extension()));

    let (width, height) = if options.full_page {
        (options.render.viewport.0, FULL_PAGE_HEIGHT)
    } else {
        options.render.viewport
    };

    let mut child = Command::new(executable)
        .args(options.render.common_args())
        .arg("--hide-scrollbars")
        .arg(format!("--window-size={width},{height}"))
        .arg(format!("--host-resolver-rules={resolver_rules}"))
        .arg(format!("--screenshot={}", screenshot_path.display()))
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Chromium executable was not available".to_string())?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Could not capture Chromium diagnostics".to_string())?;

    let wait = timeout(options.render.timeout, async {
        let stderr_bytes = read_capped(stderr, MAX_STDERR_BYTES).await.unwrap_or_default();
        let status = child.wait().await;
        (status, stderr_bytes)
    })
    .await;

    let (status_result, stderr_bytes) = match wait {
        Ok(result) => result,
        Err(_) => {
            let _ = child.kill().await;
            return Err("Chromium screenshot timed out".to_string());
        }
    };
    let status = status_result.map_err(|_| "Chromium screenshot process failed".to_string())?;
    if !status.success() {
        return Err(with_context("Chromium failed to capture the page", &stderr_bytes));
    }

    let metadata = std::fs::metadata(&screenshot_path)
        .map_err(|_| "Chromium did not produce a screenshot".to_string())?;
    if metadata.len() == 0 || metadata.len() > MAX_SCREENSHOT_BYTES {
        return Err("Chromium screenshot is empty or too large".to_string());
    }

    let image = std::fs::read(&screenshot_path)
        .map_err(|_| "Could not read Chromium screenshot".to_string())?;
    if !options.format.magic_bytes_ok(&image) {
        return Err("Chromium produced an unexpected image format".to_string());
    }
    Ok(image)
}

pub async fn screenshot_png(url: &str, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let options = ScreenshotOptions {
        render: RenderOptions {
            viewport: (width, height),
            ..RenderOptions::default()
        },
        ..ScreenshotOptions::default()
    };
    screenshot_with_options(url, &options).await
}

pub async fn screenshot_with_options(url: &str, options: &ScreenshotOptions) -> Result<Vec<u8>, String> {
    let (safe_url, resolver_rules) = pinned_target(url)?;
    let mut last_error = "Chromium executable was not available".to_string();

    for executable in chromium_candidates() {
        match run_screenshot(&executable, &safe_url, &resolver_rules, options).await {
            Ok(png) => {
                set_cached_chromium(executable);
                return Ok(png);
            }
            Err(error) => {
                clear_cached_chromium();
                last_error = error;
            }
        }
    }

    #[cfg(target_os = "macos")]
    if let Ok(png) = run_webkit_screenshot(&safe_url, options).await {
        return Ok(png);
    }

    Err(last_error)
}

pub async fn render_pdf(url: &str, options: &RenderOptions) -> Result<Vec<u8>, String> {
    let (safe_url, resolver_rules) = pinned_target(url)?;
    let mut last_error = "Chromium executable was not available".to_string();

    for executable in chromium_candidates() {
        match run_pdf(&executable, &safe_url, &resolver_rules, options).await {
            Ok(pdf) => {
                set_cached_chromium(executable);
                return Ok(pdf);
            }
            Err(error) => {
                clear_cached_chromium();
                last_error = error;
            }
        }
    }

    Err(last_error)
}

async fn run_pdf(
    executable: &Path,
    url: &str,
    resolver_rules: &str,
    options: &RenderOptions,
) -> Result<Vec<u8>, String> {
    let temp_dir = TempDir::new().map_err(|_| "Could not create PDF directory".to_string())?;
    let pdf_path = temp_dir.path().join("output.pdf");

    let mut child = Command::new(executable)
        .args(options.common_args())
        .arg(format!("--window-size={},{}", options.viewport.0, options.viewport.1))
        .arg(format!("--host-resolver-rules={resolver_rules}"))
        .arg(format!("--print-to-pdf={}", pdf_path.display()))
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Chromium executable was not available".to_string())?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Could not capture Chromium diagnostics".to_string())?;

    let wait = timeout(options.timeout, async {
        let stderr_bytes = read_capped(stderr, MAX_STDERR_BYTES).await.unwrap_or_default();
        let status = child.wait().await;
        (status, stderr_bytes)
    })
    .await;

    let (status_result, stderr_bytes) = match wait {
        Ok(result) => result,
        Err(_) => {
            let _ = child.kill().await;
            return Err("Chromium PDF export timed out".to_string());
        }
    };
    let status = status_result.map_err(|_| "Chromium PDF process failed".to_string())?;
    if !status.success() {
        return Err(with_context("Chromium failed to export the page as PDF", &stderr_bytes));
    }

    let metadata = std::fs::metadata(&pdf_path)
        .map_err(|_| "Chromium did not produce a PDF".to_string())?;
    if metadata.len() == 0 || metadata.len() > MAX_SCREENSHOT_BYTES {
        return Err("Chromium PDF is empty or too large".to_string());
    }

    let pdf = std::fs::read(&pdf_path).map_err(|_| "Could not read Chromium PDF".to_string())?;
    if !pdf.starts_with(b"%PDF-") {
        return Err("Chromium produced an invalid PDF".to_string());
    }
    Ok(pdf)
}

#[cfg(target_os = "macos")]
const WEBKIT_RUNNER_SCRIPT: &str = r#"import Foundation
import WebKit
import AppKit

guard CommandLine.arguments.count > 5, let url = URL(string: CommandLine.arguments[1]) else { exit(1) }
let (w, h, wait, out) = (Double(CommandLine.arguments[2]) ?? 1280, Double(CommandLine.arguments[3]) ?? 720, Double(CommandLine.arguments[4]) ?? 1000, CommandLine.arguments[5])

class Delegate: NSObject, WKNavigationDelegate {
    func webView(_ v: WKWebView, didFinish: WKNavigation!) {
        DispatchQueue.main.asyncAfter(deadline: .now() + max(wait / 1000.0, 0.2)) {
            let cfg = WKSnapshotConfiguration()
            cfg.rect = CGRect(x: 0, y: 0, width: w, height: h)
            v.takeSnapshot(with: cfg) { img, _ in
                guard let img = img, let tiff = img.tiffRepresentation, let bmp = NSBitmapImageRep(data: tiff),
                      let png = bmp.representation(using: .png, properties: [:]) else { exit(1) }
                try? png.write(to: URL(fileURLWithPath: out))
                exit(0)
            }
        }
    }
    func webView(_ v: WKWebView, didFail: WKNavigation!, withError: Error) { exit(1) }
    func webView(_ v: WKWebView, didFailProvisionalNavigation: WKNavigation!, withError: Error) { exit(1) }
}

let app = NSApplication.shared
let wv = WKWebView(frame: NSRect(x: 0, y: 0, width: w, height: h))
let d = Delegate()
wv.navigationDelegate = d
var req = URLRequest(url: url)
req.setValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15", forHTTPHeaderField: "User-Agent")
wv.load(req)
DispatchQueue.main.asyncAfter(deadline: .now() + 25.0) { exit(1) }
app.run()
"#;

#[cfg(target_os = "macos")]
async fn run_webkit_screenshot(
    url: &str,
    options: &ScreenshotOptions,
) -> Result<Vec<u8>, String> {
    let temp_dir = TempDir::new().map_err(|_| "Could not create screenshot directory".to_string())?;
    let script_path = temp_dir.path().join("runner.swift");
    let screenshot_path = temp_dir.path().join("screenshot.png");
    std::fs::write(&script_path, WEBKIT_RUNNER_SCRIPT).map_err(|_| "Could not write runner".to_string())?;

    let (width, height) = if options.full_page {
        (options.render.viewport.0, FULL_PAGE_HEIGHT)
    } else {
        options.render.viewport
    };
    let wait_ms = options.render.wait_after_load.as_millis().max(100).to_string();

    let status = timeout(
        options.render.timeout,
        Command::new("swift")
            .arg(&script_path)
            .arg(url)
            .arg(width.to_string())
            .arg(height.to_string())
            .arg(wait_ms)
            .arg(&screenshot_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await
    .map_err(|_| "WebKit screenshot timed out".to_string())?
    .map_err(|_| "WebKit screenshot failed".to_string())?;

    if !status.success() {
        return Err("WebKit failed to capture the page".to_string());
    }

    let image = std::fs::read(&screenshot_path).map_err(|_| "Could not read WebKit screenshot".to_string())?;
    if image.is_empty() || image.len() > MAX_SCREENSHOT_BYTES as usize || !options.format.magic_bytes_ok(&image) {
        return Err("WebKit screenshot is empty or invalid".to_string());
    }
    Ok(image)
}


