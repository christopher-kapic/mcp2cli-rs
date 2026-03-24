use std::collections::HashMap;

use crate::core::types::CommandDef;
use crate::graphql::introspection::IntrospectionSchema;
use crate::graphql::selection::build_selection_set;
use crate::graphql::types::graphql_type_string;

/// Build a GraphQL document string and variables map from a command and its arguments.
///
/// Returns (document_string, variables_map).
pub fn build_graphql_document(
    cmd: &CommandDef,
    args: &HashMap<String, serde_json::Value>,
    schema: &IntrospectionSchema,
    fields_override: Option<&str>,
) -> (String, serde_json::Value) {
    let op_type = cmd.graphql_operation_type.as_deref().unwrap_or("query");
    let field_name = cmd.graphql_field_name.as_deref().unwrap_or(&cmd.name);

    // Build variable declarations and argument references
    let mut var_decls = Vec::new();
    let mut field_args = Vec::new();
    let mut variables = serde_json::Map::new();

    for param in &cmd.params {
        if let Some(val) = args.get(&param.original_name) {
            let type_str = graphql_type_string(&param.schema);
            let var_name = &param.original_name;
            var_decls.push(format!("${var_name}: {type_str}"));
            field_args.push(format!("{var_name}: ${var_name}"));
            variables.insert(var_name.clone(), val.clone());
        }
    }

    // Build selection set
    let selection = if let Some(fields) = fields_override {
        fields.to_string()
    } else {
        build_selection_from_return_type(cmd, schema)
    };

    // Assemble document
    let var_decl_str = if var_decls.is_empty() {
        String::new()
    } else {
        format!("({})", var_decls.join(", "))
    };

    let field_arg_str = if field_args.is_empty() {
        String::new()
    } else {
        format!("({})", field_args.join(", "))
    };

    let body = if selection.is_empty() {
        format!("  {field_name}{field_arg_str}")
    } else {
        format!("  {field_name}{field_arg_str} {{ {selection} }}")
    };

    let document = format!("{op_type}{var_decl_str} {{\n{body}\n}}");

    (document, serde_json::Value::Object(variables))
}

