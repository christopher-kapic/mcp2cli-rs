use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamType {
    String,
    Int,
    Float,
    Bool,
    Array,
    Object,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamLocation {
    Path,
    Query,
    Header,
    Body,
    /// The entire request body, used when the schema is non-object (array or
    /// primitive). Only one such param exists per command.
    WholeBody,
    ToolInput,
    GraphqlArg,
    File,
}

#[derive(Debug, Clone)]
pub struct ParamDef {
    pub name: String,
    pub original_name: String,
    pub rust_type: ParamType,
    pub required: bool,
    pub description: String,
    pub choices: Option<Vec<String>>,
    pub location: ParamLocation,
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct CommandDef {
    pub name: String,
    pub description: String,
    pub params: Vec<ParamDef>,
    pub has_body: bool,
    // OpenAPI-specific
    pub method: Option<String>,
    pub path: Option<String>,
    pub content_type: Option<String>,
    // MCP-specific
    pub tool_name: Option<String>,
    // GraphQL-specific
    pub graphql_operation_type: Option<String>,
    pub graphql_field_name: Option<String>,
    pub graphql_return_type: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BakeConfig {
    pub source_type: String,
    pub source: String,
    #[serde(default)]
    pub auth_headers: Vec<(String, String)>,
    #[serde(default)]
    pub env_vars: HashMap<String, String>,
    #[serde(default)]
    pub cache_ttl: Option<u64>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub oauth: Option<bool>,
    #[serde(default)]
    pub oauth_client_id: Option<String>,
    #[serde(default)]
    pub oauth_client_secret: Option<String>,
    #[serde(default)]
    pub oauth_scope: Option<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}
