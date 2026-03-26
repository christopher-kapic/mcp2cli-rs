use clap::Parser;
use mcp2cli::bake::handler::baked_to_argv;
use mcp2cli::cli::args::{Cli, Commands};
use mcp2cli::core::helpers::parse_kv_list;
use mcp2cli::core::types::BakeConfig;
use mcp2cli::error::AppError;
use mcp2cli::graphql::handler::GraphqlHandlerOptions;
use mcp2cli::mcp::handler::McpHandlerOptions;
use mcp2cli::openapi::handler::OpenApiHandlerOptions;
use mcp2cli::output::types::OutputOptions;
use std::process;

const DEFAULT_CACHE_TTL: u64 = 3600;

#[tokio::main]
async fn main() {
    // Intercept internal --session-daemon flag before clap parsing
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 5 && args[1] == "--session-daemon" {
        let code = match mcp2cli::session::daemon::daemon_main(
            &args[2],
            &args[3],
            &args[4],
            args.get(5).map(|s| s.as_str()).unwrap_or("{}"),
        )
        .await
        {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("Session daemon error: {e}");
                1
            }
        };
        process::exit(code);
    }

    let exit_code = match run().await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e}");
            e.exit_code()
        }
    };
    process::exit(exit_code);
}

async fn run() -> mcp2cli::error::Result<()> {
    // Check for @NAME baked config before parsing
    let args: Vec<String> = std::env::args().collect();
    let (argv, bake_config) = detect_baked_config(&args).await;

    let cli = Cli::parse_from(&argv);

    // Validate --jq and --toon mutual exclusivity (matching Python)
    if cli.jq.is_some() && cli.toon {
        return Err(AppError::Cli(
            "Cannot use --jq and --toon together".into(),
        ));
    }

    // Build output options from CLI flags
    let output_opts = OutputOptions {
        pretty: cli.pretty,
        raw: cli.raw,
        toon: cli.toon,
        jq: cli.jq.clone(),
        head: cli.head,
    };

    // Parse auth headers (values support env:VAR and file:/path prefixes)
    let auth_headers = parse_kv_list(&cli.auth_header, ':', true);

    // Extract include/exclude/methods filters from bake config (not global CLI flags)
    let include = bake_config
        .as_ref()
        .map(|b| b.include.clone())
        .unwrap_or_default();
    let exclude = bake_config
        .as_ref()
        .map(|b| b.exclude.clone())
        .unwrap_or_default();
    let methods = bake_config
        .as_ref()
        .map(|b| b.methods.clone())
        .unwrap_or_default();

    // Parse --env vars into key-value pairs for subprocess injection
    let env_vars: Vec<(String, String)> = cli
        .env
        .iter()
        .filter_map(|item| {
            let (k, v) = item.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect();

    // Build OAuth provider if OAuth flags are present
    let oauth_provider = build_oauth_provider_from_cli(&cli);

    // Handle bake subcommand
    if let Some(Commands::Bake { action }) = cli.command {
        return mcp2cli::bake::handler::handle_bake(action).await;
    }

    // Handle session management flags
    if let Some(ref name) = cli.session_start {
        let source = cli
            .mcp
            .as_deref()
            .or(cli.mcp_stdio.as_deref())
            .ok_or_else(|| {
                AppError::Cli(
                    "--session-start requires --mcp or --mcp-stdio to specify the source".into(),
                )
            })?;
        let transport = if cli.mcp_stdio.is_some() {
            "stdio"
        } else {
            cli.transport.as_str()
        };
        let headers_map: std::collections::HashMap<String, String> =
            auth_headers.into_iter().collect();
        return mcp2cli::session::manager::session_start(name, source, transport, &headers_map)
            .await;
    }

    if let Some(ref name) = cli.session_stop {
        return mcp2cli::session::manager::session_stop(name).await;
    }

    if cli.session_list {
        let entries = mcp2cli::session::manager::session_list().await?;
        if entries.is_empty() {
            eprintln!("No active sessions.");
        } else {
            println!(
                "{:<15} {:<8} {:<6} {:<30} TRANSPORT",
                "NAME", "PID", "ALIVE", "SOURCE"
            );
            for entry in &entries {
                println!(
                    "{:<15} {:<8} {:<6} {:<30} {}",
                    entry.name,
                    entry.pid,
                    if entry.alive { "yes" } else { "no" },
                    entry.source,
                    entry.transport,
                );
            }
        }
        return Ok(());
    }

    // Detect source mode and route
    if let Some(spec) = &cli.spec {
        // OpenAPI mode
        let opts = OpenApiHandlerOptions {
            spec_source: spec.clone(),
            headers: auth_headers,
            output_opts,
            list: cli.list,
            search: cli.search.clone(),
            rest: cli.rest.clone(),
            include: include.clone(),
            exclude: exclude.clone(),
            methods: methods.clone(),
            base_url: cli.base_url.clone(),
            cache_key: cli.cache_key.clone(),
            cache_ttl: cli.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL),
            refresh: cli.refresh,
            oauth_provider,
        };
        mcp2cli::openapi::handler::handle_openapi(opts).await
    } else if cli.session.is_some() || cli.mcp.is_some() || cli.mcp_stdio.is_some() {
        // MCP mode: session, HTTP/SSE, or stdio
        let (url, transport, session_client) = if let Some(ref session_name) = cli.session {
            // Session mode: route through SessionMcpClient
            let socket_path = mcp2cli::session::manager::session_socket_path(session_name);
            if !socket_path.exists() {
                return Err(AppError::Cli(format!(
                    "Session '{session_name}' not found. Use --session-start to create it."
                )));
            }
            let client = mcp2cli::session::client::SessionMcpClient::new(socket_path);
            (
                session_name.clone(),
                "session".to_string(),
                Some(Box::new(client) as Box<dyn mcp2cli::mcp::protocol::McpClient>),
            )
        } else if let Some(ref mcp_cmd) = cli.mcp_stdio {
            (mcp_cmd.clone(), "stdio".to_string(), None)
        } else {
            (cli.mcp.clone().unwrap(), cli.transport.clone(), None)
        };

        let prompt_args = parse_kv_list(&cli.prompt_arg, '=', false);
        let opts = McpHandlerOptions {
            url,
            transport,
            headers: auth_headers,
            output_opts,
            list: cli.list,
            search: cli.search.clone(),
            rest: cli.rest.clone(),
            include: include.clone(),
            exclude: exclude.clone(),
            cache_key: cli.cache_key.clone(),
            cache_ttl: cli.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL),
            refresh: cli.refresh,
            list_resources: cli.list_resources,
            list_resource_templates: cli.list_resource_templates,
            read_resource: cli.read_resource.clone(),
            list_prompts: cli.list_prompts,
            get_prompt: cli.get_prompt.clone(),
            prompt_args,
            oauth_provider,
            session_client,
            env_vars,
        };
        mcp2cli::mcp::handler::handle_mcp(opts).await
    } else if let Some(graphql_url) = &cli.graphql {
        // GraphQL mode
        let opts = GraphqlHandlerOptions {
            url: graphql_url.clone(),
            headers: auth_headers,
            output_opts,
            list: cli.list,
            search: cli.search.clone(),
            rest: cli.rest.clone(),
            include: include.clone(),
            exclude: exclude.clone(),
            fields: cli.fields.clone(),
            cache_key: cli.cache_key.clone(),
            cache_ttl: cli.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL),
            refresh: cli.refresh,
            oauth_provider,
        };
        mcp2cli::graphql::handler::handle_graphql(opts).await
    } else if cli.list || cli.search.is_some() {
        Err(AppError::Cli(
            "No source specified. Use --spec, --mcp, --mcp-stdio, or --graphql".into(),
        ))
    } else {
        Err(AppError::Cli(
            "No source specified. Use --spec, --mcp, --mcp-stdio, or --graphql, or use 'bake' subcommand.\nRun with --help for usage information.".into(),
        ))
    }
}

