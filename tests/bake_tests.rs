//! Integration tests for bake config CRUD, round-trip, name validation, filters, and install.

use mcp2cli::bake::config::{
    load_baked_all_from, load_baked_from, mask_secrets, remove_baked_from, save_baked_all_to,
    validate_name,
};
use mcp2cli::bake::handler::baked_to_argv;
use mcp2cli::core::types::BakeConfig;
use std::collections::HashMap;

// ── Name Validation ──────────────────────────────────────────────────────────

#[test]
fn test_name_valid_simple() {
    assert!(validate_name("my-api").is_ok());
}

#[test]
fn test_name_valid_single_char() {
    assert!(validate_name("a").is_ok());
}

#[test]
fn test_name_valid_alphanumeric() {
    assert!(validate_name("api2-v3").is_ok());
}

#[test]
fn test_name_rejects_empty() {
    assert!(validate_name("").is_err());
}

#[test]
fn test_name_rejects_leading_digit() {
    assert!(validate_name("123abc").is_err());
}

#[test]
fn test_name_rejects_leading_hyphen() {
    assert!(validate_name("-start").is_err());
}

#[test]
fn test_name_rejects_uppercase() {
    assert!(validate_name("MyApi").is_err());
}

#[test]
fn test_name_rejects_underscore() {
    assert!(validate_name("my_api").is_err());
}

#[test]
fn test_name_rejects_spaces() {
    assert!(validate_name("my api").is_err());
}

// ── CRUD: Create ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_create_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let mut configs = HashMap::new();
    configs.insert(
        "test-api".to_string(),
        BakeConfig {
            source_type: "mcp".to_string(),
            source: "https://example.com/mcp".to_string(),
            description: Some("A test API".to_string()),
            auth_headers: vec!["Authorization: Bearer tok".to_string()],
            transport: Some("sse".to_string()),
            cache_ttl: Some(600),
            ..Default::default()
        },
    );
    save_baked_all_to(dir.path(), &configs).await.unwrap();

    let loaded = load_baked_from(dir.path(), "test-api").await.unwrap();
    assert!(loaded.is_some());
    let cfg = loaded.unwrap();
    assert_eq!(cfg.source_type, "mcp");
    assert_eq!(cfg.source, "https://example.com/mcp");
    assert_eq!(cfg.description.as_deref(), Some("A test API"));
    assert_eq!(cfg.auth_headers.len(), 1);
    assert_eq!(cfg.transport.as_deref(), Some("sse"));
    assert_eq!(cfg.cache_ttl, Some(600));
}

// ── CRUD: List ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_empty() {
    let dir = tempfile::tempdir().unwrap();
    let all = load_baked_all_from(dir.path()).await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn test_list_multiple() {
    let dir = tempfile::tempdir().unwrap();
    let mut configs = HashMap::new();
    configs.insert(
        "alpha".to_string(),
        BakeConfig {
            source_type: "mcp".to_string(),
            source: "https://alpha.com".to_string(),
            ..Default::default()
        },
    );
    configs.insert(
        "beta".to_string(),
        BakeConfig {
            source_type: "spec".to_string(),
            source: "https://beta.com/spec.json".to_string(),
            ..Default::default()
        },
    );
    configs.insert(
        "gamma".to_string(),
        BakeConfig {
            source_type: "graphql".to_string(),
            source: "https://gamma.com/graphql".to_string(),
            ..Default::default()
        },
    );
    save_baked_all_to(dir.path(), &configs).await.unwrap();

    let loaded = load_baked_all_from(dir.path()).await.unwrap();
    assert_eq!(loaded.len(), 3);
    assert!(loaded.contains_key("alpha"));
    assert!(loaded.contains_key("beta"));
    assert!(loaded.contains_key("gamma"));
}

// ── CRUD: Show (single load) ────────────────────────────────────────────────

