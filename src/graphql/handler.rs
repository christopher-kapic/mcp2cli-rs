use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::cache::file_cache;
use crate::cli::dynamic::parse_dynamic_args;
use crate::core::coerce::coerce_value;
use crate::core::filter::filter_commands;
use crate::core::types::CommandDef;
use crate::error::{AppError, Result};
use crate::graphql::commands::extract_graphql_commands;
use crate::graphql::executor::execute_graphql;
use crate::graphql::introspection::{load_graphql_schema, IntrospectionSchema};
use crate::output::format::output_result;
use crate::output::types::OutputOptions;

/// Options for the GraphQL handler, bundling all CLI-derived configuration.
pub struct GraphqlHandlerOptions {
    pub url: String,
    pub headers: HashMap<String, String>,
    pub output_opts: OutputOptions,
    pub list: bool,
    pub search: Option<String>,
    pub rest: Vec<String>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub fields: Option<String>,
    pub cache_key: Option<String>,
    pub cache_ttl: u64,
    pub refresh: bool,
    // OAuth
    pub oauth_provider: Option<Box<dyn crate::oauth::provider::OAuthProvider>>,
}

/// Main GraphQL handler — orchestrates schema introspection, command extraction, and execution.
pub async fn handle_graphql(mut opts: GraphqlHandlerOptions) -> Result<()> {
    // 0. Inject OAuth auth header if provider is present
    if let Some(ref provider) = opts.oauth_provider {
        let header = provider.get_auth_header().await?;
        opts.headers.insert("Authorization".to_string(), header);
    }

    // 1. Load schema via introspection (with cache)
    let schema = load_schema_cached(&opts).await?;

    // 2. Extract commands from schema
    let commands = extract_graphql_commands(&schema);

    // 3. Apply filters (include/exclude; methods not applicable for GraphQL)
    let commands = filter_commands(commands, &opts.include, &opts.exclude, &[]);

    // 4. Handle --list (grouped by query/mutation)
    if opts.list {
        return display_command_list(&commands, &opts.output_opts);
    }

    // 5. Handle --search
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

    // 6. Parse dynamic args
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

    // 7. Coerce args to typed JSON values
    let mut arguments: HashMap<String, Value> = HashMap::new();

    // If --stdin was passed, read JSON from stdin and merge as body
    if parsed.stdin {
        let stdin_value = crate::cli::stdin::read_stdin_json()?;
        if let serde_json::Value::Object(map) = stdin_value {
            for (k, v) in map {
                arguments.insert(k, v);
            }
        } else {
            return Err(AppError::Cli(
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

    // 8. Build headers slice for executor
    let headers_vec: Vec<(String, String)> = opts
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // 9. Execute GraphQL operation
    execute_graphql(
        cmd,
        &arguments,
        &opts.url,
        &schema,
        &headers_vec,
        opts.fields.as_deref(),
        &opts.output_opts,
    )
    .await
}

/// Load GraphQL schema via introspection, using cache if available.
async fn load_schema_cached(opts: &GraphqlHandlerOptions) -> Result<IntrospectionSchema> {
    let cache_key = opts
        .cache_key
        .clone()
        .unwrap_or_else(|| file_cache::cache_key_for(&opts.url));
    let ttl = opts.cache_ttl;

    // Try loading cached schema JSON
    let cached = if !opts.refresh {
        file_cache::load_cached(&cache_key, ttl).await
    } else {
        None
    };

    match cached {
        Some(v) => {
            let schema: IntrospectionSchema = serde_json::from_value(v)?;
            Ok(schema)
        }
        None => {
            let headers_vec: Vec<(String, String)> = opts
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let schema = load_graphql_schema(&opts.url, &headers_vec).await?;
            // Cache the schema as JSON
            let schema_json = serde_json::to_value(&schema)?;
            let _ = file_cache::save_cache(&cache_key, &schema_json).await;
            Ok(schema)
        }
    }
}

/// Display commands grouped by query/mutation.
fn display_command_list(commands: &[CommandDef], output_opts: &OutputOptions) -> Result<()> {
    let mut groups: BTreeMap<String, Vec<&CommandDef>> = BTreeMap::new();
    for cmd in commands {
        let group_key = cmd
            .graphql_operation_type
            .as_deref()
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
                            })
                        })
                        .collect();

                    serde_json::json!({
                        "name": cmd.name,
                        "operation": cmd.graphql_operation_type,
                        "field": cmd.graphql_field_name,
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
