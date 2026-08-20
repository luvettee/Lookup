use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use data_encoding::BASE64;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info};

use crate::config::BrowserConfig;

use super::chrome::ChromeProcess;
use super::page::{validate_browser_url, Page};
use super::types::{BrowserError, BrowserResult, PageId, SnapshotMode};

struct ManagerState {
    process: Option<Arc<Mutex<ChromeProcess>>>,
    pages: HashMap<PageId, Arc<Page>>,
    selected: Option<PageId>,
    screenshot_dir: Option<TempDir>,
}

pub struct BrowserManager {
    config: BrowserConfig,
    state: RwLock<ManagerState>,
    startup: Mutex<()>,
    next_page_id: AtomicU64,
    next_screenshot_id: AtomicU64,
}

impl BrowserManager {
    pub fn new(config: BrowserConfig) -> Self {
        Self {
            config,
            state: RwLock::new(ManagerState {
                process: None,
                pages: HashMap::new(),
                selected: None,
                screenshot_dir: None,
            }),
            startup: Mutex::new(()),
            next_page_id: AtomicU64::new(1),
            next_screenshot_id: AtomicU64::new(1),
        }
    }

    fn check_enabled(&self) -> BrowserResult<()> {
        if self.config.enabled {
            Ok(())
        } else {
            Err(BrowserError::new(
                "browser_disabled",
                "Browser control is disabled; set LOOKUP_BROWSER_ENABLED=true",
            ))
        }
    }

    async fn ensure_process(&self) -> BrowserResult<Arc<Mutex<ChromeProcess>>> {
        self.check_enabled()?;
        if let Some(process) = self.state.read().await.process.clone() {
            if process.lock().await.is_alive().await {
                return Ok(process);
            }
            info!("browser disconnected; restarting on next operation");
            let mut state = self.state.write().await;
            state.process = None;
            state.pages.clear();
            state.selected = None;
            state.screenshot_dir = None;
        }

        let _startup = self.startup.lock().await;
        if let Some(process) = self.state.read().await.process.clone() {
            return Ok(process);
        }
        let process = Arc::new(Mutex::new(
            ChromeProcess::connect_or_launch(&self.config).await?,
        ));
        let screenshot_dir = TempDir::new().map_err(|error| {
            BrowserError::new(
                "browser_launch_failed",
                format!("Could not create screenshot directory: {error}"),
            )
        })?;

        let imported_pages = if self.config.debug_url.is_some() {
            self.import_existing_pages(&process).await
        } else {
            Vec::new()
        };
        let mut state = self.state.write().await;
        state.process = Some(process.clone());
        state.screenshot_dir = Some(screenshot_dir);
        for page in imported_pages {
            state.selected = Some(page.id.clone());
            state.pages.insert(page.id.clone(), page);
        }
        Ok(process)
    }

    async fn import_existing_pages(&self, process: &Arc<Mutex<ChromeProcess>>) -> Vec<Arc<Page>> {
        let targets = match process.lock().await.targets().await {
            Ok(targets) => targets,
            Err(error) => {
                debug!(%error, "could not enumerate existing browser tabs");
                return Vec::new();
            }
        };
        let mut pages = Vec::new();
        for target in targets.into_iter().take(self.config.max_tabs) {
            let Some(websocket) = target.web_socket_debugger_url.as_deref() else {
                continue;
            };
            let page_id = PageId::new(format!(
                "page-{}",
                self.next_page_id.fetch_add(1, Ordering::Relaxed)
            ));
            match Page::connect(
                page_id.clone(),
                target.id.clone(),
                websocket,
                self.config.clone(),
            )
            .await
            {
                Ok(page) => {
                    info!(page_id = %page_id, "imported existing browser tab");
                    pages.push(Arc::new(page));
                }
                Err(error) => debug!(page_id = %page_id, %error, "could not import browser tab"),
            }
        }
        pages
    }

