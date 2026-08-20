use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use data_encoding::BASE64;
use serde_json::{json, Value};
use tokio::time::sleep;
use tracing::{debug, warn};

use crate::config::{
    BrowserConfig, MAX_SCREENSHOT_BYTES, MAX_SCREENSHOT_HEIGHT, MAX_SCREENSHOT_WIDTH,
};
use crate::net::ssrf::validate_url_async;

use super::cdp::CdpClient;
use super::types::{BrowserError, BrowserResult, PageId, PageInfo, SnapshotMode};

pub struct Page {
    pub id: PageId,
    pub target_id: String,
    cdp: CdpClient,
    next_element_id: AtomicU64,
    config: BrowserConfig,
}

impl Page {
    pub async fn connect(
        id: PageId,
        target_id: String,
        websocket_url: &str,
        config: BrowserConfig,
    ) -> BrowserResult<Self> {
        let cdp = CdpClient::connect(websocket_url, config.action_timeout).await?;
        cdp.command("Page.enable", json!({})).await?;
        cdp.command("Runtime.enable", json!({})).await?;
        cdp.command(
            "Fetch.enable",
            json!({ "patterns": [{ "urlPattern": "*", "requestStage": "Request" }] }),
        )
        .await?;
        spawn_request_guard(cdp.clone());
        Ok(Self {
            id,
            target_id,
            cdp,
            next_element_id: AtomicU64::new(1),
            config,
        })
    }

    pub async fn info(&self, selected: bool) -> BrowserResult<PageInfo> {
        let value = self
            .cdp
            .evaluate("({title: document.title || '', url: location.href})")
            .await?;
        Ok(PageInfo {
            page_id: self.id.clone(),
            title: value
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect(),
            url: value
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(4096)
                .collect(),
            selected,
        })
    }

    pub async fn navigate(&self, url: &str) -> BrowserResult<PageInfo> {
        let safe_url = validate_browser_url(url).await?;
        debug!(page_id = %self.id, url = %safe_url, "browser navigation started");
        let result = self
            .cdp
            .command_with_timeout(
                "Page.navigate",
                json!({ "url": safe_url }),
                self.config.navigation_timeout,
            )
            .await
            .map_err(map_navigation_timeout)?;
        if let Some(error) = result.get("errorText").and_then(Value::as_str) {
            return Err(BrowserError::new(
                "navigation_failed",
                error.chars().take(300).collect::<String>(),
            ));
        }
        self.wait_ready(self.config.navigation_timeout).await?;
        debug!(page_id = %self.id, "browser navigation completed");
        self.info(true).await
    }

    pub async fn snapshot(
        &self,
        mode: SnapshotMode,
        requested_limit: usize,
    ) -> BrowserResult<Value> {
        let limit = requested_limit.clamp(1, self.config.max_snapshot_elements);
        let seed = self
            .next_element_id
            .fetch_add(limit as u64 + 1, Ordering::Relaxed);
        let include_content = mode == SnapshotMode::Full;
        let interactive_only = mode == SnapshotMode::Interactive;
        let script = SNAPSHOT_SCRIPT
            .replace("__LIMIT__", &limit.to_string())
            .replace("__SEED__", &seed.to_string())
            .replace(
                "__CONTENT__",
                if include_content { "true" } else { "false" },
            )
            .replace(
                "__INTERACTIVE_ONLY__",
                if interactive_only { "true" } else { "false" },
            );
        let mut result = self.cdp.evaluate(&script).await?;
        let returned = result
            .get("elements")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        self.next_element_id
            .fetch_max(seed + returned as u64 + 1, Ordering::Relaxed);
        if let Some(object) = result.as_object_mut() {
            object.insert("mode".to_string(), json!(mode.as_str()));
            object.insert("page_id".to_string(), json!(self.id));
        }
        bound_json(result, self.config.max_response_chars)
    }

