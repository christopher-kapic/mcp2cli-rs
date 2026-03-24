use crate::error::{AppError, Result};
use crate::mcp::protocol::*;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// MCP client that communicates with a session daemon over a Unix socket.
pub struct SessionMcpClient {
    socket_path: std::path::PathBuf,
}

impl SessionMcpClient {
    pub fn new(socket_path: std::path::PathBuf) -> Self {
        Self { socket_path }
    }

    /// Send a JSON-RPC request to the daemon and return the result.
    async fn send_request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            AppError::Protocol(format!(
                "Failed to connect to session at {}: {e}",
                self.socket_path.display()
            ))
        })?;

        let (reader, mut writer) = stream.into_split();

        let request = JsonRpcRequest::new(method, params);
        let mut request_json = serde_json::to_string(&request)?;
        request_json.push('\n');
        writer.write_all(request_json.as_bytes()).await?;

        // Shut down the write half so the daemon knows we're done sending
        drop(writer);

        let mut lines = BufReader::new(reader).lines();
        if let Some(line) = lines.next_line().await? {
            let response: Value = serde_json::from_str(&line)?;

            if let Some(error) = response.get("error") {
                let message = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error");
                return Err(AppError::Protocol(format!(
                    "Session daemon error: {message}"
                )));
            }

            if let Some(result) = response.get("result") {
                return Ok(result.clone());
            }

            Err(AppError::Protocol(
                "Session daemon returned response with no result or error".into(),
            ))
        } else {
            Err(AppError::Protocol(
                "Session daemon closed connection without response".into(),
            ))
        }
    }
}

#[async_trait::async_trait]
impl McpClient for SessionMcpClient {
    async fn initialize(&mut self) -> Result<()> {
        // The daemon initializes the client on startup; nothing to do here.
        Ok(())
    }

    async fn list_tools(&self) -> Result<ListToolsResult> {
        let value = self.send_request("tools/list", None).await?;
        let tools: Vec<McpTool> = serde_json::from_value(value)?;
        Ok(ListToolsResult { tools })
    }

    async fn call_tool(&self, name: &str, arguments: Value) -> Result<CallToolResult> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments
        });
        let value = self.send_request("tools/call", Some(params)).await?;
        let result: CallToolResult = serde_json::from_value(value)?;
        Ok(result)
    }

    async fn list_resources(&self) -> Result<ListResourcesResult> {
        let value = self.send_request("resources/list", None).await?;
        let resources: Vec<McpResource> = serde_json::from_value(value)?;
        Ok(ListResourcesResult { resources })
    }

    async fn list_resource_templates(&self) -> Result<ListResourceTemplatesResult> {
        let value = self.send_request("resources/templates/list", None).await?;
        let resource_templates: Vec<ResourceTemplate> = serde_json::from_value(value)?;
        Ok(ListResourceTemplatesResult { resource_templates })
    }

    async fn read_resource(&self, uri: &str) -> Result<ReadResourceResult> {
        let params = serde_json::json!({ "uri": uri });
        let value = self.send_request("resources/read", Some(params)).await?;
        let contents: Vec<Value> = serde_json::from_value(value)?;
        Ok(ReadResourceResult { contents })
    }

    async fn list_prompts(&self) -> Result<ListPromptsResult> {
        let value = self.send_request("prompts/list", None).await?;
        let prompts: Vec<McpPrompt> = serde_json::from_value(value)?;
        Ok(ListPromptsResult { prompts })
    }

    async fn get_prompt(&self, name: &str, arguments: Value) -> Result<GetPromptResult> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments
        });
        let value = self.send_request("prompts/get", Some(params)).await?;
        let messages: Vec<PromptMessage> = serde_json::from_value(value)?;
        Ok(GetPromptResult { messages })
    }
}
