use crate::core::coerce::schema_type_to_rust;
use crate::core::types::{CommandDef, ParamDef, ParamLocation};
use serde_json::Value;
use std::collections::HashSet;

/// Sentinel `original_name` used by the synthetic `--body` parameter that
/// represents a non-object request body. The executor recognizes this key to
/// send the value as the entire JSON body rather than a single property.
pub(crate) const WHOLE_BODY_KEY: &str = "__mcp2cli_body__";

/// Extract CommandDefs from an OpenAPI spec.
/// The spec should already have $refs resolved before calling this.
pub fn extract_openapi_commands(spec: &Value) -> Vec<CommandDef> {
    let mut commands = Vec::new();
    let paths = match spec.get("paths").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return commands,
    };

    let mut seen_names: HashSet<String> = HashSet::new();

    for (path, path_item) in paths {
        let path_obj = match path_item.as_object() {
            Some(o) => o,
            None => continue,
        };

        // Collect path-level parameters (shared by all operations on this path)
        let path_level_params = path_obj
            .get("parameters")
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        for (method, operation) in path_obj {
            if !is_http_method(method) {
                continue;
            }
            if let Some(cmd) =
                build_command(method, path, operation, &path_level_params, &mut seen_names)
            {
                commands.push(cmd);
            }
        }
    }

    commands
}

fn is_http_method(s: &str) -> bool {
    matches!(
        s,
        "get" | "post" | "put" | "patch" | "delete" | "head" | "options"
    )
}

/// Deduplicate a command name by appending a numeric suffix if needed.
fn deduplicate_name(name: String, seen: &mut HashSet<String>) -> String {
    if seen.insert(name.clone()) {
        return name;
    }
    let mut i = 2;
    loop {
        let candidate = format!("{name}-{i}");
        if seen.insert(candidate.clone()) {
            return candidate;
        }
        i += 1;
    }
}

fn build_command(
    method: &str,
    path: &str,
    operation: &Value,
    path_level_params: &[Value],
    seen_names: &mut HashSet<String>,
) -> Option<CommandDef> {
    let raw_name = operation
        .get("operationId")
        .and_then(|v| v.as_str())
        .map(crate::core::helpers::to_kebab)
        .unwrap_or_else(|| {
            let slug = path
                .trim_matches('/')
                .replace('/', "-")
                .replace(['{', '}'], "");
            format!("{method}-{slug}")
        });

    let name = deduplicate_name(raw_name, seen_names);

    let description = operation
        .get("summary")
        .or_else(|| operation.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Collect operation-level parameters
    let op_params = operation
        .get("parameters")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();

    // Merge path-level and operation-level params (operation overrides path-level by name+in)
    let mut params = extract_parameters(path_level_params, &op_params);

    // Extract request body parameters
    let has_body = operation.get("requestBody").is_some();
    let content_type = extract_content_type(operation);
    let is_multipart = content_type
        .as_deref()
        .map(|ct| ct.contains("multipart"))
        .unwrap_or(false);

    if has_body {
        let body_params = extract_body_params(operation, is_multipart);
        params.extend(body_params);
    }

    Some(CommandDef {
        name,
        description,
        params,
        has_body,
        method: Some(method.to_uppercase()),
        path: Some(path.to_string()),
        content_type,
        tool_name: None,
        graphql_operation_type: None,
        graphql_field_name: None,
        graphql_return_type: None,
    })
}

/// Extract the preferred content type from a requestBody.
fn extract_content_type(operation: &Value) -> Option<String> {
    let content = operation
        .get("requestBody")
        .and_then(|rb| rb.get("content"))
        .and_then(|c| c.as_object())?;

    // Prefer multipart, then application/json, then first available
    if content.contains_key("multipart/form-data") {
        Some("multipart/form-data".to_string())
    } else if content.contains_key("application/json") {
        Some("application/json".to_string())
    } else {
        content.keys().next().map(String::from)
    }
}

/// Merge path-level and operation-level parameters.
/// Operation params override path-level params with the same name+location.
fn extract_parameters(path_params: &[Value], op_params: &[Value]) -> Vec<ParamDef> {
    let mut merged: Vec<&Value> = Vec::new();
    let mut op_keys: HashSet<(String, String)> = HashSet::new();

    // Add operation params first (they take priority)
    for p in op_params {
        let key = param_key(p);
        op_keys.insert(key);
        merged.push(p);
    }

    // Add path-level params that aren't overridden
    for p in path_params {
        let key = param_key(p);
        if !op_keys.contains(&key) {
            merged.push(p);
        }
    }

    merged.into_iter().filter_map(param_value_to_def).collect()
}

fn param_key(p: &Value) -> (String, String) {
    let name = p
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let loc = p
        .get("in")
        .and_then(|i| i.as_str())
        .unwrap_or("")
        .to_string();
    (name, loc)
}

/// Convert a single OpenAPI parameter JSON value to a ParamDef.
fn param_value_to_def(p: &Value) -> Option<ParamDef> {
    let original_name = p.get("name").and_then(|n| n.as_str())?.to_string();
    let name = crate::core::helpers::to_kebab(&original_name);
    let location = match p.get("in").and_then(|i| i.as_str())? {
        "path" => ParamLocation::Path,
        "query" => ParamLocation::Query,
        "header" => ParamLocation::Header,
        _ => return None,
    };
    let required = p.get("required").and_then(|r| r.as_bool()).unwrap_or(false)
        || location == ParamLocation::Path; // path params are always required
    let description = p
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("")
        .to_string();
    let schema = p
        .get("schema")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));
    let rust_type = schema_type_to_rust(&schema);
    let choices = extract_enum_choices(&schema);

    Some(ParamDef {
        name,
        original_name,
        rust_type,
        required,
        description,
        choices,
        location,
        schema,
    })
}

