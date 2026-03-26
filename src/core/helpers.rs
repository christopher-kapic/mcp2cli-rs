use crate::error::{AppError, Result};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static CAMEL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([a-z0-9])([A-Z])").unwrap());

/// Convert camelCase or snake_case to kebab-case.
pub fn to_kebab(name: &str) -> String {
    // camelCase -> camel-Case
    let s = CAMEL_RE.replace_all(name, "${1}-${2}");
    // snake_case -> snake-case, then lowercase
    s.replace('_', "-").to_lowercase()
}

/// Resolve a secret value: env:VAR, file:/path, or literal.
///
/// When the entire value starts with `env:` or `file:`, the whole value is
/// resolved. Otherwise, inline `env:VAR_NAME` references are expanded within
/// the string (e.g. `"Bearer env:MY_TOKEN"` → `"Bearer <token-value>"`).
pub fn resolve_secret(value: &str) -> Result<String> {
    if let Some(var) = value.strip_prefix("env:") {
        std::env::var(var)
            .map_err(|_| AppError::Cli(format!("environment variable '{var}' not set")))
    } else if let Some(path) = value.strip_prefix("file:") {
        std::fs::read_to_string(path)
            .map(|s| s.trim_end_matches('\n').to_string())
            .map_err(|e| AppError::Cli(format!("cannot read secret file '{path}': {e}")))
    } else if value.contains("env:") {
        // Inline env: references — replace each env:VAR_NAME with its value
        let mut result = value.to_string();
        while let Some(start) = result.find("env:") {
            let rest = &result[start + 4..];
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            let var = &rest[..end];
            let resolved = std::env::var(var)
                .map_err(|_| AppError::Cli(format!("environment variable '{var}' not set")))?;
            result = format!("{}{}{}", &result[..start], resolved, &rest[end..]);
        }
        Ok(result)
    } else {
        Ok(value.to_string())
    }
}

/// Parse key-value pairs into a HashMap, splitting on the given `delimiter`.
/// When `resolve_values` is true, each value is passed through [`resolve_secret`]
/// so that `env:VAR` and `file:/path` prefixes are resolved at runtime.
pub fn parse_kv_list(
    items: &[String],
    delimiter: char,
    resolve_values: bool,
) -> HashMap<String, String> {
    items
        .iter()
        .filter_map(|item| {
            let (k, v) = item.split_once(delimiter)?;
            let v = v.trim().to_string();
            let v = if resolve_values {
                match resolve_secret(&v) {
                    Ok(resolved) => resolved,
                    Err(e) => {
                        eprintln!("Warning: {e}");
                        v
                    }
                }
            } else {
                v
            };
            Some((k.trim().to_string(), v))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_kebab_camel() {
        assert_eq!(to_kebab("getUserById"), "get-user-by-id");
    }

    #[test]
    fn test_to_kebab_snake() {
        assert_eq!(to_kebab("get_user_by_id"), "get-user-by-id");
    }

    #[test]
    fn test_to_kebab_already_kebab() {
        assert_eq!(to_kebab("get-user"), "get-user");
    }

    #[test]
    fn test_parse_kv_list() {
        let items = vec!["Authorization: Bearer abc".into(), "X-Api-Key:123".into()];
        let map = parse_kv_list(&items, ':', false);
        assert_eq!(map.get("Authorization").unwrap(), "Bearer abc");
        assert_eq!(map.get("X-Api-Key").unwrap(), "123");
    }

    #[test]
    fn test_parse_kv_list_equals_delimiter() {
        let items = vec!["key1=value1".into(), "key2=value2".into()];
        let map = parse_kv_list(&items, '=', false);
        assert_eq!(map.get("key1").unwrap(), "value1");
        assert_eq!(map.get("key2").unwrap(), "value2");
    }

    #[test]
    fn test_parse_kv_list_resolve_env() {
        std::env::set_var("MCP2CLI_TEST_KV", "resolved-token");
        let items = vec!["Authorization: Bearer env:MCP2CLI_TEST_KV".into()];
        let map = parse_kv_list(&items, ':', true);
        assert_eq!(
            map.get("Authorization").unwrap(),
            "Bearer resolved-token"
        );
        std::env::remove_var("MCP2CLI_TEST_KV");
    }

    #[test]
    fn test_resolve_secret_literal() {
        assert_eq!(resolve_secret("my-secret").unwrap(), "my-secret");
    }

    #[test]
    fn test_resolve_secret_env() {
        std::env::set_var("MCP2CLI_TEST_SECRET", "test-value");
        assert_eq!(
            resolve_secret("env:MCP2CLI_TEST_SECRET").unwrap(),
            "test-value"
        );
        std::env::remove_var("MCP2CLI_TEST_SECRET");
    }
}