#[tokio::test]
async fn test_show_existing() {
    let dir = tempfile::tempdir().unwrap();
    let mut configs = HashMap::new();
    configs.insert(
        "my-api".to_string(),
        BakeConfig {
            source_type: "spec".to_string(),
            source: "https://api.example.com/openapi.json".to_string(),
            ..Default::default()
        },
    );
    save_baked_all_to(dir.path(), &configs).await.unwrap();

    let result = load_baked_from(dir.path(), "my-api").await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().source_type, "spec");
}

#[tokio::test]
async fn test_show_missing() {
    let dir = tempfile::tempdir().unwrap();
    let result = load_baked_from(dir.path(), "nonexistent").await.unwrap();
    assert!(result.is_none());
}

// ── CRUD: Remove ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_remove_existing() {
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
    configs.insert(
        "to-keep".to_string(),
        BakeConfig {
            source_type: "mcp".to_string(),
            source: "https://keep.com".to_string(),
            ..Default::default()
        },
    );
    save_baked_all_to(dir.path(), &configs).await.unwrap();

    let removed = remove_baked_from(dir.path(), "to-remove").await.unwrap();
    assert!(removed);

    let all = load_baked_all_from(dir.path()).await.unwrap();
    assert_eq!(all.len(), 1);
    assert!(all.contains_key("to-keep"));
    assert!(!all.contains_key("to-remove"));
}

#[tokio::test]
async fn test_remove_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    let removed = remove_baked_from(dir.path(), "nope").await.unwrap();
    assert!(!removed);
}

// ── CRUD: Update ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_update_existing() {
    let dir = tempfile::tempdir().unwrap();
    let mut configs = HashMap::new();
    configs.insert(
        "my-api".to_string(),
        BakeConfig {
            source_type: "mcp".to_string(),
            source: "https://old.example.com".to_string(),
            auth_headers: vec!["Authorization: Bearer old-tok".to_string()],
            description: Some("old desc".to_string()),
            ..Default::default()
        },
    );
    save_baked_all_to(dir.path(), &configs).await.unwrap();

    // Simulate an update that changes description (source stays the same in update mode)
    let mut all = load_baked_all_from(dir.path()).await.unwrap();
    let existing = all.get("my-api").unwrap().clone();
    let merged = BakeConfig {
        description: Some("new desc".to_string()),
        ..existing
    };
    all.insert("my-api".to_string(), merged);
    save_baked_all_to(dir.path(), &all).await.unwrap();

    let loaded = load_baked_from(dir.path(), "my-api")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.source, "https://old.example.com");
    assert_eq!(loaded.description.as_deref(), Some("new desc"));
    assert_eq!(
        loaded.auth_headers,
        vec!["Authorization: Bearer old-tok".to_string()]
    );
}

// ── Round-trip: baked_to_argv ────────────────────────────────────────────────

#[test]
fn test_round_trip_mcp() {
    let config = BakeConfig {
        source_type: "mcp".to_string(),
        source: "https://example.com/mcp".to_string(),
        auth_headers: vec!["Authorization: Bearer token123".to_string()],
        transport: Some("sse".to_string()),
        cache_ttl: Some(300),
        include: vec!["tool-*".to_string()],
        exclude: vec!["internal-*".to_string()],
        methods: vec!["GET".to_string(), "POST".to_string()],
        oauth: Some(true),
        oauth_client_id: Some("my-client".to_string()),
        oauth_client_secret: Some("my-secret".to_string()),
        oauth_scope: Some("read write".to_string()),
        ..Default::default()
    };
    let argv = baked_to_argv(&config);

    // Verify all fields present in argv
    assert!(argv.contains(&"--mcp".to_string()));
    assert!(argv.contains(&"https://example.com/mcp".to_string()));
    assert!(argv.contains(&"--auth-header".to_string()));
    assert!(argv.contains(&"Authorization: Bearer token123".to_string()));
    assert!(argv.contains(&"--transport".to_string()));
    assert!(argv.contains(&"sse".to_string()));
    assert!(argv.contains(&"--cache-ttl".to_string()));
    assert!(argv.contains(&"300".to_string()));
    assert!(argv.contains(&"--include".to_string()));
    assert!(argv.contains(&"tool-*".to_string()));
    assert!(argv.contains(&"--exclude".to_string()));
    assert!(argv.contains(&"internal-*".to_string()));
    assert!(argv.contains(&"--methods".to_string()));
    assert!(argv.contains(&"GET".to_string()));
    assert!(argv.contains(&"POST".to_string()));
    assert!(argv.contains(&"--oauth".to_string()));
    assert!(argv.contains(&"--oauth-client-id".to_string()));
    assert!(argv.contains(&"my-client".to_string()));
    assert!(argv.contains(&"--oauth-client-secret".to_string()));
    assert!(argv.contains(&"my-secret".to_string()));
    assert!(argv.contains(&"--oauth-scope".to_string()));
    assert!(argv.contains(&"read write".to_string()));
}

