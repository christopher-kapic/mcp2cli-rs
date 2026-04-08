use std::collections::HashMap;

use serde_json::Value;

use crate::cache::file_cache;
use crate::cli::dynamic::parse_dynamic_args;
use crate::core::coerce::coerce_value;
use crate::core::filter::filter_commands;
use crate::error::{AppError, Result};
use crate::mcp::commands::extract_mcp_commands;
use crate::mcp::protocol::McpClient;
use crate::output::format::output_result;
use crate::output::types::OutputOptions;

/// Options for the MCP handler, bundling all CLI-derived configuration.
pub struct McpHandlerOptions {
    pub url: String,
    pub transport: String,
    pub headers: HashMap<String, String>,
    pub output_opts: OutputOptions,
    pub list: bool,
    pub search: Option<String>,
    pub rest: Vec<String>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub cache_key: Option<String>,
    pub cache_ttl: u64,
    pub refresh: bool,
    // Resource flags
    pub list_resources: bool,
    pub list_resource_templates: bool,
    pub read_resource: Option<String>,
    // Prompt flags
    pub list_prompts: bool,
    pub get_prompt: Option<String>,
    pub prompt_args: HashMap<String, String>,
    // OAuth
    pub oauth_provider: Option<Box<dyn crate::oauth::provider::OAuthProvider>>,
    // Session: pre-built client that bypasses transport selection
    pub session_client: Option<Box<dyn crate::mcp::protocol::McpClient>>,
    // Environment variables to inject into stdio subprocess
    pub env_vars: Vec<(String, String)>,
}

/// Main MCP handler — orchestrates transport selection, tool listing, and calling.
pub async fn handle_mcp(mut opts: McpHandlerOptions) -> Result<()> {
    // 0. Inject OAuth auth header if provider is present
    if let Some(ref provider) = opts.oauth_provider {
        let header = provider.get_auth_header().await?;
        opts.headers.insert("Authorization".to_string(), header);
    }

    // 1. Create appropriate MCP client based on transport mode (or use session client)
    let mut client = if let Some(session_client) = opts.session_client.take() {
        session_client
    } else {
        create_client(
            &opts.url,
            &opts.transport,
            &opts.headers,
            opts.env_vars.clone(),
        )
        .await?
    };

    // 2. Initialize the connection
    client.initialize().await?;

    // 2a. Handle resource flags (before tool handling)
    if opts.list_resources {
        let result = client.list_resources().await?;
        let json = serde_json::to_value(&result.resources)?;
        return output_result(&json, &opts.output_opts);
    }

    if opts.list_resource_templates {
        let result = client.list_resource_templates().await?;
        let json = serde_json::to_value(&result.resource_templates)?;
        return output_result(&json, &opts.output_opts);
    }

    if let Some(ref uri) = opts.read_resource {
        let result = client.read_resource(uri).await?;
        let json = serde_json::to_value(&result.contents)?;
        return output_result(&json, &opts.output_opts);
    }

    // 2b. Handle prompt flags (before tool handling)
    if opts.list_prompts {
        let result = client.list_prompts().await?;
        let json = serde_json::to_value(&result.prompts)?;
        return output_result(&json, &opts.output_opts);
    }

    if let Some(ref prompt_name) = opts.get_prompt {
        let args_value = serde_json::to_value(&opts.prompt_args)?;
        let result = client.get_prompt(prompt_name, args_value).await?;
        let json = serde_json::to_value(&result.messages)?;
        return output_result(&json, &opts.output_opts);
    }

    // 3. Fetch tools (with cache)
    let cache_key = opts
        .cache_key
        .unwrap_or_else(|| file_cache::cache_key_for(&opts.url));
    let ttl = opts.cache_ttl;

    let tools_value = if !opts.refresh {
        file_cache::load_cached(&cache_key, ttl).await
    } else {
        None
    };

    let tools_value = match tools_value {
        Some(v) => v,
        None => {
            let result = client.list_tools().await?;
            let v = serde_json::to_value(&result.tools)?;
            let _ = file_cache::save_cache(&cache_key, &v).await;
            v
        }
    };

    // Deserialize tools from cache or fresh fetch
    let tools: Vec<crate::mcp::protocol::McpTool> = serde_json::from_value(tools_value)?;

    // 4. Extract commands and apply filters
    let commands = extract_mcp_commands(&tools);
    let commands = filter_commands(commands, &opts.include, &opts.exclude, &[]);

    // 5. Handle --list
    if opts.list {
        return display_tool_list(&commands, &opts.output_opts);
    }

    // 6. Handle --search
    if let Some(ref keyword) = opts.search {
        let keyword_lower = keyword.to_lowercase();
        let filtered: Vec<_> = commands
            .iter()
            .filter(|cmd| {
                cmd.name.to_lowercase().contains(&keyword_lower)
                    || cmd.description.to_lowercase().contains(&keyword_lower)
            })
            .cloned()
            .collect();
        return display_tool_list(&filtered, &opts.output_opts);
    }

    // 7. Dynamic arg parsing -> tool call -> output
    if opts.rest.is_empty() {
        return Err(AppError::Cli(
            "No tool specified. Use --list to see available tools.".into(),
        ));
    }

    let parsed = parse_dynamic_args(&commands, &opts.rest)?;

    // Find the matching command to get the original tool name and param schemas
    let cmd = commands
        .iter()
        .find(|c| c.name == parsed.command)
        .ok_or_else(|| AppError::Cli(format!("unknown tool: {}", parsed.command)))?;

    let tool_name = cmd.tool_name.as_ref().unwrap_or(&cmd.name);

    // Coerce args to typed JSON values based on param schemas
    let mut arguments = serde_json::Map::new();

    // If --stdin was passed, read JSON from stdin and merge as body
    if parsed.stdin {
        let stdin_value = crate::cli::stdin::read_stdin_json()?;
        if let Value::Object(map) = stdin_value {
            for (k, v) in map {
                arguments.insert(k, v);
            }
        } else {
            return Err(AppError::Cli(
                "--stdin expects a JSON object from stdin".into(),
            ));
        }
    }

    for (key, value) in &parsed.args {
        // Find the param definition to get original name and schema
        let param = cmd.params.iter().find(|p| p.name == *key);
        if let Some(param) = param {
            let coerced = coerce_value(value, &param.schema)?;
            arguments.insert(param.original_name.clone(), coerced);
        } else {
            // Unknown param — pass as string
            arguments.insert(key.clone(), Value::String(value.clone()));
        }
    }

    let result = client
        .call_tool(tool_name, Value::Object(arguments))
        .await?;

    // Convert CallToolResult to output JSON
    let output = call_result_to_value(&result);

    output_result(&output, &opts.output_opts)
}

