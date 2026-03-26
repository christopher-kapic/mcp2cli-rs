use crate::error::{AppError, Result};
use crate::mcp::protocol::*;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;

/// Streamable HTTP MCP transport client.
///
/// Sends JSON-RPC 2.0 POST requests to the MCP endpoint URL.
/// Handles both `application/json` and `text/event-stream` (SSE) responses.
pub struct HttpMcpClient {
    url: String,
    client: Client,
    headers: HashMap<String, String>,
}

impl HttpMcpClient {
    pub fn new(url: String, headers: HashMap<String, String>) -> Self {
        Self {
            url,
            client: Client::new(),
            headers: Self::merge_auth_headers(headers),
        }
    }

    /// Merge explicit headers with env-var fallback auth.
    /// Priority: explicit auth header > MCP_API_KEY > MCP_BEARER_TOKEN
    fn merge_auth_headers(mut headers: HashMap<String, String>) -> HashMap<String, String> {
        // If there's already an Authorization header, use it as-is
        let has_auth = headers
            .keys()
            .any(|k| k.eq_ignore_ascii_case("authorization"));
        if !has_auth {
            if let Ok(api_key) = std::env::var("MCP_API_KEY") {
                headers.insert("Authorization".to_string(), format!("Bearer {}", api_key));
            } else if let Ok(token) = std::env::var("MCP_BEARER_TOKEN") {
                headers.insert("Authorization".to_string(), format!("Bearer {}", token));
            }
        }
        headers
    }

    /// Send a JSON-RPC request and return the parsed response.
    /// Handles both application/json and text/event-stream content types.
    async fn send_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let mut builder = self.client.post(&self.url)
            .header(reqwest::header::ACCEPT, "application/json, text/event-stream")
            .json(&request);
        for (k, v) in &self.headers {
            builder = builder.header(k, v);
        }

        let response = builder.send().await?;

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if content_type.contains("text/event-stream") {
            self.parse_sse_response(response).await
        } else {
            // Default: application/json
            let rpc_response: JsonRpcResponse = response.json().await?;
            Self::check_rpc_error(&rpc_response)?;
            Ok(rpc_response)
        }
    }

    /// Parse a Server-Sent Events response stream and extract the JSON-RPC response.
    async fn parse_sse_response(&self, response: reqwest::Response) -> Result<JsonRpcResponse> {
        use eventsource_stream::Eventsource;
        let stream = response.bytes_stream();
        let mut event_stream = stream.eventsource();

        while let Some(event_result) = event_stream.next().await {
            match event_result {
                Ok(event) => {
                    // Try to parse the data field as a JSON-RPC response
                    if let Ok(rpc_response) = serde_json::from_str::<JsonRpcResponse>(&event.data) {
                        Self::check_rpc_error(&rpc_response)?;
                        return Ok(rpc_response);
                    }
                    // Non-JSON events (e.g. ping) are skipped
                }
                Err(e) => {
                    return Err(AppError::Protocol(format!("SSE stream error: {}", e)));
                }
            }
        }

        Err(AppError::Protocol(
            "SSE stream ended without a JSON-RPC response".to_string(),
        ))
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

    /// Extract the `result` field from a JSON-RPC response, deserializing into T.
    fn extract_result<T: serde::de::DeserializeOwned>(response: JsonRpcResponse) -> Result<T> {
        let result_value = response
            .result
            .ok_or_else(|| AppError::Protocol("Missing result in JSON-RPC response".to_string()))?;
        serde_json::from_value(result_value)
            .map_err(|e| AppError::Protocol(format!("Failed to deserialize result: {}", e)))
    }
}

#[async_trait::async_trait]
impl McpClient for HttpMcpClient {
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
        // Send initialize; we don't need the result beyond confirming no error
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
