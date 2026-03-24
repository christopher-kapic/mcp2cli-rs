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
pub fn resolve_secret(value: &str) -> Result<String> {
    if let Some(var) = value.strip_prefix("env:") {
        std::env::var(var)
            .map_err(|_| AppError::Cli(format!("environment variable '{var}' not set")))
    } else if let Some(path) = value.strip_prefix("file:") {
        std::fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .map_err(|e| AppError::Cli(format!("cannot read secret file '{path}': {e}")))
    } else {
        Ok(value.to_string())
    }
}

/// Parse "Key:Value" pairs into a HashMap.
pub fn parse_kv_list(items: &[String]) -> HashMap<String, String> {
    items
        .iter()
        .filter_map(|item| {
            let (k, v) = item.split_once(':')?;
            Some((k.trim().to_string(), v.trim().to_string()))
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
        let map = parse_kv_list(&items);
        assert_eq!(map.get("Authorization").unwrap(), "Bearer abc");
        assert_eq!(map.get("X-Api-Key").unwrap(), "123");
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