/// Create the appropriate MCP client based on transport mode.
async fn create_client(
    url: &str,
    transport: &str,
    headers: &HashMap<String, String>,
    env_vars: Vec<(String, String)>,
) -> Result<Box<dyn McpClient>> {
    match transport {
        "stdio" => Ok(Box::new(
            crate::mcp::client_stdio::StdioMcpClient::with_env(url.to_string(), env_vars),
        )),
        "streamable" => Ok(Box::new(crate::mcp::client_http::HttpMcpClient::new(
            url.to_string(),
            headers.clone(),
        ))),
        "sse" => Ok(Box::new(crate::mcp::client_sse::SseMcpClient::new(
            url.to_string(),
            headers.clone(),
        ))),
        "auto" => {
            // Try streamable HTTP first, fall back to SSE on error
            let mut http_client =
                crate::mcp::client_http::HttpMcpClient::new(url.to_string(), headers.clone());
            match http_client.initialize().await {
                Ok(()) => Ok(Box::new(http_client)),
                Err(_) => {
                    let client =
                        crate::mcp::client_sse::SseMcpClient::new(url.to_string(), headers.clone());
                    Ok(Box::new(client))
                }
            }
        }
        other => Err(AppError::Cli(format!(
            "unknown transport: {other}. Use auto, sse, streamable, or stdio"
        ))),
    }
}

/// Display a list of tools in a user-friendly format.
fn display_tool_list(
    commands: &[crate::core::types::CommandDef],
    output_opts: &OutputOptions,
) -> Result<()> {
    let tools_json: Vec<Value> = commands
        .iter()
        .map(|cmd| {
            let params: Vec<Value> = cmd
                .params
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "name": p.name,
                        "required": p.required,
                        "description": p.description,
                    })
                })
                .collect();

            serde_json::json!({
                "name": cmd.name,
                "description": cmd.description,
                "params": params,
            })
        })
        .collect();

    output_result(&Value::Array(tools_json), output_opts)
}

/// Convert a CallToolResult into a serde_json::Value for output.
fn call_result_to_value(result: &crate::mcp::protocol::CallToolResult) -> Value {
    if result.content.len() == 1 {
        content_to_value(&result.content[0])
    } else {
        let items: Vec<Value> = result.content.iter().map(content_to_value).collect();
        Value::Array(items)
    }
}

fn content_to_value(content: &crate::mcp::protocol::McpContent) -> Value {
    match content {
        crate::mcp::protocol::McpContent::Text { text } => {
            // Try to parse as JSON for structured output
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()))
        }
        crate::mcp::protocol::McpContent::Image { data, mime_type } => {
            serde_json::json!({
                "type": "image",
                "data": data,
                "mimeType": mime_type,
            })
        }
        crate::mcp::protocol::McpContent::Resource { resource } => resource.clone(),
    }
}
