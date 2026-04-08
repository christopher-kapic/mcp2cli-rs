use crate::error::{AppError, Result};
use crate::mcp::protocol::*;
use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// Stdio MCP transport client — spawns a child process and communicates via
/// line-delimited JSON-RPC over stdin/stdout.
pub struct StdioMcpClient {
    program: String,
    args: Vec<String>,
    env_vars: Vec<(String, String)>,
    child: Arc<Mutex<Option<ChildProcess>>>,
}

struct ChildProcess {
    stdin: tokio::process::ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
    _child: Child,
}

impl StdioMcpClient {
    pub fn new(command: String) -> Self {
        Self::with_env(command, vec![])
    }

    /// Create a new stdio client with additional environment variables to inject
    /// into the child process.
    pub fn with_env(command: String, env_vars: Vec<(String, String)>) -> Self {
        let parts: Vec<&str> = command.split_whitespace().collect();
        let (program, args) = if parts.is_empty() {
            (command.clone(), vec![])
        } else {
            (
                parts[0].to_string(),
                parts[1..].iter().map(|s| s.to_string()).collect(),
            )
        };
        Self {
            program,
            args,
            env_vars,
            child: Arc::new(Mutex::new(None)),
        }
    }

    /// Ensure the child process is running. Spawns it on first use.
    async fn ensure_started(&self) -> Result<()> {
        let mut guard = self.child.lock().await;
        if guard.is_some() {
            return Ok(());
        }

        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        // Inject --env variables into the child process environment
        for (key, value) in &self.env_vars {
            cmd.env(key, value);
        }
        let mut child = cmd.spawn().map_err(|e| {
            AppError::Execution(format!("Failed to spawn '{}': {}", self.program, e))
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Execution("Failed to capture child stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Execution("Failed to capture child stdout".to_string()))?;

        *guard = Some(ChildProcess {
            stdin,
            reader: BufReader::new(stdout),
            _child: child,
        });
        Ok(())
    }

    /// Send a JSON-RPC request and read the matching response by request ID.
    async fn send_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        self.ensure_started().await?;
        let mut guard = self.child.lock().await;
        let proc = guard
            .as_mut()
            .ok_or_else(|| AppError::Execution("Child process not running".to_string()))?;

        let request_id = request.id.clone();

        // Write request as a single line of JSON followed by newline
        let mut payload = serde_json::to_string(&request)?;
        payload.push('\n');
        proc.stdin.write_all(payload.as_bytes()).await?;
        proc.stdin.flush().await?;

        // Read lines until we find a response matching our request ID
        let mut line = String::new();
        loop {
            line.clear();
            let n = proc.reader.read_line(&mut line).await?;
            if n == 0 {
                return Err(AppError::Protocol(
                    "Child process closed stdout before responding".to_string(),
                ));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try to parse as a JSON-RPC response
            if let Ok(rpc_response) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                // Match by request ID
                if rpc_response.id == Some(request_id.clone()) {
                    Self::check_rpc_error(&rpc_response)?;
                    return Ok(rpc_response);
                }
                // Non-matching ID — could be a notification; skip it
            }
            // Non-JSON lines (e.g. logging) are silently skipped
        }
    }

    fn check_rpc_error(rpc_response: &JsonRpcResponse) -> Result<()> {
        if let Some(ref err) = rpc_response.error {
            return Err(AppError::Protocol(format!(
                "JSON-RPC error {}: {}",
                err.code, err.message
            )));
        }
        Ok(())
    }

    fn extract_result<T: serde::de::DeserializeOwned>(response: JsonRpcResponse) -> Result<T> {
        let result_value = response
            .result
            .ok_or_else(|| AppError::Protocol("Missing result in JSON-RPC response".to_string()))?;
        serde_json::from_value(result_value)
            .map_err(|e| AppError::Protocol(format!("Failed to deserialize result: {}", e)))
    }
}

impl Drop for StdioMcpClient {
    fn drop(&mut self) {
        // Best-effort kill of the child process
        if let Ok(mut guard) = self.child.try_lock() {
            if let Some(mut proc) = guard.take() {
                // start_kill is non-blocking
                let _ = proc._child.start_kill();
            }
        }
    }
}

#[async_trait::async_trait]
impl McpClient for StdioMcpClient {
    async fn initialize(&mut self) -> Result<()> {
        let request = JsonRpcRequest::new(
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {
                    "name": "mcp2cli",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        );
        self.send_request(request).await?;
        Ok(())
    }

    async fn list_tools(&self) -> Result<ListToolsResult> {
        let request = JsonRpcRequest::new("tools/list", None);
        let response = self.send_request(request).await?;
        Self::extract_result(response)
    }

    async fn call_tool(&self, name: &str, arguments: Value) -> Result<CallToolResult> {
        let request = JsonRpcRequest::new(
            "tools/call",
            Some(serde_json::json!({
                "name": name,
                "arguments": arguments
            })),
        );
        let response = self.send_request(request).await?;
        Self::extract_result(response)
    }

    async fn list_resources(&self) -> Result<ListResourcesResult> {
        let request = JsonRpcRequest::new("resources/list", None);
        let response = self.send_request(request).await?;
        Self::extract_result(response)
    }

    async fn list_resource_templates(&self) -> Result<ListResourceTemplatesResult> {
        let request = JsonRpcRequest::new("resources/templates/list", None);
        let response = self.send_request(request).await?;
        Self::extract_result(response)
    }

    async fn read_resource(&self, uri: &str) -> Result<ReadResourceResult> {
        let request =
            JsonRpcRequest::new("resources/read", Some(serde_json::json!({ "uri": uri })));
        let response = self.send_request(request).await?;
        Self::extract_result(response)
    }

    async fn list_prompts(&self) -> Result<ListPromptsResult> {
        let request = JsonRpcRequest::new("prompts/list", None);
        let response = self.send_request(request).await?;
        Self::extract_result(response)
    }

    async fn get_prompt(&self, name: &str, arguments: Value) -> Result<GetPromptResult> {
        let request = JsonRpcRequest::new(
            "prompts/get",
            Some(serde_json::json!({
                "name": name,
                "arguments": arguments
            })),
        );
        let response = self.send_request(request).await?;
        Self::extract_result(response)
    }
}
