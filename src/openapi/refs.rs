use serde_json::Value;
use std::collections::HashSet;

/// Recursively resolve $ref pointers in a JSON value.
/// `seen` tracks the current resolution chain to detect circular references.
/// After resolving a ref, it is removed from `seen` so the same ref can be
/// used in multiple unrelated locations without being treated as circular.
pub fn resolve_refs(value: &Value, root: &Value, seen: &mut HashSet<String>) -> Value {
    match value {
        Value::Object(map) => {
            if let Some(ref_val) = map.get("$ref").and_then(|r| r.as_str()) {
                if seen.contains(ref_val) {
                    // Circular reference — return empty object
                    return Value::Object(serde_json::Map::new());
                }
                seen.insert(ref_val.to_string());

                let result = if let Some(resolved) = resolve_pointer(ref_val, root) {
                    resolve_refs(&resolved, root, seen)
                } else {
                    value.clone()
                };

                // Backtrack so this ref can be used elsewhere
                seen.remove(ref_val);
                return result;
            }
            let mut new_map = serde_json::Map::new();
            for (k, v) in map {
                new_map.insert(k.clone(), resolve_refs(v, root, seen));
            }
            Value::Object(new_map)
        }
        Value::Array(arr) => {
            Value::Array(arr.iter().map(|v| resolve_refs(v, root, seen)).collect())
        }
        _ => value.clone(),
    }
}

/// Resolve a JSON pointer like "#/components/schemas/Pet".
fn resolve_pointer(ref_str: &str, root: &Value) -> Option<Value> {
    let path = ref_str.strip_prefix("#/")?;
    let mut current = root;
    for part in path.split('/') {
        current = current.get(part)?;
    }
    Some(current.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_ref_resolution() {
        let root = json!({
            "components": {
                "schemas": {
                    "Pet": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" }
                        }
                    }
                }
            },
            "target": {
                "$ref": "#/components/schemas/Pet"
            }
        });
        let target = root.get("target").unwrap();
        let mut seen = HashSet::new();
        let resolved = resolve_refs(target, &root, &mut seen);
        assert_eq!(
            resolved
                .get("properties")
                .unwrap()
                .get("name")
                .unwrap()
                .get("type")
                .unwrap(),
            "string"
        );
    }

    #[test]
    fn test_nested_ref_resolution() {
        let root = json!({
            "components": {
                "schemas": {
                    "Pet": {
                        "type": "object",
                        "properties": {
                            "owner": { "$ref": "#/components/schemas/Owner" }
                        }
                    },
                    "Owner": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" }
                        }
                    }
                }
            },
            "target": {
                "$ref": "#/components/schemas/Pet"
            }
        });
        let target = root.get("target").unwrap();
        let mut seen = HashSet::new();
        let resolved = resolve_refs(target, &root, &mut seen);
        let owner = resolved.get("properties").unwrap().get("owner").unwrap();
        assert_eq!(
            owner
                .get("properties")
                .unwrap()
                .get("name")
                .unwrap()
                .get("type")
                .unwrap(),
            "string"
        );
    }

    #[test]
    fn test_circular_ref_detection() {
        let root = json!({
            "components": {
                "schemas": {
                    "Node": {
                        "type": "object",
                        "properties": {
                            "child": { "$ref": "#/components/schemas/Node" }
                        }
                    }
                }
            },
            "target": {
                "$ref": "#/components/schemas/Node"
            }
        });
        let target = root.get("target").unwrap();
        let mut seen = HashSet::new();
        let resolved = resolve_refs(target, &root, &mut seen);
        // The circular child should be an empty object
        let child = resolved.get("properties").unwrap().get("child").unwrap();
        assert_eq!(child, &json!({}));
    }

    #[test]
    fn test_same_ref_reused_in_multiple_places() {
        let root = json!({
            "components": {
                "schemas": {
                    "Tag": {
                        "type": "string"
                    }
                }
            },
            "target": {
                "type": "object",
                "properties": {
                    "tag1": { "$ref": "#/components/schemas/Tag" },
                    "tag2": { "$ref": "#/components/schemas/Tag" }
                }
            }
        });
        let target = root.get("target").unwrap();
        let mut seen = HashSet::new();
        let resolved = resolve_refs(target, &root, &mut seen);
        let props = resolved.get("properties").unwrap();
        // Both should resolve to the Tag schema, not be treated as circular
        assert_eq!(props.get("tag1").unwrap(), &json!({ "type": "string" }));
        assert_eq!(props.get("tag2").unwrap(), &json!({ "type": "string" }));
    }

    #[test]
    fn test_unresolvable_ref_preserved() {
        let root = json!({
            "target": {
                "$ref": "#/components/schemas/Missing"
            }
        });
        let target = root.get("target").unwrap();
        let mut seen = HashSet::new();
        let resolved = resolve_refs(target, &root, &mut seen);
        // Unresolvable ref returns the original value
        assert_eq!(
            resolved.get("$ref").unwrap(),
            "#/components/schemas/Missing"
        );
    }
}