    pub async fn click(
        &self,
        element_id: Option<u64>,
        selector: Option<&str>,
        coordinates: Option<(f64, f64)>,
    ) -> BrowserResult<Value> {
        if let Some((x, y)) = coordinates {
            self.cdp
                .command(
                    "Input.dispatchMouseEvent",
                    json!({
                        "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1
                    }),
                )
                .await?;
            self.cdp
                .command(
                    "Input.dispatchMouseEvent",
                    json!({
                        "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1
                    }),
                )
                .await?;
            return Ok(json!({ "clicked": true, "x": x, "y": y }));
        }
        let target = element_expression(element_id, selector)?;
        let script = format!(
            r#"(() => {{
            const el = {target};
            if (!el || !el.isConnected) throw new Error('LOOKUP_STALE');
            el.scrollIntoView({{block:'center', inline:'center'}});
            el.click();
            return {{clicked:true, tag:el.tagName.toLowerCase()}};
        }})()"#
        );
        self.evaluate_element_script(&script).await
    }

    pub async fn type_text(
        &self,
        element_id: Option<u64>,
        selector: Option<&str>,
        text: &str,
        clear: bool,
    ) -> BrowserResult<Value> {
        let target = element_expression(element_id, selector)?;
        let encoded =
            serde_json::to_string(text).map_err(|_| BrowserError::invalid("text is invalid"))?;
        let script = format!(
            r#"(() => {{
            const el = {target};
            if (!el || !el.isConnected) throw new Error('LOOKUP_STALE');
            if (el.disabled || el.readOnly) throw new Error('Element is disabled or read-only');
            el.scrollIntoView({{block:'center'}}); el.focus();
            const text = {encoded};
            if (el.isContentEditable) {{ if ({clear}) el.textContent = ''; el.textContent += text; }}
            else if ('value' in el) {{
                const proto = Object.getPrototypeOf(el);
                const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
                const value = {clear} ? text : String(el.value || '') + text;
                setter ? setter.call(el, value) : el.value = value;
            }} else throw new Error('Element is not editable');
            el.dispatchEvent(new Event('input', {{bubbles:true}}));
            el.dispatchEvent(new Event('change', {{bubbles:true}}));
            return {{typed:true, value: ('value' in el ? String(el.value) : String(el.textContent || '')).slice(0,500)}};
        }})()"#
        );
        self.evaluate_element_script(&script).await
    }

    pub async fn press(&self, key_spec: &str) -> BrowserResult<Value> {
        let (key, modifiers) = parse_key(key_spec)?;
        let code = key_code(&key);
        self.cdp
            .command(
                "Input.dispatchKeyEvent",
                json!({ "type": "keyDown", "key": key, "code": code, "modifiers": modifiers }),
            )
            .await?;
        self.cdp
            .command(
                "Input.dispatchKeyEvent",
                json!({ "type": "keyUp", "key": key, "code": code, "modifiers": modifiers }),
            )
            .await?;
        Ok(json!({ "pressed": key_spec }))
    }

    pub async fn scroll(
        &self,
        direction: Option<&str>,
        amount: i64,
        element_id: Option<u64>,
    ) -> BrowserResult<Value> {
        if let Some(element_id) = element_id {
            let script = format!(
                r#"(() => {{ const el = globalThis.__lookupElements?.get({element_id});
                if (!el || !el.isConnected) throw new Error('LOOKUP_STALE');
                el.scrollIntoView({{block:'center', inline:'center'}}); return {{scrolled:true}}; }})()"#
            );
            return self.evaluate_element_script(&script).await;
        }
        let (x, y) = match direction.unwrap_or("down") {
            "up" => (0, -amount.abs()),
            "down" => (0, amount.abs()),
            "left" => (-amount.abs(), 0),
            "right" => (amount.abs(), 0),
            _ => {
                return Err(BrowserError::invalid(
                    "direction must be up, down, left, or right",
                ))
            }
        };
        self.cdp
            .evaluate(&format!("scrollBy({x}, {y}); ({{x:scrollX,y:scrollY}})"))
            .await
    }

    pub async fn wait_for(
        &self,
        condition: &str,
        value: Option<&str>,
        requested_timeout: Duration,
    ) -> BrowserResult<Value> {
        let timeout = requested_timeout.min(Duration::from_secs(30));
        let started = Instant::now();
        let initial_url = self
            .cdp
            .evaluate("location.href")
            .await?
            .as_str()
            .unwrap_or_default()
            .to_string();
        let mut stable_since = Instant::now();
        let mut previous_resources = None;
        loop {
            let matched = match condition {
                "selector" => {
                    let selector = required_wait_value(value, condition)?;
                    self.cdp
                        .evaluate(&format!(
                            "!!document.querySelector({})",
                            js_string(selector)?
                        ))
                        .await?
                        .as_bool()
                        == Some(true)
                }
                "visible" => {
                    let selector = required_wait_value(value, condition)?;
                    self.cdp.evaluate(&format!(r#"(() => {{ const e=document.querySelector({}); if(!e)return false; const r=e.getBoundingClientRect(); const s=getComputedStyle(e); return r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none'; }})()"#, js_string(selector)?)).await?.as_bool() == Some(true)
                }
                "text" => {
                    let text = required_wait_value(value, condition)?;
                    self.cdp
                        .evaluate(&format!(
                            "(document.body?.innerText || '').includes({})",
                            js_string(text)?
                        ))
                        .await?
                        .as_bool()
                        == Some(true)
                }
                "url" => {
                    let expected = required_wait_value(value, condition)?;
                    self.cdp
                        .evaluate("location.href")
                        .await?
                        .as_str()
                        .is_some_and(|url| url.contains(expected))
                }
                "url_change" => self
                    .cdp
                    .evaluate("location.href")
                    .await?
                    .as_str()
                    .is_some_and(|url| url != initial_url),
                "navigation" => {
                    self.cdp
                        .evaluate("document.readyState === 'complete'")
                        .await?
                        .as_bool()
                        == Some(true)
                }
                "network_idle" => {
                    let count = self
                        .cdp
                        .evaluate("performance.getEntriesByType('resource').length")
                        .await?
                        .as_u64();
                    if count == previous_resources {
                        stable_since.elapsed() >= Duration::from_millis(500)
                    } else {
                        previous_resources = count;
                        stable_since = Instant::now();
                        false
                    }
                }
                _ => return Err(BrowserError::invalid("unsupported wait condition")),
            };
            if matched {
                return Ok(
                    json!({ "matched": true, "condition": condition, "elapsed_ms": started.elapsed().as_millis() }),
                );
            }
            if started.elapsed() >= timeout {
                return Err(BrowserError::new(
                    "action_timeout",
                    format!("Wait for {condition} timed out"),
                ));
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn read(&self, max_chars: usize) -> BrowserResult<Value> {
        let limit = max_chars.clamp(100, self.config.max_response_chars);
        let script = format!(
            r#"(() => {{
            const root = document.querySelector('article, main, [role="main"]') || document.body;
            const text = (root?.innerText || '').replace(/\n{{3,}}/g, '\n\n').trim();
            return {{title:document.title||'', url:location.href, text:text.slice(0,{limit}), truncated:text.length>{limit}, total_chars:text.length}};
        }})()"#
        );
        self.cdp.evaluate(&script).await
    }

    pub async fn screenshot(&self, full_page: bool) -> BrowserResult<Vec<u8>> {
        let mut params =
            json!({ "format": "png", "fromSurface": true, "captureBeyondViewport": full_page });
        if full_page {
            let metrics = self.cdp.command("Page.getLayoutMetrics", json!({})).await?;
            let content = metrics
                .get("cssContentSize")
                .or_else(|| metrics.get("contentSize"));
            let width = content
                .and_then(|v| v.get("width"))
                .and_then(Value::as_f64)
                .unwrap_or(1280.0)
                .min(f64::from(MAX_SCREENSHOT_WIDTH));
            let height = content
                .and_then(|v| v.get("height"))
                .and_then(Value::as_f64)
                .unwrap_or(720.0)
                .min(f64::from(MAX_SCREENSHOT_HEIGHT) * 12.0);
            params["clip"] =
                json!({ "x": 0, "y": 0, "width": width, "height": height, "scale": 1 });
        }
        let result = self
            .cdp
            .command_with_timeout(
                "Page.captureScreenshot",
                params,
                self.config.navigation_timeout,
            )
            .await?;
        let encoded = result.get("data").and_then(Value::as_str).ok_or_else(|| {
            BrowserError::new("browser_action_failed", "Browser returned no screenshot")
        })?;
        let bytes = BASE64.decode(encoded.as_bytes()).map_err(|_| {
            BrowserError::new(
                "browser_action_failed",
                "Browser returned an invalid screenshot",
            )
        })?;
        if bytes.is_empty() || bytes.len() > MAX_SCREENSHOT_BYTES as usize {
            return Err(BrowserError::new(
                "response_too_large",
                "Screenshot exceeded Lookup's size limit",
            ));
        }
        Ok(bytes)
    }

    pub async fn history(&self, direction: i64) -> BrowserResult<PageInfo> {
        let history = self
            .cdp
            .command("Page.getNavigationHistory", json!({}))
            .await?;
        let current = history
            .get("currentIndex")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let entries = history
            .get("entries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let desired = current + direction;
        if desired < 0 || desired as usize >= entries.len() {
            return Err(BrowserError::new(
                "navigation_unavailable",
                "No history entry exists in that direction",
            ));
        }
        let entry_id = entries[desired as usize]
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                BrowserError::new("navigation_unavailable", "Browser history entry is invalid")
            })?;
        self.cdp
            .command(
                "Page.navigateToHistoryEntry",
                json!({ "entryId": entry_id }),
            )
            .await?;
        self.wait_ready(self.config.navigation_timeout).await?;
        self.info(true).await
    }

    pub async fn reload(&self) -> BrowserResult<PageInfo> {
        self.cdp
            .command("Page.reload", json!({ "ignoreCache": false }))
            .await?;
        self.wait_ready(self.config.navigation_timeout).await?;
        self.info(true).await
    }

    pub async fn evaluate_user(&self, script: &str) -> BrowserResult<Value> {
        if script.len() > 16_000 {
            return Err(BrowserError::invalid("script is too long"));
        }
        let value = self.cdp.evaluate(script).await?;
        bound_json(value, self.config.max_javascript_chars)
    }

    async fn wait_ready(&self, timeout: Duration) -> BrowserResult<()> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            match self.cdp.evaluate("document.readyState").await {
                Ok(Value::String(state)) if state == "complete" || state == "interactive" => {
                    return Ok(())
                }
                Ok(_) => {}
                Err(error) if error.code == "browser_disconnected" => return Err(error),
                Err(_) => {}
            }
            sleep(Duration::from_millis(75)).await;
        }
        Err(BrowserError::new(
            "navigation_timeout",
            "Page did not finish loading before the navigation timeout",
        ))
    }

    async fn evaluate_element_script(&self, script: &str) -> BrowserResult<Value> {
        self.cdp.evaluate(script).await.map_err(|error| {
            if error.message.contains("LOOKUP_STALE") {
                BrowserError::new(
                    "element_reference_stale",
                    "The element reference is stale; take a new snapshot",
                )
            } else {
                error
            }
        })
    }
}

