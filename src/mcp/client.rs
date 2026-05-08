use std::collections::HashMap;

use crate::error::{AppError, Result};
use crate::mcp::client_http::HttpMcpClient;
use crate::mcp::client_sse::SseMcpClient;
use crate::mcp::client_stdio::StdioMcpClient;
use crate::mcp::protocol::McpClient;

/// Construct an MCP client for the given transport.
///
/// Single source of truth for transport selection — used by direct mode and
/// the session daemon so they cannot drift apart.
///
/// `env_vars` is only meaningful for `stdio`; HTTP/SSE transports ignore it.
pub async fn make_client(
    source: &str,
    transport: &str,
    headers: &HashMap<String, String>,
    env_vars: Vec<(String, String)>,
) -> Result<Box<dyn McpClient>> {
    match transport {
        "stdio" => Ok(Box::new(StdioMcpClient::with_env(
            source.to_string(),
            env_vars,
        )?)),
        "streamable" => Ok(Box::new(HttpMcpClient::new(
            source.to_string(),
            headers.clone(),
        ))),
        "sse" => Ok(Box::new(SseMcpClient::new(
            source.to_string(),
            headers.clone(),
        ))),
        "auto" => {
            // Probe streamable HTTP; on any error fall back to SSE.
            let mut http = HttpMcpClient::new(source.to_string(), headers.clone());
            match http.initialize().await {
                Ok(()) => Ok(Box::new(http)),
                Err(_) => Ok(Box::new(SseMcpClient::new(
                    source.to_string(),
                    headers.clone(),
                ))),
            }
        }
        other => Err(AppError::Cli(format!(
            "unknown transport: {other}. Use auto, sse, streamable, or stdio"
        ))),
    }
}
