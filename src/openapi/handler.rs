use std::collections::HashMap;

use serde_json::Value;

use crate::cache::file_cache;
use crate::cli::dynamic::parse_dynamic_args;
use crate::core::coerce::coerce_value;
use crate::core::filter::filter_commands;
use crate::core::types::CommandDef;
use crate::error::{AppError, Result};
use crate::openapi::commands::extract_openapi_commands;
use crate::openapi::executor::execute_openapi;
use crate::openapi::refs::resolve_refs;
use crate::openapi::spec::load_spec;
use crate::output::format::output_result;
use crate::output::types::OutputOptions;

/// Options for the OpenAPI handler, bundling all CLI-derived configuration.
pub struct OpenApiHandlerOptions {
    pub spec_source: String,
    pub headers: HashMap<String, String>,
    pub output_opts: OutputOptions,
    pub list: bool,
    pub search: Option<String>,
    pub rest: Vec<String>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub methods: Vec<String>,
    pub base_url: Option<String>,
    pub cache_key: Option<String>,
    pub cache_ttl: u64,
    pub refresh: bool,
    // OAuth
    pub oauth_provider: Option<Box<dyn crate::oauth::provider::OAuthProvider>>,
}

/// Main OpenAPI handler — orchestrates spec loading, command extraction, and execution.
pub async fn handle_openapi(mut opts: OpenApiHandlerOptions) -> Result<()> {
    // 0. Inject OAuth auth header if provider is present
    if let Some(ref provider) = opts.oauth_provider {
        let header = provider.get_auth_header().await?;
        opts.headers.insert("Authorization".to_string(), header);
    }

    // 1. Load spec (with cache)
    let spec = load_spec_cached(&opts).await?;

    // 2. Resolve all $refs
    let mut seen = std::collections::HashSet::new();
    let resolved = resolve_refs(&spec, &spec, &mut seen);

    // 3. Extract commands
    let commands = extract_openapi_commands(&resolved);

    // 4. Apply filters (include/exclude/methods)
    let commands = filter_commands(commands, &opts.include, &opts.exclude, &opts.methods);

    // 5. Handle --list (grouped by path prefix)
    if opts.list {
        return display_command_list(&commands, &opts.output_opts);
    }

    // 6. Handle --search
    if let Some(ref keyword) = opts.search {
        let keyword_lower = keyword.to_lowercase();
        let filtered: Vec<_> = commands
            .iter()
            .filter(|cmd| {
                cmd.name.to_lowercase().contains(&keyword_lower)
                    || cmd.description.to_lowercase().contains(&keyword_lower)
            })
            .cloned()
            .collect();
        return display_command_list(&filtered, &opts.output_opts);
    }

    // 7. Parse dynamic args
    if opts.rest.is_empty() {
        return Err(AppError::Cli(
            "No command specified. Use --list to see available commands.".into(),
        ));
    }

    let parsed = parse_dynamic_args(&commands, &opts.rest)?;

    let cmd = commands
        .iter()
        .find(|c| c.name == parsed.command)
        .ok_or_else(|| AppError::Cli(format!("unknown command: {}", parsed.command)))?;

    // 8. Detect base URL
    let base_url = opts
        .base_url
        .clone()
        .or_else(|| extract_base_url(&resolved))
        .ok_or_else(|| {
            AppError::Cli(
                "No base URL found. Provide --base-url or ensure the spec has a servers[0].url"
                    .into(),
            )
        })?;

    // 9. Coerce args to typed JSON values
    let mut arguments: HashMap<String, Value> = HashMap::new();

    // If --stdin was passed, read JSON from stdin and merge as body
    if parsed.stdin {
        let stdin_value = crate::cli::stdin::read_stdin_json()?;
        if let Value::Object(map) = stdin_value {
            for (k, v) in map {
                arguments.insert(k, v);
            }
        } else {
            return Err(crate::error::AppError::Cli(
                "--stdin expects a JSON object from stdin".into(),
            ));
        }
    }

    for (key, value) in &parsed.args {
        let param = cmd.params.iter().find(|p| p.name == *key);
        if let Some(param) = param {
            let coerced = coerce_value(value, &param.schema)?;
            arguments.insert(param.original_name.clone(), coerced);
        } else {
            arguments.insert(key.clone(), Value::String(value.clone()));
        }
    }

    // 10. Execute request
    let result = execute_openapi(cmd, &arguments, &base_url, &opts.headers).await?;

    // 11. Output result
    output_result(&result, &opts.output_opts)
}

/// Load the OpenAPI spec, using cache if available.
async fn load_spec_cached(opts: &OpenApiHandlerOptions) -> Result<Value> {
    let cache_key = opts
        .cache_key
        .clone()
        .unwrap_or_else(|| file_cache::cache_key_for(&opts.spec_source));
    let ttl = opts.cache_ttl;

    let cached = if !opts.refresh {
        file_cache::load_cached(&cache_key, ttl).await
    } else {
        None
    };

    match cached {
        Some(v) => Ok(v),
        None => {
            let headers_vec: Vec<(String, String)> = opts
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let spec = load_spec(&opts.spec_source, &headers_vec).await?;
            let _ = file_cache::save_cache(&cache_key, &spec).await;
            Ok(spec)
        }
    }
}

/// Extract base URL from spec servers[0].url.
fn extract_base_url(spec: &Value) -> Option<String> {
    spec.get("servers")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .and_then(|server| server.get("url"))
        .and_then(|u| u.as_str())
        .map(|s| s.trim_end_matches('/').to_string())
}

/// Display commands grouped by path prefix.
fn display_command_list(commands: &[CommandDef], output_opts: &OutputOptions) -> Result<()> {
    // Group commands by path prefix (first path segment)
    let mut groups: std::collections::BTreeMap<String, Vec<&CommandDef>> =
        std::collections::BTreeMap::new();
    for cmd in commands {
        let group_key = cmd
            .path
            .as_deref()
            .and_then(|p| {
                let trimmed = p.trim_start_matches('/');
                trimmed.split('/').next()
            })
            .unwrap_or("other")
            .to_string();
        groups.entry(group_key).or_default().push(cmd);
    }

    let groups_json: Vec<Value> = groups
        .into_iter()
        .map(|(group, cmds)| {
            let commands_json: Vec<Value> = cmds
                .iter()
                .map(|cmd| {
                    let params: Vec<Value> = cmd
                        .params
                        .iter()
                        .map(|p| {
                            serde_json::json!({
                                "name": p.name,
                                "required": p.required,
                                "description": p.description,
                                "location": format!("{:?}", p.location),
                            })
                        })
                        .collect();

                    serde_json::json!({
                        "name": cmd.name,
                        "method": cmd.method,
                        "path": cmd.path,
                        "description": cmd.description,
                        "params": params,
                    })
                })
                .collect();

            serde_json::json!({
                "group": group,
                "commands": commands_json,
            })
        })
        .collect();

    output_result(&Value::Array(groups_json), output_opts)
}