/// Build an OAuth provider from CLI flags, if OAuth is configured.
/// Supports --mcp, --graphql, and --spec (with --base-url) as OAuth server sources.
fn build_oauth_provider_from_cli(
    cli: &Cli,
) -> Option<Box<dyn mcp2cli::oauth::provider::OAuthProvider>> {
    if !cli.oauth {
        return None;
    }

    // Derive OAuth server URL: try --mcp, then --graphql, then --base-url (for --spec)
    let oauth_server = cli
        .mcp
        .as_deref()
        .or(cli.graphql.as_deref())
        .or_else(|| {
            if cli.spec.is_some() {
                cli.base_url.as_deref()
            } else {
                None
            }
        })?;

    let client_id = cli.oauth_client_id.as_deref()?;

    let token_endpoint = format!("{}/token", oauth_server.trim_end_matches('/'));
    let auth_endpoint = format!("{}/authorize", oauth_server.trim_end_matches('/'));

    Some(mcp2cli::oauth::provider::build_oauth_provider(
        oauth_server,
        client_id,
        cli.oauth_client_secret.as_deref(),
        &token_endpoint,
        Some(&auth_endpoint),
        cli.oauth_scope.clone(),
    ))
}

/// Detect @NAME pattern in argv and expand to baked config args.
/// If argv[1] starts with '@', load the baked config and reconstruct args.
/// Returns the expanded argv and the BakeConfig (if any) for filter extraction.
async fn detect_baked_config(args: &[String]) -> (Vec<String>, Option<BakeConfig>) {
    if args.len() > 1 && args[1].starts_with('@') {
        let name = &args[1][1..];
        match mcp2cli::bake::config::load_baked(name).await {
            Ok(Some(config)) => {
                let mut new_argv = vec![args[0].clone()];
                new_argv.extend(baked_to_argv(&config));
                if args.len() > 2 {
                    new_argv.extend_from_slice(&args[2..]);
                }
                (new_argv, Some(config))
            }
            Ok(None) => {
                eprintln!("Warning: baked config '{name}' not found, passing through");
                (args.to_vec(), None)
            }
            Err(e) => {
                eprintln!("Warning: failed to load baked config '{name}': {e}");
                (args.to_vec(), None)
            }
        }
    } else {
        (args.to_vec(), None)
    }
}
