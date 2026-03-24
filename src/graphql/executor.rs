use std::collections::HashMap;

use serde_json::{json, Value};

use crate::core::types::CommandDef;
use crate::error::{AppError, Result};
use crate::graphql::document::build_graphql_document;
use crate::graphql::introspection::IntrospectionSchema;
use crate::output::format::output_result;
use crate::output::types::OutputOptions;

/// Execute a GraphQL operation.
///
/// Builds the document from the command and arguments, POSTs to the endpoint,
/// extracts the data field, and outputs the result.
pub async fn execute_graphql(
    cmd: &CommandDef,
    args: &HashMap<String, Value>,
    url: &str,
    schema: &IntrospectionSchema,
    headers: &[(String, String)],
    fields_override: Option<&str>,
    output_opts: &OutputOptions,
) -> Result<()> {
    let (document, variables) = build_graphql_document(cmd, args, schema, fields_override);

    let client = reqwest::Client::new();
    let mut builder = client.post(url);
    for (k, v) in headers {
        builder = builder.header(k, v);
    }

    let body = json!({
        "query": document,
        "variables": variables,
    });

    let resp = builder.json(&body).send().await?;
    let status = resp.status();

    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Execution(format!(
            "GraphQL request failed with status {status}: {text}"
        )));
    }

    let resp_json: Value = resp.json().await?;

    // Check for GraphQL errors
    if let Some(errors) = resp_json.get("errors") {
        if let Some(arr) = errors.as_array() {
            if !arr.is_empty() {
                // If there's no data, report errors as failure
                if resp_json.get("data").map_or(true, |d| d.is_null()) {
                    let messages: Vec<String> = arr
                        .iter()
                        .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                        .map(String::from)
                        .collect();
                    return Err(AppError::Execution(format!(
                        "GraphQL errors: {}",
                        messages.join("; ")
                    )));
                }
                // If there's data AND errors, print errors to stderr and continue
                for err in arr {
                    if let Some(msg) = err.get("message").and_then(|m| m.as_str()) {
                        eprintln!("GraphQL warning: {msg}");
                    }
                }
            }
        }
    }

    // Extract data.<fieldName>
    let field_name = cmd.graphql_field_name.as_deref().unwrap_or(&cmd.name);

    let result = resp_json
        .get("data")
        .and_then(|d| d.get(field_name))
        .cloned()
        .unwrap_or_else(|| {
            // Fall back to the entire data object if field not found
            resp_json.get("data").cloned().unwrap_or(Value::Null)
        });

    output_result(&result, output_opts)?;
    Ok(())
}
