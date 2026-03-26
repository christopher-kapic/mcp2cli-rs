use crate::bake::config::{
    config_dir, load_baked_all_from, load_baked_from, mask_secrets, remove_baked_from,
    save_baked_all_to, validate_name,
};
use crate::cli::args::BakeAction;
use crate::core::types::BakeConfig;
use crate::error::{AppError, Result};

/// Dispatch bake subcommands.
pub async fn handle_bake(action: BakeAction) -> Result<()> {
    let dir = config_dir();
    match action {
        BakeAction::Create { name, force, args } => bake_create(&dir, &name, force, &args).await,
        BakeAction::List => bake_list(&dir).await,
        BakeAction::Show { name } => bake_show(&dir, &name).await,
        BakeAction::Remove { name } => bake_remove(&dir, &name).await,
        BakeAction::Update { name, args } => bake_update(&dir, &name, &args).await,
        BakeAction::Install {
            name,
            dir: inst_dir,
        } => crate::bake::install::bake_install(&dir, &name, inst_dir.as_deref()).await,
    }
}

/// Parse trailing args from `bake create NAME <args>` into a BakeConfig.
fn parse_bake_args(args: &[String]) -> Result<BakeConfig> {
    parse_bake_args_inner(args, true, false)
}

/// Parse trailing args for `bake update`, which only allows a subset of fields.
fn parse_bake_args_update(args: &[String]) -> Result<BakeConfig> {
    parse_bake_args_inner(args, false, true)
}

fn parse_bake_args_inner(
    args: &[String],
    require_source: bool,
    update_only: bool,
) -> Result<BakeConfig> {
    let mut config = BakeConfig::default();
    let mut i = 0;

    // Fields allowed during update (matching Python: cache-ttl, include, exclude,
    // methods, description, base-url, transport)
    let update_allowed = [
        "--cache-ttl",
        "--include",
        "--exclude",
        "--methods",
        "--description",
        "--base-url",
        "--transport",
    ];

    while i < args.len() {
        let arg = &args[i];

        if update_only && !update_allowed.contains(&arg.as_str()) {
            return Err(AppError::Cli(format!(
                "Cannot update field '{arg}'. Only --cache-ttl, --include, --exclude, --methods, --description, --base-url, and --transport can be updated."
            )));
        }

        match arg.as_str() {
            "--mcp" => {
                config.source_type = "mcp".to_string();
                config.source = next_val(args, &mut i, "--mcp")?;
            }
            "--mcp-stdio" => {
                config.source_type = "mcp_stdio".to_string();
                config.source = next_val(args, &mut i, "--mcp-stdio")?;
            }
            "--spec" => {
                config.source_type = "spec".to_string();
                config.source = next_val(args, &mut i, "--spec")?;
            }
            "--graphql" => {
                config.source_type = "graphql".to_string();
                config.source = next_val(args, &mut i, "--graphql")?;
            }
            "--auth-header" => {
                let val = next_val(args, &mut i, "--auth-header")?;
                if let Some((k, v)) = val.split_once(':') {
                    config
                        .auth_headers
                        .push((k.trim().to_string(), v.trim().to_string()));
                } else {
                    return Err(AppError::Cli(format!(
                        "Invalid --auth-header format: expected 'Key:Value', got '{val}'"
                    )));
                }
            }
            "--env" => {
                let val = next_val(args, &mut i, "--env")?;
                if let Some((key, value)) = val.split_once('=') {
                    config.env_vars.insert(key.to_string(), value.to_string());
                } else {
                    return Err(AppError::Cli(format!(
                        "Invalid --env format: expected 'KEY=VALUE', got '{val}'"
                    )));
                }
            }
            "--transport" => {
                config.transport = Some(next_val(args, &mut i, "--transport")?);
            }
            "--cache-ttl" => {
                let val = next_val(args, &mut i, "--cache-ttl")?;
                config.cache_ttl = Some(
                    val.parse::<u64>()
                        .map_err(|_| AppError::Cli(format!("Invalid --cache-ttl value: {val}")))?,
                );
            }
            "--oauth" => {
                config.oauth = Some(true);
            }
            "--oauth-client-id" => {
                config.oauth_client_id = Some(next_val(args, &mut i, "--oauth-client-id")?);
            }
            "--oauth-client-secret" => {
                config.oauth_client_secret =
                    Some(next_val(args, &mut i, "--oauth-client-secret")?);
            }
            "--oauth-scope" => {
                config.oauth_scope = Some(next_val(args, &mut i, "--oauth-scope")?);
            }
            "--include" => {
                let val = next_val(args, &mut i, "--include")?;
                // Support comma-separated values (matching Python)
                for part in val.split(',') {
                    let trimmed = part.trim();
                    if !trimmed.is_empty() {
                        config.include.push(trimmed.to_string());
                    }
                }
            }
            "--exclude" => {
                let val = next_val(args, &mut i, "--exclude")?;
                for part in val.split(',') {
                    let trimmed = part.trim();
                    if !trimmed.is_empty() {
                        config.exclude.push(trimmed.to_string());
                    }
                }
            }
            "--methods" => {
                config.methods.push(next_val(args, &mut i, "--methods")?);
            }
            "--description" => {
                config.description = Some(next_val(args, &mut i, "--description")?);
            }
            "--base-url" => {
                config.base_url = Some(next_val(args, &mut i, "--base-url")?);
            }
            other => {
                return Err(AppError::Cli(format!("Unknown bake argument: {other}")));
            }
        }
        i += 1;
    }

    if require_source && config.source.is_empty() {
        return Err(AppError::Cli(
            "Must specify a source: --mcp, --mcp-stdio, --spec, or --graphql".into(),
        ));
    }

    Ok(config)
}

