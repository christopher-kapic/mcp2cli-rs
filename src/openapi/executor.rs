use crate::core::types::{CommandDef, ParamLocation};
use crate::error::{AppError, Result};
use serde_json::Value;
use std::collections::HashMap;

/// Execute an OpenAPI operation.
///
/// Builds the HTTP request from the command definition and provided arguments,
/// sends it, and returns the response body as a JSON Value.
pub async fn execute_openapi(
    cmd: &CommandDef,
    args: &HashMap<String, Value>,
    base_url: &str,
    headers: &HashMap<String, String>,
) -> Result<Value> {
    let method = cmd.method.as_deref().unwrap_or("GET").to_uppercase();
    let path_template = cmd.path.as_deref().unwrap_or("/");

    // 1. Substitute path parameters
    let mut path = path_template.to_string();
    for param in &cmd.params {
        if param.location == ParamLocation::Path {
            if let Some(val) = args.get(&param.original_name) {
                let val_str = value_to_string(val);
                path = path.replace(
                    &format!("{{{}}}", param.original_name),
                    &urlencoding::encode(&val_str),
                );
            }
        }
    }

    // 2. Build query string
    let mut query_pairs: Vec<(String, String)> = Vec::new();
    for param in &cmd.params {
        if param.location == ParamLocation::Query {
            if let Some(val) = args.get(&param.original_name) {
                query_pairs.push((param.original_name.clone(), value_to_string(val)));
            }
        }
    }

    // 3. Build full URL
    let base = base_url.trim_end_matches('/');
    let mut url = format!("{base}{path}");
    if !query_pairs.is_empty() {
        let qs: String = query_pairs
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        url = format!("{url}?{qs}");
    }

    // 4. Create HTTP client and request builder
    let client = reqwest::Client::new();
    let mut req = match method.as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "PATCH" => client.patch(&url),
        "DELETE" => client.delete(&url),
        "HEAD" => client.head(&url),
        other => {
            return Err(AppError::Execution(format!(
                "Unsupported HTTP method: {other}"
            )));
        }
    };

    // 5. Apply auth/custom headers
    for (key, value) in headers {
        req = req.header(key, value);
    }

    // 6. Apply header parameters from args
    for param in &cmd.params {
        if param.location == ParamLocation::Header {
            if let Some(val) = args.get(&param.original_name) {
                req = req.header(&param.original_name, value_to_string(val));
            }
        }
    }

    // 7. Build body (JSON or multipart)
    let is_multipart = cmd
        .content_type
        .as_deref()
        .map(|ct| ct.contains("multipart"))
        .unwrap_or(false);

    if cmd.has_body {
        if is_multipart {
            // Multipart form data
            let mut form = reqwest::multipart::Form::new();
            for param in &cmd.params {
                if let Some(val) = args.get(&param.original_name) {
                    match param.location {
                        ParamLocation::File => {
                            let file_path = value_to_string(val);
                            let file_name = std::path::Path::new(&file_path)
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| "file".to_string());
                            let bytes = tokio::fs::read(&file_path).await.map_err(|e| {
                                AppError::Execution(format!(
                                    "Failed to read file '{file_path}': {e}"
                                ))
                            })?;
                            let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name);
                            form = form.part(param.original_name.clone(), part);
                        }
                        ParamLocation::Body => {
                            form = form.text(param.original_name.clone(), value_to_string(val));
                        }
                        _ => {}
                    }
                }
            }
            req = req.multipart(form);
        } else {
            // Non-object schema: a single WholeBody param carries the entire body.
            let whole_body = cmd
                .params
                .iter()
                .find(|p| p.location == ParamLocation::WholeBody)
                .and_then(|p| args.get(&p.original_name).cloned());

            if let Some(body_value) = whole_body {
                req = req
                    .header("Content-Type", "application/json")
                    .json(&body_value);
            } else {
                // Object schema: collect per-property Body params into a map.
                let mut body = serde_json::Map::new();
                for param in &cmd.params {
                    if param.location == ParamLocation::Body {
                        if let Some(val) = args.get(&param.original_name) {
                            body.insert(param.original_name.clone(), val.clone());
                        }
                    }
                }
                if !body.is_empty() {
                    req = req
                        .header("Content-Type", "application/json")
                        .json(&Value::Object(body));
                }
            }
        }
    }

    // 8. Send request
    let response = req.send().await?;
    let status = response.status();
    let body_text = response.text().await?;

    // 9. Parse response
    let body_value: Value = serde_json::from_str(&body_text).unwrap_or(Value::String(body_text));

    // 10. Handle non-2xx
    if !status.is_success() {
        return Err(AppError::Execution(format!(
            "HTTP {}: {}",
            status.as_u16(),
            serde_json::to_string(&body_value).unwrap_or_default()
        )));
    }

    Ok(body_value)
}

/// Convert a serde_json::Value to a plain string for use in URLs, headers, and form fields.
fn value_to_string(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        // Arrays and objects serialize as JSON
        _ => val.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_to_string_types() {
        assert_eq!(value_to_string(&Value::String("hello".into())), "hello");
        assert_eq!(value_to_string(&serde_json::json!(42)), "42");
        assert_eq!(value_to_string(&serde_json::json!(true)), "true");
        assert_eq!(value_to_string(&Value::Null), "");
        assert_eq!(value_to_string(&serde_json::json!([1, 2])), "[1,2]");
    }
}
