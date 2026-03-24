use crate::core::helpers::to_kebab;
use crate::core::types::{CommandDef, ParamDef, ParamLocation};
use crate::mcp::protocol::McpTool;

/// Extract CommandDefs from MCP tools.
pub fn extract_mcp_commands(tools: &[McpTool]) -> Vec<CommandDef> {
    tools
        .iter()
        .map(|tool| {
            let params = extract_params(tool);
            let has_body = false;
            CommandDef {
                name: to_kebab(&tool.name),
                description: tool.description.clone().unwrap_or_default(),
                params,
                has_body,
                method: None,
                path: None,
                content_type: None,
                tool_name: Some(tool.name.clone()),
                graphql_operation_type: None,
                graphql_field_name: None,
                graphql_return_type: None,
            }
        })
        .collect()
}

fn extract_params(tool: &McpTool) -> Vec<ParamDef> {
    let schema = match &tool.input_schema {
        Some(s) => s,
        None => return vec![],
    };

    let properties = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return vec![],
    };

    let required: Vec<String> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    properties
        .iter()
        .map(|(name, prop_schema)| {
            let rust_type = crate::core::coerce::schema_type_to_rust(prop_schema);
            let description = prop_schema
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let choices = prop_schema
                .get("enum")
                .and_then(|e| e.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                });

            ParamDef {
                name: to_kebab(name),
                original_name: name.clone(),
                rust_type,
                required: required.contains(name),
                description,
                choices,
                location: ParamLocation::ToolInput,
                schema: prop_schema.clone(),
            }
        })
        .collect()
}