/// Build selection set from the command's return type using the schema's type map.
fn build_selection_from_return_type(cmd: &CommandDef, schema: &IntrospectionSchema) -> String {
    let return_type = match &cmd.graphql_return_type {
        Some(rt) => rt,
        None => return String::new(),
    };

    let (named_type, _, _) = crate::graphql::types::unwrap_type(return_type);

    // Scalars don't need selection sets
    if matches!(
        named_type.as_str(),
        "String" | "Int" | "Float" | "Boolean" | "ID"
    ) {
        return String::new();
    }

    // Build types map from schema
    let types_map: HashMap<String, &crate::graphql::introspection::IntrospectionType> = schema
        .types
        .iter()
        .filter_map(|t| t.name.as_ref().map(|n| (n.clone(), t)))
        .collect();

    let mut seen = std::collections::HashSet::new();
    build_selection_set(&named_type, &types_map, 0, 3, &mut seen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{ParamDef, ParamLocation, ParamType};
    use crate::graphql::introspection::{
        IntrospectionField, IntrospectionSchema, IntrospectionType, TypeName,
    };
    use serde_json::json;

    fn make_schema_with_user_type() -> IntrospectionSchema {
        IntrospectionSchema {
            query_type: Some(TypeName {
                name: "Query".to_string(),
            }),
            mutation_type: None,
            types: vec![
                IntrospectionType {
                    kind: "OBJECT".to_string(),
                    name: Some("User".to_string()),
                    description: None,
                    fields: Some(vec![
                        IntrospectionField {
                            name: "id".to_string(),
                            description: None,
                            args: vec![],
                            field_type: json!({"kind": "SCALAR", "name": "ID", "ofType": null}),
                            is_deprecated: Some(false),
                        },
                        IntrospectionField {
                            name: "name".to_string(),
                            description: None,
                            args: vec![],
                            field_type: json!({"kind": "SCALAR", "name": "String", "ofType": null}),
                            is_deprecated: Some(false),
                        },
                        IntrospectionField {
                            name: "email".to_string(),
                            description: None,
                            args: vec![],
                            field_type: json!({"kind": "SCALAR", "name": "String", "ofType": null}),
                            is_deprecated: Some(false),
                        },
                    ]),
                    input_fields: None,
                    enum_values: None,
                },
                IntrospectionType {
                    kind: "OBJECT".to_string(),
                    name: Some("Query".to_string()),
                    description: None,
                    fields: Some(vec![IntrospectionField {
                        name: "user".to_string(),
                        description: None,
                        args: vec![],
                        field_type: json!({"kind": "OBJECT", "name": "User", "ofType": null}),
                        is_deprecated: Some(false),
                    }]),
                    input_fields: None,
                    enum_values: None,
                },
            ],
        }
    }

    #[test]
    fn test_build_document_simple_query() {
        let schema = make_schema_with_user_type();
        let cmd = CommandDef {
            name: "user".to_string(),
            description: "Get user".to_string(),
            params: vec![ParamDef {
                name: "id".to_string(),
                original_name: "id".to_string(),
                rust_type: ParamType::String,
                required: true,
                description: "User ID".to_string(),
                choices: None,
                location: ParamLocation::GraphqlArg,
                schema: json!({"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "ID", "ofType": null}}),
            }],
            has_body: false,
            method: None,
            path: None,
            content_type: None,
            tool_name: None,
            graphql_operation_type: Some("query".to_string()),
            graphql_field_name: Some("user".to_string()),
            graphql_return_type: Some(json!({"kind": "OBJECT", "name": "User", "ofType": null})),
        };

        let mut args = HashMap::new();
        args.insert("id".to_string(), json!("123"));

        let (doc, vars) = build_graphql_document(&cmd, &args, &schema, None);

        assert!(doc.contains("query($id: ID!)"));
        assert!(doc.contains("user(id: $id)"));
        assert!(doc.contains("id"));
        assert!(doc.contains("name"));
        assert!(doc.contains("email"));
        assert_eq!(vars["id"], json!("123"));
    }

    #[test]
    fn test_build_document_with_fields_override() {
        let schema = make_schema_with_user_type();
        let cmd = CommandDef {
            name: "user".to_string(),
            description: "".to_string(),
            params: vec![],
            has_body: false,
            method: None,
            path: None,
            content_type: None,
            tool_name: None,
            graphql_operation_type: Some("query".to_string()),
            graphql_field_name: Some("user".to_string()),
            graphql_return_type: Some(json!({"kind": "OBJECT", "name": "User", "ofType": null})),
        };

        let args = HashMap::new();
        let (doc, _) = build_graphql_document(&cmd, &args, &schema, Some("id name"));

        assert!(doc.contains("{ id name }"));
        assert!(!doc.contains("email"));
    }

    #[test]
    fn test_build_document_mutation() {
        let schema = make_schema_with_user_type();
        let cmd = CommandDef {
            name: "create-user".to_string(),
            description: "".to_string(),
            params: vec![
                ParamDef {
                    name: "name".to_string(),
                    original_name: "name".to_string(),
                    rust_type: ParamType::String,
                    required: true,
                    description: "".to_string(),
                    choices: None,
                    location: ParamLocation::GraphqlArg,
                    schema: json!({"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "String", "ofType": null}}),
                },
                ParamDef {
                    name: "email".to_string(),
                    original_name: "email".to_string(),
                    rust_type: ParamType::String,
                    required: false,
                    description: "".to_string(),
                    choices: None,
                    location: ParamLocation::GraphqlArg,
                    schema: json!({"kind": "SCALAR", "name": "String", "ofType": null}),
                },
            ],
            has_body: false,
            method: None,
            path: None,
            content_type: None,
            tool_name: None,
            graphql_operation_type: Some("mutation".to_string()),
            graphql_field_name: Some("createUser".to_string()),
            graphql_return_type: Some(json!({"kind": "OBJECT", "name": "User", "ofType": null})),
        };

        let mut args = HashMap::new();
        args.insert("name".to_string(), json!("Alice"));
        args.insert("email".to_string(), json!("alice@example.com"));

        let (doc, vars) = build_graphql_document(&cmd, &args, &schema, None);

        assert!(doc.starts_with("mutation"));
        assert!(doc.contains("createUser"));
        assert!(doc.contains("$name: String!"));
        assert!(doc.contains("$email: String"));
        assert_eq!(vars["name"], json!("Alice"));
        assert_eq!(vars["email"], json!("alice@example.com"));
    }

    #[test]
    fn test_build_document_scalar_return_no_selection() {
        let schema = IntrospectionSchema {
            query_type: Some(TypeName {
                name: "Query".to_string(),
            }),
            mutation_type: None,
            types: vec![],
        };

        let cmd = CommandDef {
            name: "version".to_string(),
            description: "".to_string(),
            params: vec![],
            has_body: false,
            method: None,
            path: None,
            content_type: None,
            tool_name: None,
            graphql_operation_type: Some("query".to_string()),
            graphql_field_name: Some("version".to_string()),
            graphql_return_type: Some(json!({"kind": "SCALAR", "name": "String", "ofType": null})),
        };

        let (doc, _) = build_graphql_document(&cmd, &HashMap::new(), &schema, None);

        // Should not have curly braces for selection set when returning scalar
        assert!(doc.contains("version"));
        assert!(!doc.contains("{ }"));
    }
}