#[test]
fn test_round_trip_spec_with_base_url() {
    let config = BakeConfig {
        source_type: "spec".to_string(),
        source: "https://petstore.io/spec.json".to_string(),
        base_url: Some("https://api.petstore.io".to_string()),
        ..Default::default()
    };
    let argv = baked_to_argv(&config);
    assert_eq!(argv[0], "--spec");
    assert_eq!(argv[1], "https://petstore.io/spec.json");
    assert!(argv.contains(&"--base-url".to_string()));
    assert!(argv.contains(&"https://api.petstore.io".to_string()));
}

#[test]
fn test_round_trip_graphql() {
    let config = BakeConfig {
        source_type: "graphql".to_string(),
        source: "https://api.example.com/graphql".to_string(),
        ..Default::default()
    };
    let argv = baked_to_argv(&config);
    assert_eq!(argv[0], "--graphql");
    assert_eq!(argv[1], "https://api.example.com/graphql");
    assert_eq!(argv.len(), 2);
}

#[test]
fn test_round_trip_mcp_stdio() {
    let config = BakeConfig {
        source_type: "mcp_stdio".to_string(),
        source: "npx some-server".to_string(),
        ..Default::default()
    };
    let argv = baked_to_argv(&config);
    assert_eq!(argv[0], "--mcp-stdio");
    assert_eq!(argv[1], "npx some-server");
}

#[test]
fn test_round_trip_env_vars() {
    let mut env_vars = HashMap::new();
    env_vars.insert("MY_CUSTOM_VAR".to_string(), "value".to_string());
    let config = BakeConfig {
        source_type: "mcp".to_string(),
        source: "https://example.com".to_string(),
        env_vars,
        ..Default::default()
    };
    let argv = baked_to_argv(&config);
    assert!(argv.contains(&"--env".to_string()));
    assert!(argv.contains(&"MY_CUSTOM_VAR=value".to_string()));
}

// ── Filter application from baked config ─────────────────────────────────────

#[test]
fn test_baked_config_filters_in_argv() {
    let config = BakeConfig {
        source_type: "spec".to_string(),
        source: "https://api.example.com/spec.json".to_string(),
        include: vec!["pets-*".to_string(), "users-*".to_string()],
        exclude: vec!["admin-*".to_string()],
        methods: vec!["GET".to_string()],
        ..Default::default()
    };
    let argv = baked_to_argv(&config);

    // Count --include occurrences
    let include_count = argv.iter().filter(|a| a.as_str() == "--include").count();
    assert_eq!(include_count, 2);
    assert!(argv.contains(&"pets-*".to_string()));
    assert!(argv.contains(&"users-*".to_string()));

    let exclude_count = argv.iter().filter(|a| a.as_str() == "--exclude").count();
    assert_eq!(exclude_count, 1);
    assert!(argv.contains(&"admin-*".to_string()));

    let methods_count = argv.iter().filter(|a| a.as_str() == "--methods").count();
    assert_eq!(methods_count, 1);
    assert!(argv.contains(&"GET".to_string()));
}

// ── Secret masking ───────────────────────────────────────────────────────────

