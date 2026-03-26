use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "mcp2cli",
    version,
    about = "Universal CLI adapter for MCP, OpenAPI, and GraphQL services"
)]
pub struct Cli {
    // Source modes (mutually exclusive in practice)
    /// OpenAPI spec URL or file path
    #[arg(long = "spec")]
    pub spec: Option<String>,

    /// MCP server URL (HTTP/SSE)
    #[arg(long = "mcp")]
    pub mcp: Option<String>,

    /// MCP server command (stdio transport)
    #[arg(long = "mcp-stdio")]
    pub mcp_stdio: Option<String>,

    /// GraphQL endpoint URL
    #[arg(long = "graphql")]
    pub graphql: Option<String>,

    // Auth
    /// Auth header(s) in "Key:Value" format
    #[arg(long = "auth-header", num_args = 1)]
    pub auth_header: Vec<String>,

    /// Enable OAuth flow (uses MCP server URL for discovery)
    #[arg(long = "oauth")]
    pub oauth: bool,

    /// OAuth client ID
    #[arg(long = "oauth-client-id")]
    pub oauth_client_id: Option<String>,

    /// OAuth client secret
    #[arg(long = "oauth-client-secret")]
    pub oauth_client_secret: Option<String>,

    /// OAuth scope
    #[arg(long = "oauth-scope")]
    pub oauth_scope: Option<String>,

    // Output
    /// Pretty-print JSON output
    #[arg(long = "pretty")]
    pub pretty: bool,

    /// Raw output (no JSON formatting)
    #[arg(long = "raw")]
    pub raw: bool,

    /// Pipe output through toon formatter
    #[arg(long = "toon")]
    pub toon: bool,

    /// Pipe output through jq with given expression
    #[arg(long = "jq")]
    pub jq: Option<String>,

    /// Limit array output to N items
    #[arg(long = "head")]
    pub head: Option<usize>,

    // Cache
    /// Custom cache key
    #[arg(long = "cache-key")]
    pub cache_key: Option<String>,

    /// Cache TTL in seconds
    #[arg(long = "cache-ttl")]
    pub cache_ttl: Option<u64>,

    /// Force cache refresh
    #[arg(long = "refresh")]
    pub refresh: bool,

    // Discovery
    /// List available commands
    #[arg(long = "list")]
    pub list: bool,

    /// Search commands by name
    #[arg(long = "search")]
    pub search: Option<String>,

    // Transport
    /// MCP transport mode: auto, sse, streamable
    #[arg(long = "transport", default_value = "auto")]
    pub transport: String,

    // Filtering
    /// Include only commands matching glob pattern (comma-separated or repeated)
    #[arg(long = "include", num_args = 1, value_delimiter = ',')]
    pub include: Vec<String>,

    /// Exclude commands matching glob pattern (comma-separated or repeated)
    #[arg(long = "exclude", num_args = 1, value_delimiter = ',')]
    pub exclude: Vec<String>,

    /// Filter by HTTP methods (OpenAPI only)
    #[arg(long = "methods", num_args = 1)]
    pub methods: Vec<String>,

    // Env/base
    /// Environment variables in "KEY=VALUE" format
    #[arg(long = "env", num_args = 1)]
    pub env: Vec<String>,

    /// Base URL override for OpenAPI
    #[arg(long = "base-url")]
    pub base_url: Option<String>,

    // MCP resources
    /// List MCP resources
    #[arg(long = "list-resources")]
    pub list_resources: bool,

    /// List MCP resource templates
    #[arg(long = "list-resource-templates")]
    pub list_resource_templates: bool,

    /// Read an MCP resource by URI
    #[arg(long = "read-resource")]
    pub read_resource: Option<String>,

    // MCP prompts
    /// List MCP prompts
    #[arg(long = "list-prompts")]
    pub list_prompts: bool,

    /// Get an MCP prompt by name
    #[arg(long = "get-prompt")]
    pub get_prompt: Option<String>,

    /// Prompt arguments in "key:value" format
    #[arg(long = "prompt-arg", num_args = 1)]
    pub prompt_arg: Vec<String>,

    // Sessions
    /// Start a named session
    #[arg(long = "session-start")]
    pub session_start: Option<String>,

    /// Stop a named session
    #[arg(long = "session-stop")]
    pub session_stop: Option<String>,

    /// Use a named session
    #[arg(long = "session")]
    pub session: Option<String>,

    /// List active sessions
    #[arg(long = "session-list")]
    pub session_list: bool,

    // GraphQL
    /// Override GraphQL selection set fields
    #[arg(long = "fields")]
    pub fields: Option<String>,

    // Bake subcommand
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Remaining args (subcommand name + tool arguments)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub rest: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Manage baked configurations
    Bake {
        #[command(subcommand)]
        action: BakeAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum BakeAction {
    /// Create a new baked configuration
    Create {
        name: String,
        /// Overwrite existing config if it exists
        #[arg(long = "force", short = 'f')]
        force: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// List all baked configurations
    List,
    /// Show a baked configuration
    Show { name: String },
    /// Remove a baked configuration
    Remove { name: String },
    /// Update a baked configuration
    Update {
        name: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Install a baked configuration as a shell command
    Install {
        name: String,
        /// Install directory
        #[arg(long = "dir")]
        dir: Option<String>,
    },
}