fn spawn_request_guard(cdp: CdpClient) {
    let mut events = cdp.subscribe();
    tokio::spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    warn!(count, "browser request guard lagged");
                    continue;
                }
                Err(_) => break,
            };
            if event.get("method").and_then(Value::as_str) != Some("Fetch.requestPaused") {
                continue;
            }
            let params = &event["params"];
            let Some(request_id) = params.get("requestId").and_then(Value::as_str) else {
                continue;
            };
            let url = params
                .get("request")
                .and_then(|v| v.get("url"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let allowed = request_allowed(url).await;
            let command = if allowed {
                "Fetch.continueRequest"
            } else {
                "Fetch.failRequest"
            };
            let payload = if allowed {
                json!({ "requestId": request_id })
            } else {
                debug!(url = %redact_url(url), "blocked browser request by URL policy");
                json!({ "requestId": request_id, "errorReason": "BlockedByClient" })
            };
            if cdp.command(command, payload).await.is_err() {
                break;
            }
        }
    });
}

async fn request_allowed(raw: &str) -> bool {
    if raw == "about:blank"
        || raw.starts_with("blob:")
        || raw.starts_with("data:image/")
        || raw.starts_with("data:font/")
    {
        return true;
    }
    validate_url_async(raw, true).await.is_ok()
}

