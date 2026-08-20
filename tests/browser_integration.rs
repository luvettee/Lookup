use std::path::Path;
use std::time::Duration;

use lookup::browser::chrome::detect_browser;
use lookup::browser::manager::BrowserManager;
use lookup::browser::types::SnapshotMode;
use lookup::config::BrowserConfig;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn test_server() -> std::io::Result<(String, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr().expect("test server address");
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut request = [0u8; 2048];
                let count = socket.read(&mut request).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..count]);
                let second = request.starts_with("GET /second ");
                let body = if second {
                    r#"<!doctype html><title>Second</title><main><h1>Second page</h1><a href='/'>Home</a></main>"#
                } else {
                    r#"<!doctype html><title>Lookup browser test</title>
                    <main><h1>Browser test</h1><label>Name <input aria-label='Name'></label>
                    <label><input type='checkbox' aria-label='Remember me'> Remember me</label>
                    <select aria-label='Color'><option>Red</option><option>Blue</option></select>
                    <button onclick="document.querySelector('#result').textContent='Clicked '+document.querySelector('input').value">Submit</button>
                    <a href='/second'>Next page</a><p id='result'>Ready</p><div id='delayed'></div></main>
                    <script>setTimeout(()=>document.querySelector('#delayed').textContent='Loaded later', 250)</script>"#
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    Ok((format!("http://{address}/"), task))
}

fn element_id(snapshot: &Value, name: &str) -> u64 {
    snapshot["elements"]
        .as_array()
        .expect("snapshot elements")
        .iter()
        .find(|element| {
            element["name"]
                .as_str()
                .is_some_and(|value| value.contains(name))
        })
        .and_then(|element| element["id"].as_u64())
        .unwrap_or_else(|| panic!("missing snapshot element {name}: {snapshot}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_browser_flow_skips_without_chromium() {
    if detect_browser(None).is_none() {
        eprintln!("skipping browser integration test: no supported Chromium browser found");
        return;
    }

    std::env::set_var("LOOKUP_BROWSER_ENABLED", "true");
    std::env::set_var("LOOKUP_BROWSER_HEADLESS", "true");
    std::env::set_var("LOOKUP_ALLOW_PRIVATE_URLS", "true");
    std::env::set_var("LOOKUP_BROWSER_NAVIGATION_TIMEOUT_MS", "10000");
    let (url, server) = match test_server().await {
        Ok(server) => server,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping browser integration test: localhost listeners are not permitted");
            return;
        }
        Err(error) => panic!("could not start browser test server: {error}"),
    };
    let manager = BrowserManager::new(BrowserConfig::from_env());

    let opened = manager
        .open(&url)
        .await
        .expect("launch and open local page");
    let page_id = opened["page_id"].as_str().expect("page id").to_string();
    assert_eq!(opened["title"], "Lookup browser test");

    let snapshot = manager
        .snapshot(Some(&page_id), SnapshotMode::Interactive, 100)
        .await
        .expect("snapshot");
    let input_id = element_id(&snapshot, "Name");
    let button_id = element_id(&snapshot, "Submit");
    let link_id = element_id(&snapshot, "Next page");

    manager
        .type_text(Some(&page_id), Some(input_id), None, "Ada", true)
        .await
        .expect("type in textbox");
    manager
        .click(Some(&page_id), Some(button_id), None, None)
        .await
        .expect("click button");
    manager
        .wait(
            Some(&page_id),
            "text",
            Some("Clicked Ada"),
            Duration::from_secs(2),
        )
        .await
        .expect("wait for click result");
    manager
        .wait(
            Some(&page_id),
            "text",
            Some("Loaded later"),
            Duration::from_secs(2),
        )
        .await
        .expect("wait for delayed content");

    let readable = manager.read(Some(&page_id), 4000).await.expect("read page");
    assert!(readable["text"]
        .as_str()
        .unwrap_or_default()
        .contains("Clicked Ada"));
    let screenshot = manager
        .screenshot(Some(&page_id), false)
        .await
        .expect("screenshot");
    assert!(Path::new(screenshot["path"].as_str().expect("screenshot path")).is_file());
    assert_eq!(screenshot["mime_type"], "image/png");
    assert!(screenshot["_mcp_image"]["data"]
        .as_str()
        .is_some_and(|data| !data.is_empty()));

    manager
        .click(Some(&page_id), Some(link_id), None, None)
        .await
        .expect("click navigation link");
    manager
        .wait(
            Some(&page_id),
            "text",
            Some("Second page"),
            Duration::from_secs(3),
        )
        .await
        .expect("wait after navigation");
    let stale = manager
        .click(Some(&page_id), Some(button_id), None, None)
        .await;
    assert_eq!(
        stale.expect_err("old element should be stale").code,
        "element_reference_stale"
    );

    manager.history(Some(&page_id), -1).await.expect("go back");
    manager.reload(Some(&page_id)).await.expect("reload");
    manager
        .history(Some(&page_id), 1)
        .await
        .expect("go forward");
    let timed_out = manager
        .wait(
            Some(&page_id),
            "selector",
            Some("#never"),
            Duration::from_millis(150),
        )
        .await;
    assert_eq!(
        timed_out.expect_err("wait should timeout").code,
        "action_timeout"
    );

    let tabs = manager.tabs().await.expect("list tabs");
    assert_eq!(tabs["count"], 1);
    manager.close(Some(&page_id)).await.expect("close tab");
    manager.shutdown().await;

    let reopened = manager.open(&url).await.expect("restart after shutdown");
    assert_eq!(reopened["title"], "Lookup browser test");
    manager.shutdown().await;
    server.abort();
}