/// Extract enum choices from a schema (supports top-level enum and allOf/oneOf with enum).
fn extract_enum_choices(schema: &Value) -> Option<Vec<String>> {
    // Direct enum
    if let Some(arr) = schema.get("enum").and_then(|e| e.as_array()) {
        let choices: Vec<String> = arr
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                Value::Bool(b) => Some(b.to_string()),
                _ => None,
            })
            .collect();
        if !choices.is_empty() {
            return Some(choices);
        }
    }
    None
}

/// Extract parameters from a request body schema.
fn extract_body_params(operation: &Value, is_multipart: bool) -> Vec<ParamDef> {
    let content_type_key = if is_multipart {
        "multipart/form-data"
    } else {
        "application/json"
    };

    let schema = match operation
        .get("requestBody")
        .and_then(|rb| rb.get("content"))
        .and_then(|c| c.get(content_type_key))
        .and_then(|ct| ct.get("schema"))
    {
        Some(s) => s,
        None => {
            // Try first available content type
            let schema = operation
                .get("requestBody")
                .and_then(|rb| rb.get("content"))
                .and_then(|c| c.as_object())
                .and_then(|m| m.values().next())
                .and_then(|ct| ct.get("schema"));
            match schema {
                Some(s) => s,
                None => return vec![],
            }
        }
    };

    let required_set: HashSet<String> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let body_required = operation
        .get("requestBody")
        .and_then(|rb| rb.get("required"))
        .and_then(|r| r.as_bool())
        .unwrap_or(false);

    match schema.get("properties").and_then(|p| p.as_object()) {
        Some(props) => props
            .iter()
            .map(|(prop_name, prop_schema)| {
                let name = crate::core::helpers::to_kebab(prop_name);
                let rust_type = schema_type_to_rust(prop_schema);
                let description = prop_schema
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                let choices = extract_enum_choices(prop_schema);

                // Detect file upload fields
                let is_file = is_multipart
                    && (prop_schema.get("format").and_then(|f| f.as_str()) == Some("binary")
                        || prop_schema.get("type").and_then(|t| t.as_str()) == Some("string")
                            && prop_schema.get("format").and_then(|f| f.as_str())
                                == Some("binary"));

                let location = if is_file {
                    ParamLocation::File
                } else {
                    ParamLocation::Body
                };

                ParamDef {
                    name,
                    original_name: prop_name.clone(),
                    rust_type,
                    required: body_required && required_set.contains(prop_name),
                    description,
                    choices,
                    location,
                    schema: prop_schema.clone(),
                }
            })
            .collect(),
        None => {
            // Non-object body schema (array or primitive). Expose the request
            // body as a single `--body` argument; the executor sends its value
            // verbatim as JSON.
            let description = schema
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("Request body (JSON)")
                .to_string();
            vec![ParamDef {
                name: "body".to_string(),
                original_name: WHOLE_BODY_KEY.to_string(),
                rust_type: schema_type_to_rust(schema),
                required: body_required,
                description,
                choices: extract_enum_choices(schema),
                location: ParamLocation::WholeBody,
                schema: schema.clone(),
            }]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::ParamType;
    use serde_json::json;

    fn petstore_spec() -> Value {
        json!({
            "openapi": "3.0.0",
            "paths": {
                "/pets": {
                    "get": {
                        "operationId": "listPets",
                        "summary": "List all pets",
                        "parameters": [
                            {
                                "name": "limit",
                                "in": "query",
                                "required": false,
                                "description": "Max items to return",
                                "schema": { "type": "integer" }
                            },
                            {
                                "name": "status",
                                "in": "query",
                                "schema": {
                                    "type": "string",
                                    "enum": ["available", "pending", "sold"]
                                }
                            }
                        ]
                    },
                    "post": {
                        "operationId": "createPet",
                        "summary": "Create a pet",
                        "requestBody": {
                            "required": true,
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "required": ["name"],
                                        "properties": {
                                            "name": {
                                                "type": "string",
                                                "description": "Pet name"
                                            },
                                            "tag": {
                                                "type": "string"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "/pets/{petId}": {
                    "parameters": [
                        {
                            "name": "petId",
                            "in": "path",
                            "required": true,
                            "schema": { "type": "integer" }
                        }
                    ],
                    "get": {
                        "operationId": "getPetById",
                        "summary": "Get a pet by ID"
                    },
                    "delete": {
                        "summary": "Delete a pet"
                    }
                }
            }
        })
    }

    #[test]
    fn test_extract_commands_count() {
        let spec = petstore_spec();
        let cmds = extract_openapi_commands(&spec);
        assert_eq!(cmds.len(), 4);
    }

    #[test]
    fn test_operation_id_naming() {
        let spec = petstore_spec();
        let cmds = extract_openapi_commands(&spec);
        let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"list-pets"));
        assert!(names.contains(&"create-pet"));
        assert!(names.contains(&"get-pet-by-id"));
    }

    #[test]
    fn test_fallback_naming() {
        let spec = petstore_spec();
        let cmds = extract_openapi_commands(&spec);
        // The DELETE /pets/{petId} has no operationId
        let delete_cmd = cmds
            .iter()
            .find(|c| c.method.as_deref() == Some("DELETE"))
            .unwrap();
        assert_eq!(delete_cmd.name, "delete-pets-petId");
    }

    #[test]
    fn test_query_params_extracted() {
        let spec = petstore_spec();
        let cmds = extract_openapi_commands(&spec);
        let list = cmds.iter().find(|c| c.name == "list-pets").unwrap();
        assert_eq!(list.params.len(), 2);
        let limit = list
            .params
            .iter()
            .find(|p| p.original_name == "limit")
            .unwrap();
        assert_eq!(limit.location, ParamLocation::Query);
        assert_eq!(limit.rust_type, ParamType::Int);
        assert!(!limit.required);
    }

    #[test]
    fn test_enum_choices_extracted() {
        let spec = petstore_spec();
        let cmds = extract_openapi_commands(&spec);
        let list = cmds.iter().find(|c| c.name == "list-pets").unwrap();
        let status = list
            .params
            .iter()
            .find(|p| p.original_name == "status")
            .unwrap();
        assert_eq!(
            status.choices.as_ref().unwrap(),
            &["available", "pending", "sold"]
        );
    }

    #[test]
    fn test_path_params_from_path_level() {
        let spec = petstore_spec();
        let cmds = extract_openapi_commands(&spec);
        let get = cmds.iter().find(|c| c.name == "get-pet-by-id").unwrap();
        assert_eq!(get.params.len(), 1);
        let pet_id = &get.params[0];
        assert_eq!(pet_id.original_name, "petId");
        assert_eq!(pet_id.location, ParamLocation::Path);
        assert!(pet_id.required);
    }

    #[test]
    fn test_body_params_extracted() {
        let spec = petstore_spec();
        let cmds = extract_openapi_commands(&spec);
        let create = cmds.iter().find(|c| c.name == "create-pet").unwrap();
        assert!(create.has_body);
        assert_eq!(create.content_type.as_deref(), Some("application/json"));
        assert_eq!(create.params.len(), 2); // name and tag
        let name_param = create
            .params
            .iter()
            .find(|p| p.original_name == "name")
            .unwrap();
        assert_eq!(name_param.location, ParamLocation::Body);
        assert!(name_param.required);
        let tag_param = create
            .params
            .iter()
            .find(|p| p.original_name == "tag")
            .unwrap();
        assert!(!tag_param.required);
    }

    #[test]
    fn test_multipart_file_detection() {
        let spec = json!({
            "paths": {
                "/upload": {
                    "post": {
                        "operationId": "uploadFile",
                        "requestBody": {
                            "content": {
                                "multipart/form-data": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "file": {
                                                "type": "string",
                                                "format": "binary"
                                            },
                                            "description": {
                                                "type": "string"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        let cmds = extract_openapi_commands(&spec);
        assert_eq!(cmds.len(), 1);
        let cmd = &cmds[0];
        assert_eq!(cmd.content_type.as_deref(), Some("multipart/form-data"));
        let file_param = cmd
            .params
            .iter()
            .find(|p| p.original_name == "file")
            .unwrap();
        assert_eq!(file_param.location, ParamLocation::File);
    }

    #[test]
    fn test_name_deduplication() {
        let spec = json!({
            "paths": {
                "/a": {
                    "get": { "operationId": "doThing" }
                },
                "/b": {
                    "get": { "operationId": "doThing" }
                }
            }
        });
        let cmds = extract_openapi_commands(&spec);
        let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"do-thing"));
        assert!(names.contains(&"do-thing-2"));
    }
}