pub async fn validate_browser_url(raw: &str) -> BrowserResult<String> {
    validate_url_async(raw, true)
        .await
        .map_err(|message| BrowserError::new("invalid_url", message))
}

fn redact_url(raw: &str) -> String {
    url::Url::parse(raw)
        .map(|url| {
            let host = url.host_str().unwrap_or("<unknown>");
            format!("{}://{host}", url.scheme())
        })
        .unwrap_or_else(|_| "<invalid URL>".to_string())
}

fn map_navigation_timeout(error: BrowserError) -> BrowserError {
    if error.code == "action_timeout" {
        BrowserError::new("navigation_timeout", error.message)
    } else {
        error
    }
}

fn element_expression(element_id: Option<u64>, selector: Option<&str>) -> BrowserResult<String> {
    if let Some(id) = element_id {
        return Ok(format!("globalThis.__lookupElements?.get({id})"));
    }
    if let Some(selector) = selector {
        return Ok(format!("document.querySelector({})", js_string(selector)?));
    }
    Err(BrowserError::invalid("element_id or selector is required"))
}

fn js_string(value: &str) -> BrowserResult<String> {
    serde_json::to_string(value).map_err(|_| BrowserError::invalid("value cannot be encoded"))
}

fn required_wait_value<'a>(value: Option<&'a str>, condition: &str) -> BrowserResult<&'a str> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BrowserError::invalid(format!("value is required for {condition}")))
}