    pub async fn open(&self, url: &str) -> BrowserResult<Value> {
        let safe_url = validate_browser_url(url).await?;
        let process = self.ensure_process().await?;
        if self.state.read().await.pages.len() >= self.config.max_tabs {
            return Err(BrowserError::new(
                "too_many_tabs",
                "Maximum browser tab count reached",
            ));
        }
        let target = process.lock().await.new_target("about:blank").await?;
        let websocket = target.web_socket_debugger_url.as_deref().ok_or_else(|| {
            BrowserError::new(
                "browser_disconnected",
                "Browser tab has no local CDP endpoint",
            )
        })?;
        let page_id = PageId::new(format!(
            "page-{}",
            self.next_page_id.fetch_add(1, Ordering::Relaxed)
        ));
        let page = Arc::new(
            Page::connect(
                page_id.clone(),
                target.id.clone(),
                websocket,
                self.config.clone(),
            )
            .await?,
        );
        {
            let mut state = self.state.write().await;
            state.pages.insert(page_id.clone(), page.clone());
            state.selected = Some(page_id.clone());
        }
        info!(page_id = %page_id, "browser page created");
        match page.navigate(&safe_url).await {
            Ok(info) => Ok(json!({
                "page_id": info.page_id,
                "url": info.url,
                "title": info.title,
                "load_state": "complete"
            })),
            Err(error) => {
                self.remove_page(&page_id).await;
                let _ = process.lock().await.close_target(&target.id).await;
                Err(error)
            }
        }
    }

    pub async fn tabs(&self) -> BrowserResult<Value> {
        let process = self.ensure_process().await?;
        let live_targets = process.lock().await.targets().await?;
        let live_ids: std::collections::HashSet<&str> = live_targets
            .iter()
            .map(|target| target.id.as_str())
            .collect();
        let (pages, selected) = {
            let state = self.state.read().await;
            (
                state.pages.values().cloned().collect::<Vec<_>>(),
                state.selected.clone(),
            )
        };
        let mut output = Vec::new();
        let mut stale = Vec::new();
        for page in pages {
            if !live_ids.contains(page.target_id.as_str()) {
                stale.push(page.id.clone());
                continue;
            }
            match page.info(selected.as_ref() == Some(&page.id)).await {
                Ok(info) => output.push(info),
                Err(_) => stale.push(page.id.clone()),
            }
        }
        if !stale.is_empty() {
            let mut state = self.state.write().await;
            for id in stale {
                state.pages.remove(&id);
            }
        }
        Ok(json!({ "tabs": output, "count": output.len(), "max_tabs": self.config.max_tabs }))
    }

    pub async fn navigate(&self, page_id: Option<&str>, url: &str) -> BrowserResult<Value> {
        let page = self.page(page_id).await?;
        let info = page.navigate(url).await?;
        self.select(&page.id).await;
        Ok(
            json!({ "page_id": info.page_id, "url": info.url, "title": info.title, "load_state": "complete" }),
        )
    }

    pub async fn snapshot(
        &self,
        page_id: Option<&str>,
        mode: SnapshotMode,
        limit: usize,
    ) -> BrowserResult<Value> {
        self.page(page_id).await?.snapshot(mode, limit).await
    }

    pub async fn click(
        &self,
        page_id: Option<&str>,
        element_id: Option<u64>,
        selector: Option<&str>,
        coordinates: Option<(f64, f64)>,
    ) -> BrowserResult<Value> {
        self.page(page_id)
            .await?
            .click(element_id, selector, coordinates)
            .await
    }

    pub async fn type_text(
        &self,
        page_id: Option<&str>,
        element_id: Option<u64>,
        selector: Option<&str>,
        text: &str,
        clear: bool,
    ) -> BrowserResult<Value> {
        self.page(page_id)
            .await?
            .type_text(element_id, selector, text, clear)
            .await
    }

    pub async fn press(&self, page_id: Option<&str>, key: &str) -> BrowserResult<Value> {
        self.page(page_id).await?.press(key).await
    }

    pub async fn scroll(
        &self,
        page_id: Option<&str>,
        direction: Option<&str>,
        amount: i64,
        element_id: Option<u64>,
    ) -> BrowserResult<Value> {
        self.page(page_id)
            .await?
            .scroll(direction, amount, element_id)
            .await
    }

    pub async fn wait(
        &self,
        page_id: Option<&str>,
        condition: &str,
        value: Option<&str>,
        timeout: Duration,
    ) -> BrowserResult<Value> {
        self.page(page_id)
            .await?
            .wait_for(condition, value, timeout)
            .await
    }

    pub async fn read(&self, page_id: Option<&str>, max_chars: usize) -> BrowserResult<Value> {
        self.page(page_id).await?.read(max_chars).await
    }

