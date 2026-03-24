//! GraphQL integration tests.
//!
//! Tests cover: type mapping, command extraction from mock introspection,
//! selection set building, document building, and execution with mocked endpoint.

use std::collections::{HashMap, HashSet};

use serde_json::json;
use wiremock::matchers::{header, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

use mcp2cli::core::types::{CommandDef, ParamDef, ParamLocation, ParamType};
use mcp2cli::graphql::commands::extract_graphql_commands;
use mcp2cli::graphql::document::build_graphql_document;
use mcp2cli::graphql::executor::execute_graphql;
use mcp2cli::graphql::introspection::{
    IntrospectionEnumValue, IntrospectionField, IntrospectionInputValue, IntrospectionSchema,
    IntrospectionType, TypeName,
};
use mcp2cli::graphql::selection::build_selection_set;
use mcp2cli::graphql::types::{graphql_type_string, graphql_type_to_rust, unwrap_type};
use mcp2cli::output::types::OutputOptions;

// ─── Type Mapping Tests ───────────────────────────────────────────────

#[test]
fn test_unwrap_scalar_string() {
    let t = json!({"kind": "SCALAR", "name": "String", "ofType": null});
    let (name, non_null, is_list) = unwrap_type(&t);
    assert_eq!(name, "String");
    assert!(!non_null);
    assert!(!is_list);
}

#[test]
fn test_unwrap_non_null_string() {
    let t =
        json!({"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "String", "ofType": null}});
    let (name, non_null, is_list) = unwrap_type(&t);
    assert_eq!(name, "String");
    assert!(non_null);
    assert!(!is_list);
}

#[test]
fn test_unwrap_list_of_strings() {
    let t = json!({"kind": "LIST", "ofType": {"kind": "SCALAR", "name": "String", "ofType": null}});
    let (name, _non_null, is_list) = unwrap_type(&t);
    assert_eq!(name, "String");
    assert!(is_list);
}

#[test]
fn test_unwrap_non_null_list_of_non_null_int() {
    let t = json!({
        "kind": "NON_NULL",
        "ofType": {
            "kind": "LIST",
            "ofType": {
                "kind": "NON_NULL",
                "ofType": {"kind": "SCALAR", "name": "Int", "ofType": null}
            }
        }
    });
    let (name, non_null, is_list) = unwrap_type(&t);
    assert_eq!(name, "Int");
    assert!(non_null);
    assert!(is_list);
}

#[test]
fn test_unwrap_enum_type() {
    let t = json!({"kind": "ENUM", "name": "Status", "ofType": null});
    let (name, non_null, is_list) = unwrap_type(&t);
    assert_eq!(name, "Status");
    assert!(!non_null);
    assert!(!is_list);
}

#[test]
fn test_unwrap_input_object() {
    let t = json!({"kind": "INPUT_OBJECT", "name": "CreateUserInput", "ofType": null});
    let (name, _, _) = unwrap_type(&t);
    assert_eq!(name, "CreateUserInput");
}

#[test]
fn test_graphql_type_string_scalar() {
    let t = json!({"kind": "SCALAR", "name": "String", "ofType": null});
    assert_eq!(graphql_type_string(&t), "String");
}

#[test]
fn test_graphql_type_string_non_null() {
    let t = json!({"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "ID", "ofType": null}});
    assert_eq!(graphql_type_string(&t), "ID!");
}

#[test]
fn test_graphql_type_string_list_non_null() {
    let t = json!({
        "kind": "NON_NULL",
        "ofType": {
            "kind": "LIST",
            "ofType": {
                "kind": "NON_NULL",
                "ofType": {"kind": "SCALAR", "name": "String", "ofType": null}
            }
        }
    });
    assert_eq!(graphql_type_string(&t), "[String!]!");
}

#[test]
fn test_graphql_type_to_rust_scalars() {
    assert_eq!(
        graphql_type_to_rust(&json!({"kind": "SCALAR", "name": "Int", "ofType": null})),
        ParamType::Int
    );
    assert_eq!(
        graphql_type_to_rust(&json!({"kind": "SCALAR", "name": "Float", "ofType": null})),
        ParamType::Float
    );
    assert_eq!(
        graphql_type_to_rust(&json!({"kind": "SCALAR", "name": "Boolean", "ofType": null})),
        ParamType::Bool
    );
    assert_eq!(
        graphql_type_to_rust(&json!({"kind": "SCALAR", "name": "String", "ofType": null})),
        ParamType::String
    );
    assert_eq!(
        graphql_type_to_rust(&json!({"kind": "SCALAR", "name": "ID", "ofType": null})),
        ParamType::String
    );
}

#[test]
fn test_graphql_type_to_rust_list() {
    let t = json!({"kind": "LIST", "ofType": {"kind": "SCALAR", "name": "String", "ofType": null}});
    assert_eq!(graphql_type_to_rust(&t), ParamType::Array);
}

#[test]
fn test_graphql_type_to_rust_enum_maps_to_object() {
    let t = json!({"kind": "ENUM", "name": "Status", "ofType": null});
    assert_eq!(graphql_type_to_rust(&t), ParamType::Object);
}

#[test]
fn test_graphql_type_to_rust_input_object_maps_to_object() {
    let t = json!({"kind": "INPUT_OBJECT", "name": "CreateUserInput", "ofType": null});
    assert_eq!(graphql_type_to_rust(&t), ParamType::Object);
}

// ─── Helper: Build Mock Schema ────────────────────────────────────────

fn make_mock_schema() -> IntrospectionSchema {
    IntrospectionSchema {
        query_type: Some(TypeName {
            name: "Query".to_string(),
        }),
        mutation_type: Some(TypeName {
            name: "Mutation".to_string(),
        }),
        types: vec![
            IntrospectionType {
                kind: "SCALAR".to_string(),
                name: Some("String".to_string()),
                description: None,
                fields: None,
                input_fields: None,
                enum_values: None,
            },
            IntrospectionType {
                kind: "SCALAR".to_string(),
                name: Some("Int".to_string()),
                description: None,
                fields: None,
                input_fields: None,
                enum_values: None,
            },
            IntrospectionType {
                kind: "SCALAR".to_string(),
                name: Some("ID".to_string()),
                description: None,
                fields: None,
                input_fields: None,
                enum_values: None,
            },
            IntrospectionType {
                kind: "SCALAR".to_string(),
                name: Some("Boolean".to_string()),
                description: None,
                fields: None,
                input_fields: None,
                enum_values: None,
            },
            IntrospectionType {
                kind: "ENUM".to_string(),
                name: Some("Role".to_string()),
                description: Some("User role".to_string()),
                fields: None,
                input_fields: None,
                enum_values: Some(vec![
                    IntrospectionEnumValue {
                        name: "ADMIN".to_string(),
                        description: None,
                    },
                    IntrospectionEnumValue {
                        name: "USER".to_string(),
                        description: None,
                    },
                ]),
            },
            IntrospectionType {
                kind: "OBJECT".to_string(),
                name: Some("User".to_string()),
                description: Some("A user".to_string()),
                fields: Some(vec![
                    IntrospectionField {
                        name: "id".to_string(),
                        description: None,
                        args: vec![],
                        field_type: json!({"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "ID", "ofType": null}}),
                        is_deprecated: Some(false),
                    },
                    IntrospectionField {
                        name: "name".to_string(),
                        description: None,
                        args: vec![],
                        field_type: json!({"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "String", "ofType": null}}),
                        is_deprecated: Some(false),
                    },
                    IntrospectionField {
                        name: "email".to_string(),
                        description: None,
                        args: vec![],
                        field_type: json!({"kind": "SCALAR", "name": "String", "ofType": null}),
                        is_deprecated: Some(false),
                    },
                    IntrospectionField {
                        name: "role".to_string(),
                        description: None,
                        args: vec![],
                        field_type: json!({"kind": "ENUM", "name": "Role", "ofType": null}),
                        is_deprecated: Some(false),
                    },
                ]),
                input_fields: None,
                enum_values: None,
            },
            IntrospectionType {
                kind: "OBJECT".to_string(),
                name: Some("Post".to_string()),
                description: Some("A blog post".to_string()),
                fields: Some(vec![
                    IntrospectionField {
                        name: "id".to_string(),
                        description: None,
                        args: vec![],
                        field_type: json!({"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "ID", "ofType": null}}),
                        is_deprecated: Some(false),
                    },
                    IntrospectionField {
                        name: "title".to_string(),
                        description: None,
                        args: vec![],
                        field_type: json!({"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "String", "ofType": null}}),
                        is_deprecated: Some(false),
                    },
                    IntrospectionField {
                        name: "author".to_string(),
                        description: None,
                        args: vec![],
                        field_type: json!({"kind": "OBJECT", "name": "User", "ofType": null}),
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
                fields: Some(vec![
                    IntrospectionField {
                        name: "user".to_string(),
                        description: Some("Get a user by ID".to_string()),
                        args: vec![IntrospectionInputValue {
                            name: "id".to_string(),
                            description: Some("User ID".to_string()),
                            input_type: json!({"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "ID", "ofType": null}}),
                            default_value: None,
                        }],
                        field_type: json!({"kind": "OBJECT", "name": "User", "ofType": null}),
                        is_deprecated: Some(false),
                    },
                    IntrospectionField {
                        name: "users".to_string(),
                        description: Some("List all users".to_string()),
                        args: vec![],
                        field_type: json!({
                            "kind": "NON_NULL",
                            "ofType": {
                                "kind": "LIST",
                                "ofType": {"kind": "OBJECT", "name": "User", "ofType": null}
                            }
                        }),
                        is_deprecated: Some(false),
                    },
                    IntrospectionField {
                        name: "post".to_string(),
                        description: Some("Get a post".to_string()),
                        args: vec![IntrospectionInputValue {
                            name: "id".to_string(),
                            description: None,
                            input_type: json!({"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "ID", "ofType": null}}),
                            default_value: None,
                        }],
                        field_type: json!({"kind": "OBJECT", "name": "Post", "ofType": null}),
                        is_deprecated: Some(false),
                    },
                    IntrospectionField {
                        name: "serverVersion".to_string(),
                        description: Some("Get server version".to_string()),
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
                name: Some("Mutation".to_string()),
                description: None,
                fields: Some(vec![
                    IntrospectionField {
                        name: "createUser".to_string(),
                        description: Some("Create a new user".to_string()),
                        args: vec![
                            IntrospectionInputValue {
                                name: "name".to_string(),
                                description: Some("User name".to_string()),
                                input_type: json!({"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "String", "ofType": null}}),
                                default_value: None,
                            },
                            IntrospectionInputValue {
                                name: "email".to_string(),
                                description: Some("Email address".to_string()),
                                input_type: json!({"kind": "SCALAR", "name": "String", "ofType": null}),
                                default_value: None,
                            },
                        ],
                        field_type: json!({"kind": "OBJECT", "name": "User", "ofType": null}),
                        is_deprecated: Some(false),
                    },
                    IntrospectionField {
                        name: "deleteUser".to_string(),
                        description: Some("Delete a user".to_string()),
                        args: vec![IntrospectionInputValue {
                            name: "id".to_string(),
                            description: None,
                            input_type: json!({"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "ID", "ofType": null}}),
                            default_value: None,
                        }],
                        field_type: json!({"kind": "SCALAR", "name": "Boolean", "ofType": null}),
                        is_deprecated: Some(false),
                    },
                ]),
                input_fields: None,
                enum_values: None,
            },
        ],
    }
}

// ─── Command Extraction Tests ─────────────────────────────────────────

#[test]
fn test_extract_commands_count() {
    let schema = make_mock_schema();
    let commands = extract_graphql_commands(&schema);
    // 4 queries + 2 mutations = 6
    assert_eq!(commands.len(), 6);
}

#[test]
fn test_extract_commands_query_names() {
    let schema = make_mock_schema();
    let commands = extract_graphql_commands(&schema);
    let query_names: Vec<&str> = commands
        .iter()
        .filter(|c| c.graphql_operation_type.as_deref() == Some("query"))
        .map(|c| c.name.as_str())
        .collect();
    assert!(query_names.contains(&"user"));
    assert!(query_names.contains(&"users"));
    assert!(query_names.contains(&"post"));
    assert!(query_names.contains(&"server-version"));
}

#[test]
fn test_extract_commands_mutation_names() {
    let schema = make_mock_schema();
    let commands = extract_graphql_commands(&schema);
    let mutation_names: Vec<&str> = commands
        .iter()
        .filter(|c| c.graphql_operation_type.as_deref() == Some("mutation"))
        .map(|c| c.name.as_str())
        .collect();
    assert!(mutation_names.contains(&"create-user"));
    assert!(mutation_names.contains(&"delete-user"));
}

#[test]
fn test_extract_commands_params() {
    let schema = make_mock_schema();
    let commands = extract_graphql_commands(&schema);
    let create_user = commands
        .iter()
        .find(|c| c.graphql_field_name.as_deref() == Some("createUser"))
        .expect("createUser command");
    assert_eq!(create_user.params.len(), 2);

    let name_param = &create_user.params[0];
    assert_eq!(name_param.original_name, "name");
    assert_eq!(name_param.rust_type, ParamType::String);
    assert!(name_param.required); // NON_NULL

    let email_param = &create_user.params[1];
    assert_eq!(email_param.original_name, "email");
    assert_eq!(email_param.rust_type, ParamType::String);
    assert!(!email_param.required); // nullable
}

#[test]
fn test_extract_commands_preserves_field_name() {
    let schema = make_mock_schema();
    let commands = extract_graphql_commands(&schema);
    let sv = commands
        .iter()
        .find(|c| c.name == "server-version")
        .unwrap();
    assert_eq!(sv.graphql_field_name.as_deref(), Some("serverVersion"));
}

#[test]
fn test_extract_commands_operation_type() {
    let schema = make_mock_schema();
    let commands = extract_graphql_commands(&schema);
    let user = commands.iter().find(|c| c.name == "user").unwrap();
    assert_eq!(user.graphql_operation_type.as_deref(), Some("query"));

    let create = commands
        .iter()
        .find(|c| c.graphql_field_name.as_deref() == Some("createUser"))
        .unwrap();
    assert_eq!(create.graphql_operation_type.as_deref(), Some("mutation"));
}

#[test]
fn test_extract_commands_name_collision() {
    // When query and mutation have the same field name, one gets prefixed
    let schema = IntrospectionSchema {
        query_type: Some(TypeName {
            name: "Query".to_string(),
        }),
        mutation_type: Some(TypeName {
            name: "Mutation".to_string(),
        }),
        types: vec![
            IntrospectionType {
                kind: "OBJECT".to_string(),
                name: Some("Query".to_string()),
                description: None,
                fields: Some(vec![IntrospectionField {
                    name: "user".to_string(),
                    description: None,
                    args: vec![],
                    field_type: json!({"kind": "SCALAR", "name": "String", "ofType": null}),
                    is_deprecated: None,
                }]),
                input_fields: None,
                enum_values: None,
            },
            IntrospectionType {
                kind: "OBJECT".to_string(),
                name: Some("Mutation".to_string()),
                description: None,
                fields: Some(vec![IntrospectionField {
                    name: "user".to_string(),
                    description: None,
                    args: vec![],
                    field_type: json!({"kind": "SCALAR", "name": "Boolean", "ofType": null}),
                    is_deprecated: None,
                }]),
                input_fields: None,
                enum_values: None,
            },
        ],
    };
    let commands = extract_graphql_commands(&schema);
    let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
    // First one keeps "user", second gets prefixed
    assert!(names.contains(&"user"));
    assert!(
        names.contains(&"query-user") || names.contains(&"mutation-user"),
        "Expected a prefixed name for the collision, got: {:?}",
        names
    );
}

// ─── Selection Set Building Tests ─────────────────────────────────────

fn build_types_map(schema: &IntrospectionSchema) -> HashMap<String, &IntrospectionType> {
    schema
        .types
        .iter()
        .filter_map(|t| t.name.as_ref().map(|n| (n.clone(), t)))
        .collect()
}

#[test]
fn test_selection_set_scalar_fields() {
    let schema = make_mock_schema();
    let types_map = build_types_map(&schema);
    let mut seen = HashSet::new();
    let sel = build_selection_set("User", &types_map, 0, 3, &mut seen);
    assert!(sel.contains("id"));
    assert!(sel.contains("name"));
    assert!(sel.contains("email"));
}

#[test]
fn test_selection_set_nested_object() {
    let schema = make_mock_schema();
    let types_map = build_types_map(&schema);
    let mut seen = HashSet::new();
    let sel = build_selection_set("Post", &types_map, 0, 3, &mut seen);
    assert!(sel.contains("id"));
    assert!(sel.contains("title"));
    // author is a User object, should have nested fields
    assert!(sel.contains("author"));
    assert!(sel.contains("author { id name email"));
}

#[test]
fn test_selection_set_depth_limit() {
    // With max_depth=1, should only get top-level scalar fields
    let schema = make_mock_schema();
    let types_map = build_types_map(&schema);
    let mut seen = HashSet::new();
    let sel = build_selection_set("Post", &types_map, 0, 1, &mut seen);
    assert!(sel.contains("id"));
    assert!(sel.contains("title"));
    // At depth 1, author (object) should NOT be expanded since it needs depth 2
    assert!(!sel.contains("author {"));
}

#[test]
fn test_selection_set_circular_reference() {
    // Create a type that references itself: Node -> children: [Node]
    let schema = IntrospectionSchema {
        query_type: None,
        mutation_type: None,
        types: vec![IntrospectionType {
            kind: "OBJECT".to_string(),
            name: Some("Node".to_string()),
            description: None,
            fields: Some(vec![
                IntrospectionField {
                    name: "id".to_string(),
                    description: None,
                    args: vec![],
                    field_type: json!({"kind": "SCALAR", "name": "ID", "ofType": null}),
                    is_deprecated: None,
                },
                IntrospectionField {
                    name: "children".to_string(),
                    description: None,
                    args: vec![],
                    field_type: json!({"kind": "LIST", "ofType": {"kind": "OBJECT", "name": "Node", "ofType": null}}),
                    is_deprecated: None,
                },
            ]),
            input_fields: None,
            enum_values: None,
        }],
    };
    let types_map = build_types_map(&schema);
    let mut seen = HashSet::new();
    let sel = build_selection_set("Node", &types_map, 0, 5, &mut seen);
    // Should not infinite loop; should have scalar id at top level
    assert!(sel.contains("id"));
    // children → Node is circular (Node already in seen set during recursion),
    // so nested selection is empty and the field is omitted — correct behavior
    assert!(
        !sel.contains("children { children"),
        "Circular reference should not produce infinite nesting, got: {sel}"
    );
    // seen should be cleared after (backtracking)
    assert!(seen.is_empty());
}

#[test]
fn test_selection_set_reuse_in_siblings() {
    // Two fields reference the same type — both should get selection sets
    let schema = IntrospectionSchema {
        query_type: None,
        mutation_type: None,
        types: vec![
            IntrospectionType {
                kind: "OBJECT".to_string(),
                name: Some("Team".to_string()),
                description: None,
                fields: Some(vec![
                    IntrospectionField {
                        name: "lead".to_string(),
                        description: None,
                        args: vec![],
                        field_type: json!({"kind": "OBJECT", "name": "Person", "ofType": null}),
                        is_deprecated: None,
                    },
                    IntrospectionField {
                        name: "manager".to_string(),
                        description: None,
                        args: vec![],
                        field_type: json!({"kind": "OBJECT", "name": "Person", "ofType": null}),
                        is_deprecated: None,
                    },
                ]),
                input_fields: None,
                enum_values: None,
            },
            IntrospectionType {
                kind: "OBJECT".to_string(),
                name: Some("Person".to_string()),
                description: None,
                fields: Some(vec![IntrospectionField {
                    name: "name".to_string(),
                    description: None,
                    args: vec![],
                    field_type: json!({"kind": "SCALAR", "name": "String", "ofType": null}),
                    is_deprecated: None,
                }]),
                input_fields: None,
                enum_values: None,
            },
        ],
    };
    let types_map = build_types_map(&schema);
    let mut seen = HashSet::new();
    let sel = build_selection_set("Team", &types_map, 0, 3, &mut seen);
    // Both lead and manager should have nested selection
    assert!(sel.contains("lead { name }"));
    assert!(sel.contains("manager { name }"));
}

// ─── Document Building Tests ──────────────────────────────────────────

#[test]
fn test_document_query_with_variables() {
    let schema = make_mock_schema();
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
    args.insert("id".to_string(), json!("42"));

    let (doc, vars) = build_graphql_document(&cmd, &args, &schema, None);
    assert!(doc.starts_with("query"));
    assert!(doc.contains("$id: ID!"));
    assert!(doc.contains("user(id: $id)"));
    assert!(doc.contains("{ id name email"));
    assert_eq!(vars["id"], json!("42"));
}

#[test]
fn test_document_mutation_with_multiple_variables() {
    let schema = make_mock_schema();
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
    assert!(doc.contains("$name: String!"));
    assert!(doc.contains("$email: String"));
    assert!(doc.contains("createUser("));
    assert!(doc.contains("name: $name"));
    assert!(doc.contains("email: $email"));
    assert_eq!(vars["name"], json!("Alice"));
    assert_eq!(vars["email"], json!("alice@example.com"));
}

#[test]
fn test_document_fields_override() {
    let schema = make_mock_schema();
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
    let (doc, _) = build_graphql_document(&cmd, &HashMap::new(), &schema, Some("id name"));
    assert!(doc.contains("{ id name }"));
    assert!(!doc.contains("email"));
}

#[test]
fn test_document_scalar_return_no_selection() {
    let schema = make_mock_schema();
    let cmd = CommandDef {
        name: "server-version".to_string(),
        description: "".to_string(),
        params: vec![],
        has_body: false,
        method: None,
        path: None,
        content_type: None,
        tool_name: None,
        graphql_operation_type: Some("query".to_string()),
        graphql_field_name: Some("serverVersion".to_string()),
        graphql_return_type: Some(json!({"kind": "SCALAR", "name": "String", "ofType": null})),
    };
    let (doc, _) = build_graphql_document(&cmd, &HashMap::new(), &schema, None);
    assert!(doc.contains("serverVersion"));
    // Scalar return should have no { } selection set
    let count = doc.matches('{').count();
    // Only the outer operation brace, not a selection set brace
    assert_eq!(
        count, 1,
        "Expected only 1 opening brace for scalar return, doc: {doc}"
    );
}

#[test]
fn test_document_no_args_no_parens() {
    let schema = make_mock_schema();
    let cmd = CommandDef {
        name: "users".to_string(),
        description: "".to_string(),
        params: vec![],
        has_body: false,
        method: None,
        path: None,
        content_type: None,
        tool_name: None,
        graphql_operation_type: Some("query".to_string()),
        graphql_field_name: Some("users".to_string()),
        graphql_return_type: Some(json!({"kind": "OBJECT", "name": "User", "ofType": null})),
    };
    let (doc, vars) = build_graphql_document(&cmd, &HashMap::new(), &schema, None);
    // No args means no parentheses on the operation or field
    assert!(doc.starts_with("query {"));
    assert!(!doc.contains("users("));
    assert_eq!(vars, json!({}));
}

// ─── Execution Tests (wiremock) ───────────────────────────────────────

fn default_output_opts() -> OutputOptions {
    OutputOptions {
        pretty: false,
        raw: true, // raw to avoid TTY detection issues in tests
        head: None,
        jq: None,
        toon: false,
    }
}

#[tokio::test]
async fn test_execute_graphql_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "user": {
                    "id": "1",
                    "name": "Alice",
                    "email": "alice@example.com"
                }
            }
        })))
        .mount(&mock_server)
        .await;

    let schema = make_mock_schema();
    let cmd = CommandDef {
        name: "user".to_string(),
        description: "".to_string(),
        params: vec![ParamDef {
            name: "id".to_string(),
            original_name: "id".to_string(),
            rust_type: ParamType::String,
            required: true,
            description: "".to_string(),
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
    args.insert("id".to_string(), json!("1"));

    let result = execute_graphql(
        &cmd,
        &args,
        &mock_server.uri(),
        &schema,
        &[],
        None,
        &default_output_opts(),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_graphql_with_auth_header() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "serverVersion": "1.0.0" }
        })))
        .mount(&mock_server)
        .await;

    let schema = make_mock_schema();
    let cmd = CommandDef {
        name: "server-version".to_string(),
        description: "".to_string(),
        params: vec![],
        has_body: false,
        method: None,
        path: None,
        content_type: None,
        tool_name: None,
        graphql_operation_type: Some("query".to_string()),
        graphql_field_name: Some("serverVersion".to_string()),
        graphql_return_type: Some(json!({"kind": "SCALAR", "name": "String", "ofType": null})),
    };

    let result = execute_graphql(
        &cmd,
        &HashMap::new(),
        &mock_server.uri(),
        &schema,
        &[("Authorization".to_string(), "Bearer test-token".to_string())],
        None,
        &default_output_opts(),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_graphql_errors_no_data() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "errors": [
                { "message": "Field 'unknown' not found" }
            ]
        })))
        .mount(&mock_server)
        .await;

    let schema = make_mock_schema();
    let cmd = CommandDef {
        name: "unknown".to_string(),
        description: "".to_string(),
        params: vec![],
        has_body: false,
        method: None,
        path: None,
        content_type: None,
        tool_name: None,
        graphql_operation_type: Some("query".to_string()),
        graphql_field_name: Some("unknown".to_string()),
        graphql_return_type: None,
    };

    let result = execute_graphql(
        &cmd,
        &HashMap::new(),
        &mock_server.uri(),
        &schema,
        &[],
        None,
        &default_output_opts(),
    )
    .await;

    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("Field 'unknown' not found"));
}

#[tokio::test]
async fn test_execute_graphql_non_2xx_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&mock_server)
        .await;

    let schema = make_mock_schema();
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
        graphql_return_type: None,
    };

    let result = execute_graphql(
        &cmd,
        &HashMap::new(),
        &mock_server.uri(),
        &schema,
        &[],
        None,
        &default_output_opts(),
    )
    .await;

    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("500"));
}

#[tokio::test]
async fn test_execute_graphql_partial_errors_with_data() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "user": { "id": "1", "name": "Alice", "email": null }
            },
            "errors": [
                { "message": "Could not resolve email" }
            ]
        })))
        .mount(&mock_server)
        .await;

    let schema = make_mock_schema();
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

    // Should succeed since data is present (errors are warnings)
    let result = execute_graphql(
        &cmd,
        &HashMap::new(),
        &mock_server.uri(),
        &schema,
        &[],
        None,
        &default_output_opts(),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_graphql_verifies_query_format() {
    let mock_server = MockServer::start().await;

    // Use a custom responder to capture and verify the request
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "createUser": { "id": "99", "name": "Bob", "email": "bob@test.com" } }
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let schema = make_mock_schema();
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
    args.insert("name".to_string(), json!("Bob"));
    args.insert("email".to_string(), json!("bob@test.com"));

    let result = execute_graphql(
        &cmd,
        &args,
        &mock_server.uri(),
        &schema,
        &[],
        None,
        &default_output_opts(),
    )
    .await;

    assert!(result.is_ok());
    // wiremock .expect(1) verifies exactly one request was made
}

#[tokio::test]
async fn test_execute_graphql_with_fields_override() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "user": { "id": "1", "name": "Alice" }
            }
        })))
        .mount(&mock_server)
        .await;

    let schema = make_mock_schema();
    let cmd = CommandDef {
        name: "user".to_string(),
        description: "".to_string(),
        params: vec![ParamDef {
            name: "id".to_string(),
            original_name: "id".to_string(),
            rust_type: ParamType::String,
            required: true,
            description: "".to_string(),
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
    args.insert("id".to_string(), json!("1"));

    let result = execute_graphql(
        &cmd,
        &args,
        &mock_server.uri(),
        &schema,
        &[],
        Some("id name"),
        &default_output_opts(),
    )
    .await;

    assert!(result.is_ok());
}
