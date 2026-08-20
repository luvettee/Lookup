use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, warn};

use super::types::{BrowserError, BrowserResult};

struct CommandRequest {
    id: u64,
    method: String,
    params: Value,
    response: oneshot::Sender<BrowserResult<Value>>,
}

#[derive(Clone)]
pub struct CdpClient {
    commands: mpsc::Sender<CommandRequest>,
    events: broadcast::Sender<Value>,
    next_id: Arc<AtomicU64>,
    default_timeout: Duration,
}

impl CdpClient {
    pub async fn connect(websocket_url: &str, default_timeout: Duration) -> BrowserResult<Self> {
        let (socket, _) = connect_async(websocket_url).await.map_err(|_| {
            BrowserError::new(
                "browser_disconnected",
                "Could not connect to the local browser tab",
            )
        })?;
        let (mut sink, mut stream) = socket.split();
        let (command_tx, mut command_rx) = mpsc::channel::<CommandRequest>(64);
        let (event_tx, _) = broadcast::channel::<Value>(256);
        let event_output = event_tx.clone();

        tokio::spawn(async move {
            let mut pending: HashMap<u64, oneshot::Sender<BrowserResult<Value>>> = HashMap::new();
            loop {
                tokio::select! {
                    request = command_rx.recv() => {
                        let Some(request) = request else { break; };
                        let payload = json!({
                            "id": request.id,
                            "method": request.method,
                            "params": request.params,
                        });
                        if sink.send(Message::Text(payload.to_string().into())).await.is_err() {
                            let _ = request.response.send(Err(BrowserError::new(
                                "browser_disconnected", "Browser connection closed while sending a command",
                            )));
                            break;
                        }
                        pending.insert(request.id, request.response);
                    }
                    incoming = stream.next() => {
                        let Some(incoming) = incoming else { break; };
                        let message = match incoming {
                            Ok(Message::Text(text)) => text.to_string(),
                            Ok(Message::Binary(bytes)) => String::from_utf8_lossy(&bytes).to_string(),
                            Ok(Message::Ping(payload)) => {
                                if sink.send(Message::Pong(payload)).await.is_err() { break; }
                                continue;
                            }
                            Ok(Message::Close(_)) | Err(_) => break,
                            _ => continue,
                        };
                        let Ok(value) = serde_json::from_str::<Value>(&message) else {
                            warn!("browser returned malformed CDP JSON");
                            continue;
                        };
                        if let Some(id) = value.get("id").and_then(Value::as_u64) {
                            if let Some(sender) = pending.remove(&id) {
                                if let Some(error) = value.get("error") {
                                    let message = error.get("message").and_then(Value::as_str)
                                        .unwrap_or("Chrome DevTools command failed");
                                    debug!(id, %message, "CDP command failed");
                                    let _ = sender.send(Err(BrowserError::new(
                                        "browser_action_failed", message.chars().take(300).collect::<String>(),
                                    )));
                                } else {
                                    let result = value.get("result").cloned().unwrap_or_else(|| json!({}));
                                    let _ = sender.send(Ok(result));
                                }
                            }
                        } else if value.get("method").is_some() {
                            let _ = event_output.send(value);
                        }
                    }
                }
            }
            for (_, sender) in pending {
                let _ = sender.send(Err(BrowserError::new(
                    "browser_disconnected",
                    "Browser connection closed",
                )));
            }
        });

        Ok(Self {
            commands: command_tx,
            events: event_tx,
            next_id: Arc::new(AtomicU64::new(1)),
            default_timeout,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
    }

    pub async fn command(&self, method: &str, params: Value) -> BrowserResult<Value> {
        self.command_with_timeout(method, params, self.default_timeout)
            .await
    }

    pub async fn command_with_timeout(
        &self,
        method: &str,
        params: Value,
        duration: Duration,
    ) -> BrowserResult<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (response_tx, response_rx) = oneshot::channel();
        self.commands
            .send(CommandRequest {
                id,
                method: method.to_string(),
                params,
                response: response_tx,
            })
            .await
            .map_err(|_| {
                BrowserError::new("browser_disconnected", "Browser connection is closed")
            })?;
        timeout(duration, response_rx)
            .await
            .map_err(|_| BrowserError::new("action_timeout", format!("{method} timed out")))?
            .map_err(|_| {
                BrowserError::new("browser_disconnected", "Browser command was cancelled")
            })?
    }

    pub async fn evaluate(&self, script: &str) -> BrowserResult<Value> {
        let result = self
            .command(
                "Runtime.evaluate",
                json!({
                    "expression": script,
                    "awaitPromise": true,
                    "returnByValue": true,
                    "userGesture": true,
                }),
            )
            .await?;
        if let Some(exception) = result.get("exceptionDetails") {
            let message = exception
                .get("exception")
                .and_then(|value| value.get("description"))
                .and_then(Value::as_str)
                .or_else(|| exception.get("text").and_then(Value::as_str))
                .unwrap_or("JavaScript evaluation failed");
            return Err(BrowserError::new(
                "javascript_error",
                message.chars().take(300).collect::<String>(),
            ));
        }
        Ok(result
            .get("result")
            .and_then(|value| value.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }
}
