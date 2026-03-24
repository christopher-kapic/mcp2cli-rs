use std::io::{self, IsTerminal, Write};

use serde_json::Value;

use super::jq::run_jq;
use super::toon::run_toon;
use super::types::OutputOptions;
use crate::error::Result;

/// Apply head truncation to a value if it's an array.
#[cfg(test)]
fn apply_head(value: &Value, head: Option<usize>) -> Value {
    if let Some(n) = head {
        if let Value::Array(ref arr) = value {
            return Value::Array(arr.iter().take(n).cloned().collect());
        }
    }
    value.clone()
}

/// Format a value as a JSON string, choosing pretty or compact.
#[cfg(test)]
fn format_json(value: &Value, pretty: bool) -> std::result::Result<String, serde_json::Error> {
    if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
}

/// Format and output a JSON value according to the given options.
///
/// Applies head truncation, jq/toon piping, and pretty/raw formatting.
/// TTY detection determines default formatting when --pretty/--raw are not set.
pub fn output_result(value: &Value, opts: &OutputOptions) -> Result<()> {
    let mut val = value.clone();

    // Apply --head truncation for arrays
    if let Some(n) = opts.head {
        if let Value::Array(ref arr) = val {
            val = Value::Array(arr.iter().take(n).cloned().collect());
        }
    }

    // --raw: output text content as-is, no JSON wrapping
    if opts.raw {
        let text = match &val {
            Value::String(s) => s.clone(),
            _ => val.to_string(),
        };
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        write!(handle, "{text}")?;
        return Ok(());
    }

    // Serialize to JSON string (pretty or compact based on TTY / --pretty)
    let is_tty = io::stdout().is_terminal();
    let json_str = if opts.pretty || is_tty {
        serde_json::to_string_pretty(&val)?
    } else {
        serde_json::to_string(&val)?
    };

    // Pipe through jq if requested
    if let Some(ref expr) = opts.jq {
        let result = run_jq(&json_str, expr)?;
        print!("{result}");
        return Ok(());
    }

    // Pipe through toon if requested
    if opts.toon {
        let result = run_toon(&json_str)?;
        print!("{result}");
        return Ok(());
    }

    // Default output
    println!("{json_str}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_apply_head_truncates_array() {
        let val = json!([1, 2, 3, 4, 5]);
        let result = apply_head(&val, Some(3));
        assert_eq!(result, json!([1, 2, 3]));
    }

    #[test]
    fn test_apply_head_no_op_for_non_array() {
        let val = json!({"key": "value"});
        let result = apply_head(&val, Some(2));
        assert_eq!(result, val);
    }

    #[test]
    fn test_apply_head_none_returns_clone() {
        let val = json!([1, 2, 3]);
        let result = apply_head(&val, None);
        assert_eq!(result, val);
    }

    #[test]
    fn test_format_json_compact() {
        let val = json!({"a": 1});
        let result = format_json(&val, false).unwrap();
        assert_eq!(result, r#"{"a":1}"#);
    }

    #[test]
    fn test_format_json_pretty() {
        let val = json!({"a": 1});
        let result = format_json(&val, true).unwrap();
        assert!(result.contains('\n'));
        assert!(result.contains("\"a\": 1"));
    }

    #[test]
    fn test_apply_head_larger_than_array() {
        let val = json!([1, 2]);
        let result = apply_head(&val, Some(10));
        assert_eq!(result, json!([1, 2]));
    }
}
