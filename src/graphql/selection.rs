use crate::graphql::introspection::IntrospectionType;
use std::collections::{HashMap, HashSet};

/// Build a selection set for a GraphQL type, recursing up to `max_depth`.
pub fn build_selection_set(
    type_name: &str,
    types_map: &HashMap<String, &IntrospectionType>,
    depth: usize,
    max_depth: usize,
    seen: &mut HashSet<String>,
) -> String {
    if depth >= max_depth || seen.contains(type_name) {
        return String::new();
    }

    let type_def = match types_map.get(type_name) {
        Some(t) => t,
        None => return String::new(),
    };

    let fields = match &type_def.fields {
        Some(f) => f,
        None => return String::new(),
    };

    seen.insert(type_name.to_string());

    let mut parts = Vec::new();
    for field in fields {
        let (named_type, _, _) = crate::graphql::types::unwrap_type(&field.field_type);

        if is_scalar(&named_type) {
            parts.push(field.name.clone());
        } else {
            let nested = build_selection_set(&named_type, types_map, depth + 1, max_depth, seen);
            if !nested.is_empty() {
                parts.push(format!("{} {{ {} }}", field.name, nested));
            }
        }
    }

    seen.remove(type_name);
    parts.join(" ")
}

fn is_scalar(name: &str) -> bool {
    matches!(name, "String" | "Int" | "Float" | "Boolean" | "ID") || name.starts_with("__")
}