fn parse_key(spec: &str) -> BrowserResult<(String, u8)> {
    let mut parts: Vec<&str> = spec.split('+').filter(|part| !part.is_empty()).collect();
    let key = parts
        .pop()
        .ok_or_else(|| BrowserError::invalid("key must not be empty"))?;
    let mut modifiers = 0u8;
    for modifier in parts {
        match modifier.to_ascii_lowercase().as_str() {
            "alt" | "option" => modifiers |= 1,
            "control" | "ctrl" => modifiers |= 2,
            "meta" | "command" | "cmd" => modifiers |= 4,
            "shift" => modifiers |= 8,
            _ => {
                return Err(BrowserError::invalid(format!(
                    "Unknown key modifier: {modifier}"
                )))
            }
        }
    }
    #[cfg(target_os = "macos")]
    if spec.starts_with("Control+") {
        modifiers = (modifiers & !2) | 4;
    }
    Ok((normalize_key(key), modifiers))
}

fn normalize_key(key: &str) -> String {
    match key.to_ascii_lowercase().as_str() {
        "enter" | "return" => "Enter".to_string(),
        "escape" | "esc" => "Escape".to_string(),
        "tab" => "Tab".to_string(),
        "backspace" => "Backspace".to_string(),
        "delete" => "Delete".to_string(),
        "arrowup" | "up" => "ArrowUp".to_string(),
        "arrowdown" | "down" => "ArrowDown".to_string(),
        "arrowleft" | "left" => "ArrowLeft".to_string(),
        "arrowright" | "right" => "ArrowRight".to_string(),
        "space" => " ".to_string(),
        _ => key.to_string(),
    }
}

fn key_code(key: &str) -> String {
    match key {
        " " => "Space".to_string(),
        value if value.len() == 1 => format!("Key{}", value.to_ascii_uppercase()),
        value => value.to_string(),
    }
}

