# mcp2cli

Universal CLI adapter that turns [MCP](https://modelcontextprotocol.io/) servers, OpenAPI specs, and GraphQL APIs into interactive command-line tools.

> **Inspired by [mcp2cli (Python)](https://github.com/knowsuchagency/mcp2cli)** — this is a ground-up Rust rewrite with expanded protocol support.

## Install

### Precompiled binary (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/christopher-kapic/mcp2cli-rs/master/scripts/install.sh | bash
```

This installs the latest release to `/usr/local/bin`. Run it again at any time to update.

To install a specific version or to a custom directory:

```bash
# Specific version
curl -fsSL https://raw.githubusercontent.com/christopher-kapic/mcp2cli-rs/master/scripts/install.sh | bash -s v0.2.0

# Custom directory
curl -fsSL https://raw.githubusercontent.com/christopher-kapic/mcp2cli-rs/master/scripts/install.sh | INSTALL_DIR=~/.local/bin bash
```

### From source

```bash
cargo install --git https://github.com/christopher-kapic/mcp2cli-rs.git
```

## Quick start

### MCP

```bash
# HTTP / SSE server
mcp2cli --mcp http://localhost:3000 --list
mcp2cli --mcp http://localhost:3000 get-user --id 42

# Stdio server
mcp2cli --mcp-stdio "npx -y @modelcontextprotocol/server-filesystem /" --list
```

### OpenAPI

```bash
mcp2cli --spec https://petstore3.swagger.io/api/v3/openapi.json --list
mcp2cli --spec ./openapi.yaml findPetsByStatus --status available --pretty
```

### GraphQL

```bash
mcp2cli --graphql https://countries.trevorblades.com --list
mcp2cli --graphql https://countries.trevorblades.com country --code US --pretty
```

## Features

- **Three protocols** — MCP (HTTP, SSE, stdio), OpenAPI, and GraphQL through one binary
- **Authentication** — static headers (`--auth-header`) or OAuth 2.0 (PKCE and client-credentials flows)
- **Sessions** — persistent background daemons for long-lived MCP connections
- **Bake** — save and recall CLI configurations as named profiles, installable as shell commands
- **Output** — pretty-print, raw, `--jq` filters, `--toon` formatting, `--head` to limit arrays
- **Caching** — file-based response cache with configurable TTL
- **Filtering** — `--search` globally; `--include` / `--exclude` glob patterns and `--methods` in bake configs

## Authentication

```bash
# Static header
mcp2cli --spec api.yaml --auth-header "Authorization:Bearer $TOKEN" list-items

# OAuth 2.0 PKCE (opens browser)
mcp2cli --spec api.yaml --oauth https://auth.example.com --oauth-client-id my-app list-items

# OAuth 2.0 client credentials
mcp2cli --spec api.yaml --oauth https://auth.example.com \
  --oauth-client-id my-app --oauth-client-secret $SECRET list-items
```

## Sessions

Keep an MCP connection alive across invocations:

```bash
mcp2cli --mcp http://localhost:3000 --session-start my-session
mcp2cli --session my-session get-user --id 42
mcp2cli --session my-session --session-stop my-session
mcp2cli --session-list
```

## Bake (saved configurations)

```bash
# Save a configuration
mcp2cli bake create petstore --spec https://petstore3.swagger.io/api/v3/openapi.json --pretty

# Use it
mcp2cli @petstore --list
mcp2cli @petstore findPetsByStatus --status available

# Install as a standalone shell command
mcp2cli bake install petstore
petstore findPetsByStatus --status available
```

## Output formatting

```bash
mcp2cli --spec api.yaml list-items --pretty          # Pretty-print JSON
mcp2cli --spec api.yaml list-items --jq '.[0].name'  # jq filter
mcp2cli --spec api.yaml list-items --head 5           # First 5 items
mcp2cli --spec api.yaml list-items --raw              # Raw text
```

## Requirements

- Rust 1.80+

## License

MIT