/// Get the next value for a flag, advancing the index.
fn next_val(args: &[String], i: &mut usize, flag: &str) -> Result<String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| AppError::Cli(format!("Missing value for {flag}")))
}

async fn bake_create(
    dir: &std::path::Path,
    name: &str,
    force: bool,
    args: &[String],
) -> Result<()> {
    validate_name(name)?;
    let mut all = load_baked_all_from(dir).await?;
    if all.contains_key(name) && !force {
        return Err(AppError::Cli(format!(
            "Baked config '{name}' already exists. Use --force to overwrite or 'bake update' to modify."
        )));
    }
    let config = parse_bake_args(args)?;
    all.insert(name.to_string(), config);
    save_baked_all_to(dir, &all).await?;
    eprintln!("Created baked config '{name}'");
    Ok(())
}

async fn bake_list(dir: &std::path::Path) -> Result<()> {
    let all = load_baked_all_from(dir).await?;
    if all.is_empty() {
        println!("No baked configurations found.");
        return Ok(());
    }
    // Tabular display: NAME | TYPE | SOURCE
    println!("{:<20} {:<12} SOURCE", "NAME", "TYPE");
    println!("{}", "-".repeat(80));
    let mut entries: Vec<_> = all.iter().collect();
    entries.sort_by_key(|(k, _)| (*k).clone());
    for (name, config) in entries {
        let source = if config.source.len() > 48 {
            format!("{}...", &config.source[..45])
        } else {
            config.source.clone()
        };
        println!("{:<20} {:<12} {}", name, config.source_type, source);
    }
    Ok(())
}

async fn bake_show(dir: &std::path::Path, name: &str) -> Result<()> {
    let config = load_baked_from(dir, name)
        .await?
        .ok_or_else(|| AppError::Cli(format!("Baked config '{name}' not found")))?;
    let masked = mask_secrets(&config);
    let json = serde_json::to_string_pretty(&masked)?;
    println!("{json}");
    Ok(())
}

async fn bake_remove(dir: &std::path::Path, name: &str) -> Result<()> {
    let removed = remove_baked_from(dir, name).await?;
    if removed {
        // Also try to remove the wrapper script from default install dir
        let default_bin = dirs::home_dir().map(|h| h.join(".local/bin").join(name));
        if let Some(script_path) = default_bin {
            let _ = tokio::fs::remove_file(&script_path).await;
        }
        eprintln!("Removed baked config '{name}'");
    } else {
        return Err(AppError::Cli(format!("Baked config '{name}' not found")));
    }
    Ok(())
}

async fn bake_update(dir: &std::path::Path, name: &str, args: &[String]) -> Result<()> {
    let mut all = load_baked_all_from(dir).await?;
    let existing = all
        .get(name)
        .ok_or_else(|| AppError::Cli(format!("Baked config '{name}' not found")))?
        .clone();

    let updates = parse_bake_args_update(args)?;
    let merged = merge_config(existing, updates);
    all.insert(name.to_string(), merged);
    save_baked_all_to(dir, &all).await?;
    eprintln!("Updated baked config '{name}'");
    Ok(())
}

