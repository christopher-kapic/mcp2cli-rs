use crate::core::types::ParamType;
use serde_json::Value;

/// Unwrap a GraphQL type reference to its named type.
/// Returns (named_type, is_non_null, is_list).
pub fn unwrap_type(type_ref: &Value) -> (String, bool, bool) {
    let kind = type_ref.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    let name = type_ref.get("name").and_then(|n| n.as_str());
    let of_type = type_ref.get("ofType");

    match kind {
        "NON_NULL" => {
            if let Some(inner) = of_type {
                let (named, _, is_list) = unwrap_type(inner);
                (named, true, is_list)
            } else {
                (String::new(), true, false)
            }
        }
        "LIST" => {
            if let Some(inner) = of_type {
                let (named, _, _) = unwrap_type(inner);
                (named, false, true)
            } else {
                (String::new(), false, true)
            }
        }
        _ => (name.unwrap_or("").to_string(), false, false),
    }
}

/// Build a GraphQL type string like "[String!]!".
pub fn graphql_type_string(type_ref: &Value) -> String {
    let kind = type_ref.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    let name = type_ref.get("name").and_then(|n| n.as_str());
    let of_type = type_ref.get("ofType");

    match kind {
        "NON_NULL" => {
            if let Some(inner) = of_type {
                format!("{}!", graphql_type_string(inner))
            } else {
                "!".to_string()
            }
        }
        "LIST" => {
            if let Some(inner) = of_type {
                format!("[{}]", graphql_type_string(inner))
            } else {
                "[]".to_string()
            }
        }
        _ => name.unwrap_or("Unknown").to_string(),
    }
}

/// Map a GraphQL type to our internal ParamType.
pub fn graphql_type_to_rust(type_ref: &Value) -> ParamType {
    let (named, _, is_list) = unwrap_type(type_ref);
    if is_list {
        return ParamType::Array;
    }
    match named.as_str() {
        "Int" => ParamType::Int,
        "Float" => ParamType::Float,
        "Boolean" => ParamType::Bool,
        "String" | "ID" => ParamType::String,
        _ => ParamType::Object, // input objects, enums handled separately
    }
}