fn bound_json(mut value: Value, max_chars: usize) -> BrowserResult<Value> {
    let serialized = serde_json::to_string(&value).map_err(|_| {
        BrowserError::new(
            "browser_action_failed",
            "Could not serialize browser result",
        )
    })?;
    if serialized.len() <= max_chars {
        return Ok(value);
    }
    if let Some(object) = value.as_object_mut() {
        object.insert("response_truncated".to_string(), Value::Bool(true));
        if let Some(Value::String(text)) = object.get_mut("text") {
            text.truncate(max_chars.saturating_div(2));
        }
        loop {
            if serde_json::to_string(&value).map_or(usize::MAX, |serialized| serialized.len())
                <= max_chars
            {
                break;
            }
            let Some(elements) = value.get_mut("elements").and_then(Value::as_array_mut) else {
                break;
            };
            if elements.len() <= 1 {
                break;
            }
            elements.pop();
        }
        return Ok(value);
    }
    Err(BrowserError::new(
        "response_too_large",
        "Browser result exceeded Lookup's response limit",
    ))
}

const SNAPSHOT_SCRIPT: &str = r#"(() => {
  const limit=__LIMIT__, seed=__SEED__, includeContent=__CONTENT__, interactiveOnly=__INTERACTIVE_ONLY__;
  globalThis.__lookupElements = new Map();
  const visible = el => { const s=getComputedStyle(el), r=el.getBoundingClientRect(); return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0; };
  const roleOf = el => el.getAttribute('role') || ({A:'link',BUTTON:'button',INPUT:({checkbox:'checkbox',radio:'radio',submit:'button',button:'button',range:'slider',email:'textbox',password:'textbox',search:'textbox',text:'textbox',url:'textbox'}[el.type]||'input'),TEXTAREA:'textbox',SELECT:'combobox',H1:'heading',H2:'heading',H3:'heading',H4:'heading',H5:'heading',H6:'heading',FORM:'form'}[el.tagName]||'');
  const nameOf = el => { const labelled=el.getAttribute('aria-label')||el.getAttribute('title')||el.getAttribute('placeholder')||el.getAttribute('alt'); if(labelled)return labelled; if(el.labels?.length)return [...el.labels].map(x=>x.innerText).join(' '); return (el.innerText||el.value||'').trim(); };
  const actionable = el => el.matches('a[href],button,input,textarea,select,summary,[role="button"],[role="link"],[role="textbox"],[role="checkbox"],[role="radio"],[tabindex]:not([tabindex="-1"])');
  const candidates=[...document.querySelectorAll('a[href],button,input,textarea,select,summary,h1,h2,h3,h4,h5,h6,form,[role],[tabindex],p,li')];
  const elements=[]; let total=0;
  for(const el of candidates){ if(!visible(el))continue; const interactive=actionable(el); if(interactiveOnly&&!interactive)continue; if(!interactive&&!includeContent&&!/^H[1-6]$/.test(el.tagName))continue; total++; if(elements.length>=limit)continue; const id=seed+elements.length; globalThis.__lookupElements.set(id,el); const role=roleOf(el); const item={id,role:role||'text',name:nameOf(el).replace(/\s+/g,' ').slice(0,180)}; if(!item.name&&!interactive)continue; if(el.href)item.href=el.href.slice(0,500); if(el.disabled)item.disabled=true; if('checked' in el)item.checked=!!el.checked; if(el.tagName==='SELECT')item.value=String(el.value).slice(0,180); if(/^H[1-6]$/.test(el.tagName))item.level=Number(el.tagName[1]); elements.push(item); }
  const lines=elements.map(e=>`[${e.id}] ${e.role} "${e.name}"${e.href?` -> ${e.href}`:''}${e.disabled?' disabled':''}${e.checked===true?' checked':''}`);
  const bodyText=includeContent?(document.querySelector('main,article,[role="main"]')||document.body)?.innerText?.replace(/\n{3,}/g,'\n\n').trim().slice(0,8000)||'':'';
  return {title:(document.title||'').slice(0,300),url:location.href,elements,snapshot:lines.join('\n'),content:bodyText||undefined,truncated:total>elements.length,remaining_elements:Math.max(0,total-elements.length)};
})()"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_modifiers_are_bounded() {
        let (_, modifiers) = parse_key("Shift+Control+Enter").expect("valid key");
        assert_ne!(modifiers, 0);
        assert!(parse_key("Magic+X").is_err());
    }
}
