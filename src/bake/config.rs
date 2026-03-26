use crate::core::types::BakeConfig;
use crate::error::AppError;
use crate::error::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Return the config directory, respecting MCP2CLI_CONFIG_DIR env var.
pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MCP2CLI_CONFIG_DIR") {
        PathBuf::from(dir)
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".config")
            .join("mcp2cli")
    }
}

/// Path to the baked.json file within a given directory.
fn baked_json_path(dir: &Path) -> PathBuf {
    dir.join("baked.json")
}

/// Validate a bake config name: must match [a-z][a-z0-9-]*.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(AppError::Cli("Bake name cannot be empty".into()));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() {
        return Err(AppError::Cli(format!(
            "Bake name must start with a lowercase letter, got '{name}'"
        )));
    }
    for ch in chars {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '-' {
            return Err(AppError::Cli(format!(
                "Bake name can only contain lowercase letters, digits, and hyphens, got '{ch}' in '{name}'"
            )));
        }
    }
    Ok(())
}

/// Load all baked configs from the default config directory.
pub async fn load_baked_all() -> Result<HashMap<String, BakeConfig>> {
    load_baked_all_from(&config_dir()).await
}

/// Load all baked configs from a specific directory.
pub async fn load_baked_all_from(dir: &Path) -> Result<HashMap<String, BakeConfig>> {
    let path = baked_json_path(dir);
    match tokio::fs::read_to_string(&path).await {
        Ok(data) => {
            let configs: HashMap<String, BakeConfig> = serde_json::from_str(&data)?;
            Ok(configs)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(e) => Err(e.into()),
    }
}

/// Load a single baked config by name from the default config directory.
pub async fn load_baked(name: &str) -> Result<Option<BakeConfig>> {
    load_baked_from(&config_dir(), name).await
}

/// Load a single baked config by name from a specific directory.
pub async fn load_baked_from(dir: &Path, name: &str) -> Result<Option<BakeConfig>> {
    let all = load_baked_all_from(dir).await?;
    Ok(all.get(name).cloned())
}

/// Save all baked configs to the default config directory.
pub async fn save_baked_all(data: &HashMap<String, BakeConfig>) -> Result<()> {
    save_baked_all_to(&config_dir(), data).await
}

/// Save all baked configs to a specific directory.
pub async fn save_baked_all_to(dir: &Path, data: &HashMap<String, BakeConfig>) -> Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    let path = baked_json_path(dir);
    let json = serde_json::to_string_pretty(data)?;
    tokio::fs::write(&path, json).await?;
    Ok(())
}

/// Remove a baked config by name from the default config directory.
/// Returns true if the config existed and was removed.
pub async fn remove_baked(name: &str) -> Result<bool> {
    remove_baked_from(&config_dir(), name).await
}

/// Remove a baked config by name from a specific directory.
pub async fn remove_baked_from(dir: &Path, name: &str) -> Result<bool> {
    let mut all = load_baked_all_from(dir).await?;
    if all.remove(name).is_some() {
        save_baked_all_to(dir, &all).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Mask secrets in a BakeConfig for display purposes.
pub fn mask_secrets(config: &BakeConfig) -> BakeConfig {
    let mut masked = config.clone();
    masked.auth_headers = config
        .auth_headers
        .iter()
        .map(|h| {
            if let Some(colon_pos) = h.find(':') {
                let key = &h[..colon_pos];
                let value = h[colon_pos + 1..].trim();
                if is_secret_value(value) {
                    format!("{key}: ***")
                } else {
                    h.clone()
                }
            } else {
                h.clone()
            }
        })
        .collect();
    masked.oauth_client_secret = config
        .oauth_client_secret
        .as_ref()
        .map(|_| "***".to_string());
    masked
}

/// Check if a value looks like a secret that should be masked.
fn is_secret_value(value: &str) -> bool {
    value.starts_with("env:") || value.starts_with("file:") || value.len() > 20
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_name_valid() {
        assert!(validate_name("my-config").is_ok());
        assert!(validate_name("a").is_ok());
        assert!(validate_name("test123").is_ok());
        assert!(validate_name("my-api-v2").is_ok());
    }

    #[test]
    fn test_validate_name_invalid() {
        assert!(validate_name("").is_err());
        assert!(validate_name("123").is_err());
        assert!(validate_name("-start").is_err());
        assert!(validate_name("UPPER").is_err());
        assert!(validate_name("has_underscore").is_err());
        assert!(validate_name("has space").is_err());
    }

    #[tokio::test]
    async fn test_load_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let all = load_baked_all_from(dir.path()).await.unwrap();
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn test_save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut configs = HashMap::new();
        configs.insert(
            "my-api".to_string(),
            BakeConfig {
                source_type: "mcp".to_string(),
                source: "https://example.com/mcp".to_string(),
                description: Some("Test config".to_string()),
                ..Default::default()
            },
        );
        save_baked_all_to(dir.path(), &configs).await.unwrap();
        let loaded = load_baked_all_from(dir.path()).await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["my-api"].source, "https://example.com/mcp");
        assert_eq!(
            loaded["my-api"].description,
            Some("Test config".to_string())
        );
    }

    #[tokio::test]
    async fn test_load_single() {
        let dir = tempfile::tempdir().unwrap();
        let mut configs = HashMap::new();
        configs.insert(
            "api-a".to_string(),
            BakeConfig {
                source_type: "spec".to_string(),
                source: "https://a.com/spec".to_string(),
                ..Default::default()
            },
        );
        configs.insert(
            "api-b".to_string(),
            BakeConfig {
                source_type: "graphql".to_string(),
                source: "https://b.com/graphql".to_string(),
                ..Default::default()
            },
        );
        save_baked_all_to(dir.path(), &configs).await.unwrap();

        let a = load_baked_from(dir.path(), "api-a").await.unwrap();
        assert!(a.is_some());
        assert_eq!(a.unwrap().source_type, "spec");

        let missing = load_baked_from(dir.path(), "nope").await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_remove_baked() {
        let dir = tempfile::tempdir().unwrap();
        let mut configs = HashMap::new();
        configs.insert(
            "to-remove".to_string(),
            BakeConfig {
                source_type: "mcp".to_string(),
                source: "https://example.com".to_string(),
                ..Default::default()
            },
        );
        save_baked_all_to(dir.path(), &configs).await.unwrap();

        let removed = remove_baked_from(dir.path(), "to-remove").await.unwrap();
        assert!(removed);

        let all = load_baked_all_from(dir.path()).await.unwrap();
        assert!(all.is_empty());

        // Removing again returns false
        let removed_again = remove_baked_from(dir.path(), "to-remove").await.unwrap();
        assert!(!removed_again);
    }

    #[test]
    fn test_mask_secrets() {
        let config = BakeConfig {
            source_type: "mcp".to_string(),
            source: "https://example.com".to_string(),
            auth_headers: vec![
                "Authorization: env:MY_TOKEN".to_string(),
                "X-Custom: short".to_string(),
                "X-Key: this-is-a-very-long-secret-token-value".to_string(),
            ],
            oauth_client_secret: Some("super-secret".to_string()),
            ..Default::default()
        };
        let masked = mask_secrets(&config);
        assert_eq!(masked.auth_headers[0], "Authorization: ***");
        assert_eq!(masked.auth_headers[1], "X-Custom: short");
        assert_eq!(masked.auth_headers[2], "X-Key: ***");
        assert_eq!(masked.oauth_client_secret, Some("***".to_string()));
    }
}
