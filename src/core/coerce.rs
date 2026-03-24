use crate::core::types::ParamType;
use crate::error::{AppError, Result};
use serde_json::Value;

/// Map a JSON schema type to our internal ParamType.
pub fn schema_type_to_rust(schema: &Value) -> ParamType {
    match schema.get("type").and_then(|t| t.as_str()) {
        Some("integer") => ParamType::Int,
        Some("number") => ParamType::Float,
        Some("boolean") => ParamType::Bool,
        Some("array") => ParamType::Array,
        Some("object") => ParamType::Object,
        _ => ParamType::String,
    }
}

/// Coerce a string CLI value into a typed serde_json::Value based on the schema.
pub fn coerce_value(value: &str, schema: &Value) -> Result<Value> {
    let param_type = schema_type_to_rust(schema);
    match param_type {
        ParamType::Int => value
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| AppError::Cli(format!("expected integer, got '{value}'"))),
        ParamType::Float => value
            .parse::<f64>()
            .map(Value::from)
            .map_err(|_| AppError::Cli(format!("expected number, got '{value}'"))),
        ParamType::Bool => match value.to_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(Value::Bool(true)),
            "false" | "0" | "no" => Ok(Value::Bool(false)),
            _ => Err(AppError::Cli(format!("expected boolean, got '{value}'"))),
        },
        ParamType::Array => {
            // Try JSON parse first, then comma-delimited
            if let Ok(v) = serde_json::from_str::<Value>(value) {
                if v.is_array() {
                    return Ok(v);
                }
            }
            let items: Vec<Value> = value
                .split(',')
                .map(|s| Value::String(s.trim().to_string()))
                .collect();
            Ok(Value::Array(items))
        }
        ParamType::Object => serde_json::from_str(value)
            .map_err(|e| AppError::Cli(format!("expected JSON object: {e}"))),
        ParamType::String => Ok(Value::String(value.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_coerce_int() {
        let schema = json!({"type": "integer"});
        assert_eq!(coerce_value("42", &schema).unwrap(), json!(42));
    }

    #[test]
    fn test_coerce_bool() {
        let schema = json!({"type": "boolean"});
        assert_eq!(coerce_value("true", &schema).unwrap(), json!(true));
        assert_eq!(coerce_value("false", &schema).unwrap(), json!(false));
    }

    #[test]
    fn test_coerce_array_csv() {
        let schema = json!({"type": "array"});
        assert_eq!(
            coerce_value("a,b,c", &schema).unwrap(),
            json!(["a", "b", "c"])
        );
    }

    #[test]
    fn test_coerce_array_json() {
        let schema = json!({"type": "array"});
        assert_eq!(coerce_value("[1,2,3]", &schema).unwrap(), json!([1, 2, 3]));
    }

    #[test]
    fn test_coerce_float() {
        let schema = json!({"type": "number"});
        assert_eq!(coerce_value("2.72", &schema).unwrap(), json!(2.72));
        assert!(coerce_value("not-a-number", &schema).is_err());
    }

    #[test]
    fn test_coerce_string() {
        let schema = json!({"type": "string"});
        assert_eq!(
            coerce_value("hello world", &schema).unwrap(),
            json!("hello world")
        );
    }

    #[test]
    fn test_coerce_object() {
        let schema = json!({"type": "object"});
        assert_eq!(
            coerce_value(r#"{"key":"val"}"#, &schema).unwrap(),
            json!({"key": "val"})
        );
        assert!(coerce_value("not-json", &schema).is_err());
    }

    #[test]
    fn test_schema_type_to_rust() {
        assert_eq!(
            schema_type_to_rust(&json!({"type": "integer"})),
            ParamType::Int
        );
        assert_eq!(
            schema_type_to_rust(&json!({"type": "number"})),
            ParamType::Float
        );
        assert_eq!(
            schema_type_to_rust(&json!({"type": "boolean"})),
            ParamType::Bool
        );
        assert_eq!(
            schema_type_to_rust(&json!({"type": "array"})),
            ParamType::Array
        );
        assert_eq!(
            schema_type_to_rust(&json!({"type": "object"})),
            ParamType::Object
        );
        assert_eq!(schema_type_to_rust(&json!({})), ParamType::String);
    }

    #[test]
    fn test_coerce_int_error() {
        let schema = json!({"type": "integer"});
        assert!(coerce_value("abc", &schema).is_err());
    }

    #[test]
    fn test_coerce_bool_variants() {
        let schema = json!({"type": "boolean"});
        assert_eq!(coerce_value("1", &schema).unwrap(), json!(true));
        assert_eq!(coerce_value("yes", &schema).unwrap(), json!(true));
        assert_eq!(coerce_value("0", &schema).unwrap(), json!(false));
        assert_eq!(coerce_value("no", &schema).unwrap(), json!(false));
        assert_eq!(coerce_value("TRUE", &schema).unwrap(), json!(true));
        assert!(coerce_value("maybe", &schema).is_err());
    }
}