#[test]
fn test_mask_secrets_env_prefix() {
    let config = BakeConfig {
        source_type: "mcp".to_string(),
        source: "https://example.com".to_string(),
        auth_headers: vec!["Authorization: env:MY_TOKEN".to_string()],
        ..Default::default()
    };
    let masked = mask_secrets(&config);
    assert_eq!(masked.auth_headers[0], "Authorization: ***");
}

#[test]
fn test_mask_secrets_long_value() {
    let config = BakeConfig {
        source_type: "mcp".to_string(),
        source: "https://example.com".to_string(),
        auth_headers: vec!["X-Key: abcdefghij1234567890x".to_string()],
        ..Default::default()
    };
    let masked = mask_secrets(&config);
    assert_eq!(masked.auth_headers[0], "X-Key: ***");
}

#[test]
fn test_mask_secrets_short_value_not_masked() {
    let config = BakeConfig {
        source_type: "mcp".to_string(),
        source: "https://example.com".to_string(),
        auth_headers: vec!["X-Custom: short".to_string()],
        ..Default::default()
    };
    let masked = mask_secrets(&config);
    assert_eq!(masked.auth_headers[0], "X-Custom: short");
}

#[test]
fn test_mask_secrets_oauth_client_secret() {
    let config = BakeConfig {
        source_type: "mcp".to_string(),
        source: "https://example.com".to_string(),
        oauth_client_secret: Some("super-secret-value".to_string()),
        ..Default::default()
    };
    let masked = mask_secrets(&config);
    assert_eq!(masked.oauth_client_secret, Some("***".to_string()));
}

#[test]
fn test_mask_secrets_no_oauth_secret() {
    let config = BakeConfig {
        source_type: "mcp".to_string(),
        source: "https://example.com".to_string(),
        oauth_client_secret: None,
        ..Default::default()
    };
    let masked = mask_secrets(&config);
    assert_eq!(masked.oauth_client_secret, None);
}

// ── Install script generation ────────────────────────────────────────────────

#[tokio::test]
async fn test_install_generates_script() {
    let config_dir = tempfile::tempdir().unwrap();
    let install_dir = tempfile::tempdir().unwrap();

    // Save a config first
    let mut configs = HashMap::new();
    configs.insert(
        "my-tool".to_string(),
        BakeConfig {
            source_type: "mcp".to_string(),
            source: "https://example.com/mcp".to_string(),
            ..Default::default()
        },
    );
    save_baked_all_to(config_dir.path(), &configs)
        .await
        .unwrap();

    // Install
    mcp2cli::bake::install::bake_install(
        config_dir.path(),
        "my-tool",
        Some(install_dir.path().to_str().unwrap()),
    )
    .await
    .unwrap();

    // Verify script contains the expected structure (binary path may vary)
    let script_path = install_dir.path().join("my-tool");
    let content = tokio::fs::read_to_string(&script_path).await.unwrap();
    assert!(content.starts_with("#!/bin/sh\nexec "));
    assert!(content.contains("@my-tool"));
    assert!(content.contains("\"$@\""));

    // Verify executable permission on unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = tokio::fs::metadata(&script_path).await.unwrap();
        let mode = meta.permissions().mode();
        assert_eq!(mode & 0o755, 0o755);
    }
}

#[tokio::test]
async fn test_install_missing_config_fails() {
    let config_dir = tempfile::tempdir().unwrap();
    let install_dir = tempfile::tempdir().unwrap();

    let result = mcp2cli::bake::install::bake_install(
        config_dir.path(),
        "nonexistent",
        Some(install_dir.path().to_str().unwrap()),
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_install_invalid_name_fails() {
    let config_dir = tempfile::tempdir().unwrap();
    let install_dir = tempfile::tempdir().unwrap();

    let result = mcp2cli::bake::install::bake_install(
        config_dir.path(),
        "INVALID",
        Some(install_dir.path().to_str().unwrap()),
    )
    .await;
    assert!(result.is_err());
}
