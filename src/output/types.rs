use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Represents a single piece of content in a tool response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Content {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "json")]
    Json { data: Value },
    #[serde(rename = "image")]
    Image { data: String, mime_type: String },
    #[serde(rename = "error")]
    Error { detail: ErrorDetail },
}

/// Details about an error in a tool response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDetail {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// Metadata about a tool response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Complete tool response with content and optional metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResponse {
    pub content: Vec<Content>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ResponseMetadata>,
}

/// Manifest entry for a single tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifestEntry {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A list of available tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    pub tools: Vec<ToolManifestEntry>,
}

/// Options controlling output formatting.
#[derive(Debug, Clone, Default)]
pub struct OutputOptions {
    /// Force pretty-print even when not a TTY.
    pub pretty: bool,
    /// Raw output (no JSON formatting, print text as-is).
    pub raw: bool,
    /// Pipe through toon formatter.
    pub toon: bool,
    /// Pipe through jq with this expression.
    pub jq: Option<String>,
    /// Truncate arrays to this many items.
    pub head: Option<usize>,
}
