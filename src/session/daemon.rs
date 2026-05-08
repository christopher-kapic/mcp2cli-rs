use crate::error::{AppError, Result};
use crate::mcp::protocol::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

/// Session daemon that holds an MCP client and serves JSON-RPC over a Unix socket.
pub struct SessionDaemon {
    client: Arc<tokio::sync::Mutex<Box<dyn McpClient>>>,
}

impl SessionDaemon {
    pub fn new(client: Box<dyn McpClient>) -> Self {
        Self {
            client: Arc::new(tokio::sync::Mutex::new(client)),
        }
    }

    /// Run the daemon, listening on the given Unix socket path.
    /// Blocks until SIGTERM is received.
    pub async fn run(self, socket_path: &Path) -> Result<()> {
        // Remove stale socket if it exists
        let _ = tokio::fs::remove_file(socket_path).await;

        // Ensure parent directory exists
        if let Some(parent) = socket_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let listener = UnixListener::bind(socket_path)?;

        // Initialize the MCP client
        {
            let mut client = self.client.lock().await;
            client.initialize().await?;
        }

        let client = self.client.clone();

        // Handle SIGTERM for clean shutdown
        let shutdown = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(AppError::Io)?;

        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _)) => {
                            let client = client.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, client).await {
                                    eprintln!("Session connection error: {e}");
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("Session accept error: {e}");
                        }
                    }
                }
                _ = shutdown.recv() => {
                    break;
                }
            }
        }

        // Cleanup socket file
        let _ = tokio::fs::remove_file(socket_path).await;
        Ok(())
    }
}

/// Handle a single client connection: read line-delimited JSON-RPC requests,
/// dispatch to the MCP client, and write responses.
async fn handle_connection(
    stream: tokio::net::UnixStream,
    client: Arc<tokio::sync::Mutex<Box<dyn McpClient>>>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let error_response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {e}")
                    }
                });
                let mut resp = serde_json::to_string(&error_response).unwrap();
                resp.push('\n');
                writer.write_all(resp.as_bytes()).await?;
                continue;
            }
        };

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = request.get("params").cloned();

        let result = dispatch_method(&client, method, params).await;

        let response = match result {
            Ok(value) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": value
            }),
            Err(e) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32603,
                    "message": format!("{e}")
                }
            }),
        };

        let mut resp = serde_json::to_string(&response).unwrap();
        resp.push('\n');
        writer.write_all(resp.as_bytes()).await?;
    }

    Ok(())
}

/// Dispatch a JSON-RPC method to the appropriate McpClient method.
async fn dispatch_method(
    client: &Arc<tokio::sync::Mutex<Box<dyn McpClient>>>,
    method: &str,
    params: Option<Value>,
) -> Result<Value> {
    let client = client.lock().await;

    match method {
        "tools/list" => {
            let result = client.list_tools().await?;
            Ok(serde_json::to_value(&result.tools).map_err(AppError::Json)?)
        }
        "tools/call" => {
            let params =
                params.ok_or_else(|| AppError::Protocol("tools/call requires params".into()))?;
            let name = params
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| AppError::Protocol("tools/call requires params.name".into()))?;
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::new()));
            let result = client.call_tool(name, arguments).await?;
            Ok(serde_json::to_value(&result).map_err(AppError::Json)?)
        }
        "resources/list" => {
            let result = client.list_resources().await?;
            Ok(serde_json::to_value(&result.resources).map_err(AppError::Json)?)
        }
        "resources/read" => {
            let params = params
                .ok_or_else(|| AppError::Protocol("resources/read requires params".into()))?;
            let uri = params
                .get("uri")
                .and_then(|u| u.as_str())
                .ok_or_else(|| AppError::Protocol("resources/read requires params.uri".into()))?;
            let result = client.read_resource(uri).await?;
            Ok(serde_json::to_value(&result.contents).map_err(AppError::Json)?)
        }
        "prompts/list" => {
            let result = client.list_prompts().await?;
            Ok(serde_json::to_value(&result.prompts).map_err(AppError::Json)?)
        }
        "prompts/get" => {
            let params =
                params.ok_or_else(|| AppError::Protocol("prompts/get requires params".into()))?;
            let name = params
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| AppError::Protocol("prompts/get requires params.name".into()))?;
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::new()));
            let result = client.get_prompt(name, arguments).await?;
            Ok(serde_json::to_value(&result.messages).map_err(AppError::Json)?)
        }
        _ => Err(AppError::Protocol(format!("Unknown method: {method}"))),
    }
}

/// Entry point for the session daemon subprocess.
/// Called when the binary is invoked with internal `--session-daemon` flag.
pub async fn daemon_main(
    name: &str,
    source: &str,
    transport: &str,
    headers_json: &str,
    env_json: &str,
) -> Result<()> {
    let headers: HashMap<String, String> = serde_json::from_str(headers_json).unwrap_or_default();
    let env_vars: Vec<(String, String)> = serde_json::from_str(env_json).unwrap_or_default();

    // Construct the client through the shared factory so direct mode and
    // session mode honor the same transport semantics (sse, auto, env injection).
    let client = crate::mcp::client::make_client(source, transport, &headers, env_vars).await?;

    let socket_path = super::manager::session_socket_path(name);
    let daemon = SessionDaemon::new(client);
    daemon.run(&socket_path).await
}
