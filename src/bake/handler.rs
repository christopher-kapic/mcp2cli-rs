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
        BakeAction::Create { name, args } => bake_create(&dir, &name, &args).await,
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
/// Supports: --mcp, --mcp-stdio, --spec, --graphql, --auth-header, --transport,
/// --cache-ttl, --oauth-client-id, --oauth-client-secret, --oauth-scope,
/// --include, --exclude, --methods, --description, --env, --base-url, --oauth
fn parse_bake_args(args: &[String]) -> Result<BakeConfig> {
    let mut config = BakeConfig::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--mcp" => {
                config.source_type = "mcp".to_string();
                config.source = next_val(args, &mut i, "--mcp")?;
            }
            "--mcp-stdio" => {
                config.source_type = "mcp-stdio".to_string();
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
                config
                    .auth_headers
                    .push(next_val(args, &mut i, "--auth-header")?);
            }
            "--env" => {
                config.env_vars.push(next_val(args, &mut i, "--env")?);
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
            "--oauth-client-id" => {
                config.oauth_client_id = Some(next_val(args, &mut i, "--oauth-client-id")?);
            }
            "--oauth-client-secret" => {
                config.oauth_client_secret = Some(next_val(args, &mut i, "--oauth-client-secret")?);
            }
            "--oauth-scope" => {
                config.oauth_scope = Some(next_val(args, &mut i, "--oauth-scope")?);
            }
            "--oauth" => {
                // Store as source-level hint; the source already captures the URL
                // but oauth server URL is separate — store in env_vars as MCP2CLI_OAUTH=<url>
                let val = next_val(args, &mut i, "--oauth")?;
                config.env_vars.push(format!("MCP2CLI_OAUTH={val}"));
            }
            "--include" => {
                config.include.push(next_val(args, &mut i, "--include")?);
            }
            "--exclude" => {
                config.exclude.push(next_val(args, &mut i, "--exclude")?);
            }
            "--methods" => {
                config.methods.push(next_val(args, &mut i, "--methods")?);
            }
            "--description" => {
                config.description = Some(next_val(args, &mut i, "--description")?);
            }
            "--base-url" => {
                // Store base-url in env_vars as MCP2CLI_BASE_URL=<url>
                let val = next_val(args, &mut i, "--base-url")?;
                config.env_vars.push(format!("MCP2CLI_BASE_URL={val}"));
            }
            other => {
                return Err(AppError::Cli(format!("Unknown bake argument: {other}")));
            }
        }
        i += 1;
    }

    if config.source.is_empty() {
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

async fn bake_create(dir: &std::path::Path, name: &str, args: &[String]) -> Result<()> {
    validate_name(name)?;
    let mut all = load_baked_all_from(dir).await?;
    if all.contains_key(name) {
        return Err(AppError::Cli(format!(
            "Baked config '{name}' already exists. Use 'bake update' to modify."
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
    // Tabular display: NAME | TYPE | SOURCE | DESCRIPTION
    println!("{:<20} {:<10} {:<40} DESCRIPTION", "NAME", "TYPE", "SOURCE");
    println!("{}", "-".repeat(80));
    let mut entries: Vec<_> = all.iter().collect();
    entries.sort_by_key(|(k, _)| (*k).clone());
    for (name, config) in entries {
        let desc = config.description.as_deref().unwrap_or("");
        let source = if config.source.len() > 38 {
            format!("{}...", &config.source[..35])
        } else {
            config.source.clone()
        };
        println!(
            "{:<20} {:<10} {:<40} {}",
            name, config.source_type, source, desc
        );
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

    let updates = parse_bake_args(args)?;
    let merged = merge_config(existing, updates);
    all.insert(name.to_string(), merged);
    save_baked_all_to(dir, &all).await?;
    eprintln!("Updated baked config '{name}'");
    Ok(())
}

/// Merge updates into an existing config. Non-default fields in `updates` override `base`.
fn merge_config(base: BakeConfig, updates: BakeConfig) -> BakeConfig {
    BakeConfig {
        source_type: if updates.source_type.is_empty() {
            base.source_type
        } else {
            updates.source_type
        },
        source: if updates.source.is_empty() {
            base.source
        } else {
            updates.source
        },
        auth_headers: if updates.auth_headers.is_empty() {
            base.auth_headers
        } else {
            updates.auth_headers
        },
        env_vars: if updates.env_vars.is_empty() {
            base.env_vars
        } else {
            updates.env_vars
        },
        cache_ttl: updates.cache_ttl.or(base.cache_ttl),
        transport: updates.transport.or(base.transport),
        oauth_client_id: updates.oauth_client_id.or(base.oauth_client_id),
        oauth_client_secret: updates.oauth_client_secret.or(base.oauth_client_secret),
        oauth_scope: updates.oauth_scope.or(base.oauth_scope),
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
        "mcp-stdio" => {
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
    for header in &config.auth_headers {
        argv.push("--auth-header".to_string());
        argv.push(header.clone());
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

    // Include/exclude/methods
    for inc in &config.include {
        argv.push("--include".to_string());
        argv.push(inc.clone());
    }
    for exc in &config.exclude {
        argv.push("--exclude".to_string());
        argv.push(exc.clone());
    }
    for method in &config.methods {
        argv.push("--methods".to_string());
        argv.push(method.clone());
    }

    // Env vars — split back into --env and special flags
    for env_var in &config.env_vars {
        if let Some(val) = env_var.strip_prefix("MCP2CLI_OAUTH=") {
            argv.push("--oauth".to_string());
            argv.push(val.to_string());
        } else if let Some(val) = env_var.strip_prefix("MCP2CLI_BASE_URL=") {
            argv.push("--base-url".to_string());
            argv.push(val.to_string());
        } else {
            argv.push("--env".to_string());
            argv.push(env_var.clone());
        }
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
        assert_eq!(config.transport, Some("sse".to_string()));
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
    fn test_baked_to_argv_round_trip() {
        let config = BakeConfig {
            source_type: "mcp".to_string(),
            source: "https://example.com/mcp".to_string(),
            auth_headers: vec!["Authorization: Bearer tok".to_string()],
            transport: Some("sse".to_string()),
            cache_ttl: Some(600),
            include: vec!["tool-*".to_string()],
            exclude: vec!["internal-*".to_string()],
            oauth_client_id: Some("my-client".to_string()),
            ..Default::default()
        };
        let argv = baked_to_argv(&config);
        assert!(argv.contains(&"--mcp".to_string()));
        assert!(argv.contains(&"https://example.com/mcp".to_string()));
        assert!(argv.contains(&"--auth-header".to_string()));
        assert!(argv.contains(&"--transport".to_string()));
        assert!(argv.contains(&"sse".to_string()));
        assert!(argv.contains(&"--cache-ttl".to_string()));
        assert!(argv.contains(&"600".to_string()));
        assert!(argv.contains(&"--include".to_string()));
        assert!(argv.contains(&"--exclude".to_string()));
        assert!(argv.contains(&"--oauth-client-id".to_string()));
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
    fn test_baked_to_argv_env_vars_with_special() {
        let config = BakeConfig {
            source_type: "mcp".to_string(),
            source: "https://example.com".to_string(),
            env_vars: vec![
                "MCP2CLI_OAUTH=https://auth.example.com".to_string(),
                "MCP2CLI_BASE_URL=https://api.example.com".to_string(),
                "MY_VAR=value".to_string(),
            ],
            ..Default::default()
        };
        let argv = baked_to_argv(&config);
        assert!(argv.contains(&"--oauth".to_string()));
        assert!(argv.contains(&"https://auth.example.com".to_string()));
        assert!(argv.contains(&"--base-url".to_string()));
        assert!(argv.contains(&"https://api.example.com".to_string()));
        assert!(argv.contains(&"--env".to_string()));
        assert!(argv.contains(&"MY_VAR=value".to_string()));
    }

    #[test]
    fn test_merge_config() {
        let base = BakeConfig {
            source_type: "mcp".to_string(),
            source: "https://old.example.com".to_string(),
            auth_headers: vec!["Old: header".to_string()],
            description: Some("old desc".to_string()),
            ..Default::default()
        };
        let updates = BakeConfig {
            source_type: "mcp".to_string(),
            source: "https://new.example.com".to_string(),
            description: Some("new desc".to_string()),
            ..Default::default()
        };
        let merged = merge_config(base, updates);
        assert_eq!(merged.source, "https://new.example.com");
        assert_eq!(merged.description, Some("new desc".to_string()));
        // auth_headers from base kept since updates was empty
        assert_eq!(merged.auth_headers, vec!["Old: header".to_string()]);
    }
}
