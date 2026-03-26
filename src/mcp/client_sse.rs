use crate::error::{AppError, Result};
use crate::mcp::protocol::*;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;

/// SSE MCP transport client.
///
/// Connects to an SSE endpoint via GET, discovers the message URL from the
/// event stream, then sends JSON-RPC 2.0 POST requests to that message URL.
pub struct SseMcpClient {
    url: String,
    client: Client,
    headers: HashMap<String, String>,
    message_url: Option<String>,
}

impl SseMcpClient {
    pub fn new(url: String, headers: HashMap<String, String>) -> Self {
        Self {
            url,
            client: Client::new(),
            headers: Self::merge_auth_headers(headers),
            message_url: None,
        }
    }

    /// Merge explicit headers with env-var fallback auth.
    /// Priority: explicit auth header > MCP_API_KEY > MCP_BEARER_TOKEN
    fn merge_auth_headers(mut headers: HashMap<String, String>) -> HashMap<String, String> {
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

    /// Connect to the SSE endpoint and discover the message URL from the event stream.
    async fn discover_message_url(&mut self) -> Result<()> {
        use eventsource_stream::Eventsource;

        let mut builder = self.client.get(&self.url)
            .header(reqwest::header::ACCEPT, "text/event-stream");
        for (k, v) in &self.headers {
            builder = builder.header(k, v);
        }

        let response = builder.send().await?;
        let stream = response.bytes_stream();
        let mut event_stream = stream.eventsource();

        while let Some(event_result) = event_stream.next().await {
            match event_result {
                Ok(event) => {
                    // The MCP SSE protocol sends an "endpoint" event with the message URL
                    if event.event == "endpoint" {
                        let endpoint = event.data.trim().to_string();
                        // Resolve relative URLs against the base SSE URL
                        let resolved = self.resolve_url(&endpoint)?;
                        self.message_url = Some(resolved);
                        return Ok(());
                    }
                }
                Err(e) => {
                    return Err(AppError::Protocol(format!("SSE stream error: {}", e)));
                }
            }
        }

        Err(AppError::Protocol(
            "SSE stream ended without providing a message endpoint URL".to_string(),
        ))
    }

    /// Resolve a potentially relative URL against the SSE base URL.
    fn resolve_url(&self, endpoint: &str) -> Result<String> {
        // If it's already absolute, use it as-is
        if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            return Ok(endpoint.to_string());
        }
        // Resolve relative to the SSE base URL
        let base = url::Url::parse(&self.url)
            .map_err(|e| AppError::Protocol(format!("Invalid SSE URL: {}", e)))?;
        let resolved = base
            .join(endpoint)
            .map_err(|e| AppError::Protocol(format!("Failed to resolve message URL: {}", e)))?;
        Ok(resolved.to_string())
    }

    /// Ensure the message URL has been discovered.
    fn get_message_url(&self) -> Result<&str> {
        self.message_url.as_deref().ok_or_else(|| {
            AppError::Protocol(
                "SSE message URL not yet discovered; call initialize() first".to_string(),
            )
        })
    }

    /// Send a JSON-RPC request to the discovered message URL.
    /// Handles both application/json and text/event-stream responses.
    async fn send_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let message_url = self.get_message_url()?;

        let mut builder = self.client.post(message_url)
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
                    if let Ok(rpc_response) = serde_json::from_str::<JsonRpcResponse>(&event.data) {
                        Self::check_rpc_error(&rpc_response)?;
                        return Ok(rpc_response);
                    }
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

    fn extract_result<T: serde::de::DeserializeOwned>(response: JsonRpcResponse) -> Result<T> {
        let result_value = response
            .result
            .ok_or_else(|| AppError::Protocol("Missing result in JSON-RPC response".to_string()))?;
        serde_json::from_value(result_value)
            .map_err(|e| AppError::Protocol(format!("Failed to deserialize result: {}", e)))
    }
}

#[async_trait::async_trait]
impl McpClient for SseMcpClient {
    async fn initialize(&mut self) -> Result<()> {
        // Step 1: Connect to SSE endpoint and discover the message URL
        self.discover_message_url().await?;

        // Step 2: Send initialize request to the message URL
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