/// Merge updates into an existing config. Non-default fields in `updates` override `base`.
fn merge_config(base: BakeConfig, updates: BakeConfig) -> BakeConfig {
    BakeConfig {
        // Source and auth fields are not updatable (enforced by parse_bake_args_update),
        // but we keep this logic for completeness
        source_type: base.source_type,
        source: base.source,
        auth_headers: base.auth_headers,
        env_vars: base.env_vars,
        oauth: base.oauth,
        oauth_client_id: base.oauth_client_id,
        oauth_client_secret: base.oauth_client_secret,
        oauth_scope: base.oauth_scope,
        // Updatable fields
        cache_ttl: updates.cache_ttl.or(base.cache_ttl),
        transport: updates.transport.or(base.transport),
        include: if updates.include.is_empty() {
            base.include
        } else {
            updates.include
        },
        exclude: if updates.exclude.is_empty() {
            base.exclude
        } else {
            updates.exclude
        },
        methods: if updates.methods.is_empty() {
            base.methods
        } else {
            updates.methods
        },
        description: updates.description.or(base.description),
        base_url: updates.base_url.or(base.base_url),
    }
}

/// Convert a BakeConfig back to CLI argv fragments.
/// Used by @NAME detection to reconstruct the full CLI invocation.
pub fn baked_to_argv(config: &BakeConfig) -> Vec<String> {
    let mut argv = Vec::new();

    // Source flag
    match config.source_type.as_str() {
        "mcp" => {
            argv.push("--mcp".to_string());
            argv.push(config.source.clone());
        }
        "mcp_stdio" => {
            argv.push("--mcp-stdio".to_string());
            argv.push(config.source.clone());
        }
        "spec" => {
            argv.push("--spec".to_string());
            argv.push(config.source.clone());
        }
        "graphql" => {
            argv.push("--graphql".to_string());
            argv.push(config.source.clone());
        }
        _ => {
            // Unknown source type — try --mcp as fallback
            argv.push("--mcp".to_string());
            argv.push(config.source.clone());
        }
    }

    // Auth headers
    for (name, value) in &config.auth_headers {
        argv.push("--auth-header".to_string());
        argv.push(format!("{name}:{value}"));
    }

    // Transport
    if let Some(ref transport) = config.transport {
        argv.push("--transport".to_string());
        argv.push(transport.clone());
    }

    // Cache TTL
    if let Some(ttl) = config.cache_ttl {
        argv.push("--cache-ttl".to_string());
        argv.push(ttl.to_string());
    }

    // OAuth
    if config.oauth.unwrap_or(false) {
        argv.push("--oauth".to_string());
    }
    if let Some(ref client_id) = config.oauth_client_id {
        argv.push("--oauth-client-id".to_string());
        argv.push(client_id.clone());
    }
    if let Some(ref client_secret) = config.oauth_client_secret {
        argv.push("--oauth-client-secret".to_string());
        argv.push(client_secret.clone());
    }
    if let Some(ref scope) = config.oauth_scope {
        argv.push("--oauth-scope".to_string());
        argv.push(scope.clone());
    }

    // Note: include/exclude/methods are applied via BakeConfig object, not CLI args

    // Base URL
    if let Some(ref base_url) = config.base_url {
        argv.push("--base-url".to_string());
        argv.push(base_url.clone());
    }

    // Env vars
    for (key, value) in &config.env_vars {
        argv.push("--env".to_string());
        argv.push(format!("{key}={value}"));
    }

    argv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bake_args_mcp() {
        let args: Vec<String> = vec![
            "--mcp",
            "https://example.com/mcp",
            "--auth-header",
            "Authorization: Bearer token123",
            "--transport",
            "sse",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let config = parse_bake_args(&args).unwrap();
        assert_eq!(config.source_type, "mcp");
        assert_eq!(config.source, "https://example.com/mcp");
        assert_eq!(config.auth_headers.len(), 1);
        assert_eq!(
            config.auth_headers[0],
            ("Authorization".to_string(), "Bearer token123".to_string())
        );
        assert_eq!(config.transport, Some("sse".to_string()));
    }

    #[test]
    fn test_parse_bake_args_mcp_stdio() {
        let args: Vec<String> = vec!["--mcp-stdio", "my-server --port 8080"]
            .into_iter()
            .map(String::from)
            .collect();
        let config = parse_bake_args(&args).unwrap();
        assert_eq!(config.source_type, "mcp_stdio");
    }

    #[test]
    fn test_parse_bake_args_spec() {
        let args: Vec<String> = vec!["--spec", "https://petstore.io/spec.json"]
            .into_iter()
            .map(String::from)
            .collect();
        let config = parse_bake_args(&args).unwrap();
        assert_eq!(config.source_type, "spec");
    }

    #[test]
    fn test_parse_bake_args_missing_source() {
        let args: Vec<String> = vec!["--auth-header", "X: Y"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(parse_bake_args(&args).is_err());
    }

    #[test]
    fn test_parse_bake_args_oauth_boolean() {
        let args: Vec<String> = vec!["--mcp", "https://example.com", "--oauth"]
            .into_iter()
            .map(String::from)
            .collect();
        let config = parse_bake_args(&args).unwrap();
        assert_eq!(config.oauth, Some(true));
    }

    #[test]
    fn test_parse_bake_args_comma_separated_include() {
        let args: Vec<String> = vec!["--mcp", "https://example.com", "--include", "a,b,c"]
            .into_iter()
            .map(String::from)
            .collect();
        let config = parse_bake_args(&args).unwrap();
        assert_eq!(config.include, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_parse_bake_args_env_as_dict() {
        let args: Vec<String> = vec!["--mcp", "https://example.com", "--env", "KEY=VALUE"]
            .into_iter()
            .map(String::from)
            .collect();
        let config = parse_bake_args(&args).unwrap();
        assert_eq!(config.env_vars.get("KEY").unwrap(), "VALUE");
    }

    #[test]
    fn test_update_rejects_source_changes() {
        let args: Vec<String> = vec!["--mcp", "https://new.example.com"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(parse_bake_args_update(&args).is_err());
    }

    #[test]
    fn test_update_allows_cache_ttl() {
        let args: Vec<String> = vec!["--cache-ttl", "600"]
            .into_iter()
            .map(String::from)
            .collect();
        let config = parse_bake_args_update(&args).unwrap();
        assert_eq!(config.cache_ttl, Some(600));
    }

    #[test]
    fn test_baked_to_argv_round_trip() {
        let config = BakeConfig {
            source_type: "mcp".to_string(),
            source: "https://example.com/mcp".to_string(),
            auth_headers: vec![("Authorization".to_string(), "Bearer tok".to_string())],
            transport: Some("sse".to_string()),
            cache_ttl: Some(600),
            include: vec!["tool-*".to_string()],
            exclude: vec!["internal-*".to_string()],
            oauth: Some(true),
            oauth_client_id: Some("my-client".to_string()),
            ..Default::default()
        };
        let argv = baked_to_argv(&config);
        assert!(argv.contains(&"--mcp".to_string()));
        assert!(argv.contains(&"https://example.com/mcp".to_string()));
        assert!(argv.contains(&"--auth-header".to_string()));
        assert!(argv.contains(&"Authorization:Bearer tok".to_string()));
        assert!(argv.contains(&"--transport".to_string()));
        assert!(argv.contains(&"sse".to_string()));
        assert!(argv.contains(&"--cache-ttl".to_string()));
        assert!(argv.contains(&"600".to_string()));
        // include/exclude/methods are NOT emitted to argv (applied via BakeConfig object)
        assert!(!argv.contains(&"--include".to_string()));
        assert!(!argv.contains(&"--exclude".to_string()));
        assert!(argv.contains(&"--oauth".to_string()));
        assert!(argv.contains(&"--oauth-client-id".to_string()));
    }

    #[test]
    fn test_baked_to_argv_mcp_stdio() {
        let config = BakeConfig {
            source_type: "mcp_stdio".to_string(),
            source: "my-server --port 8080".to_string(),
            ..Default::default()
        };
        let argv = baked_to_argv(&config);
        assert_eq!(argv[0], "--mcp-stdio");
    }

    #[test]
    fn test_baked_to_argv_graphql() {
        let config = BakeConfig {
            source_type: "graphql".to_string(),
            source: "https://api.example.com/graphql".to_string(),
            ..Default::default()
        };
        let argv = baked_to_argv(&config);
        assert_eq!(argv[0], "--graphql");
        assert_eq!(argv[1], "https://api.example.com/graphql");
    }

    #[test]
    fn test_baked_to_argv_base_url() {
        let config = BakeConfig {
            source_type: "spec".to_string(),
            source: "https://example.com/spec.json".to_string(),
            base_url: Some("https://api.example.com".to_string()),
            ..Default::default()
        };
        let argv = baked_to_argv(&config);
        assert!(argv.contains(&"--base-url".to_string()));
        assert!(argv.contains(&"https://api.example.com".to_string()));
    }

    #[test]
    fn test_merge_config_preserves_source() {
        let base = BakeConfig {
            source_type: "mcp".to_string(),
            source: "https://old.example.com".to_string(),
            auth_headers: vec![("Old".to_string(), "header".to_string())],
            description: Some("old desc".to_string()),
            ..Default::default()
        };
        let updates = BakeConfig {
            description: Some("new desc".to_string()),
            cache_ttl: Some(600),
            ..Default::default()
        };
        let merged = merge_config(base, updates);
        // Source preserved from base
        assert_eq!(merged.source, "https://old.example.com");
        assert_eq!(merged.source_type, "mcp");
        // Auth preserved from base
        assert_eq!(merged.auth_headers.len(), 1);
        // Updated fields applied
        assert_eq!(merged.description, Some("new desc".to_string()));
        assert_eq!(merged.cache_ttl, Some(600));
    }
}