    pub async fn screenshot(&self, page_id: Option<&str>, full_page: bool) -> BrowserResult<Value> {
        let page = self.page(page_id).await?;
        let png = page.screenshot(full_page).await?;
        let size_bytes = png.len();
        let image_data = BASE64.encode(&png);
        let path = self.screenshot_path().await?;
        let write_path = path.clone();
        tokio::task::spawn_blocking(move || std::fs::write(&write_path, png))
            .await
            .map_err(|_| {
                BrowserError::new("browser_action_failed", "Screenshot write task failed")
            })?
            .map_err(|error| {
                BrowserError::new(
                    "browser_action_failed",
                    format!("Could not write screenshot: {error}"),
                )
            })?;
        Ok(json!({
            "page_id": page.id,
            "path": path,
            "mime_type": "image/png",
            "size_bytes": size_bytes,
            "full_page": full_page,
            "_mcp_image": {
                "mime_type": "image/png",
                "data": image_data
            }
        }))
    }

    pub async fn history(&self, page_id: Option<&str>, direction: i64) -> BrowserResult<Value> {
        let info = self.page(page_id).await?.history(direction).await?;
        Ok(
            json!({ "page_id": info.page_id, "url": info.url, "title": info.title, "load_state": "complete" }),
        )
    }

    pub async fn reload(&self, page_id: Option<&str>) -> BrowserResult<Value> {
        let info = self.page(page_id).await?.reload().await?;
        Ok(
            json!({ "page_id": info.page_id, "url": info.url, "title": info.title, "load_state": "complete" }),
        )
    }

    pub async fn evaluate(&self, page_id: Option<&str>, script: &str) -> BrowserResult<Value> {
        let page = self.page(page_id).await?;
        let result = page.evaluate_user(script).await?;
        Ok(json!({ "page_id": page.id, "result": result }))
    }

    pub async fn close(&self, page_id: Option<&str>) -> BrowserResult<Value> {
        if let Some(id) = page_id {
            let page = self.page(Some(id)).await?;
            let process = self.ensure_process().await?;
            process.lock().await.close_target(&page.target_id).await?;
            self.remove_page(&page.id).await;
            info!(page_id = %page.id, "browser page closed");
            return Ok(json!({ "closed": "page", "page_id": page.id }));
        }
        self.shutdown().await;
        Ok(json!({ "closed": "browser" }))
    }

    pub async fn shutdown(&self) {
        let process = {
            let mut state = self.state.write().await;
            state.pages.clear();
            state.selected = None;
            state.screenshot_dir = None;
            state.process.take()
        };
        if let Some(process) = process {
            process.lock().await.shutdown().await;
        }
    }

    async fn page(&self, requested: Option<&str>) -> BrowserResult<Arc<Page>> {
        self.ensure_process().await?;
        let state = self.state.read().await;
        let id = match requested {
            Some(id) => PageId::new(id),
            None => state.selected.clone().ok_or_else(|| {
                BrowserError::new(
                    "page_not_found",
                    "No browser tab is selected; open a page first",
                )
            })?,
        };
        state.pages.get(&id).cloned().ok_or_else(|| {
            BrowserError::new("page_not_found", format!("Browser tab {id} was not found"))
        })
    }

    async fn select(&self, page_id: &PageId) {
        self.state.write().await.selected = Some(page_id.clone());
    }

    async fn remove_page(&self, page_id: &PageId) {
        let mut state = self.state.write().await;
        state.pages.remove(page_id);
        if state.selected.as_ref() == Some(page_id) {
            state.selected = state.pages.keys().next().cloned();
        }
    }

    async fn screenshot_path(&self) -> BrowserResult<PathBuf> {
        let state = self.state.read().await;
        let directory = state.screenshot_dir.as_ref().ok_or_else(|| {
            BrowserError::new(
                "browser_disconnected",
                "Browser screenshot directory is unavailable",
            )
        })?;
        let id = self.next_screenshot_id.fetch_add(1, Ordering::Relaxed);
        Ok(directory.path().join(format!("lookup-browser-{id}.png")))
    }
}

static BROWSER_MANAGER: LazyLock<BrowserManager> =
    LazyLock::new(|| BrowserManager::new(BrowserConfig::from_env()));

pub fn global_manager() -> &'static BrowserManager {
    &BROWSER_MANAGER
}

pub async fn shutdown_global() {
    BROWSER_MANAGER.shutdown().await;
    debug!("browser manager shutdown finished");
}
