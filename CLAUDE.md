# mcp2cli — Universal CLI Adapter

A Rust CLI that bridges MCP (Model Context Protocol), OpenAPI, and GraphQL services into a unified command-line interface. Converts remote API specs into local CLI commands with automatic argument parsing, OAuth flows, and response formatting.

## Tech Stack

- **Language:** Rust (edition 2021, MSRV 1.80)
- **CLI:** clap v4 with derive macros
- **Async:** tokio (multi-threaded runtime)
- **HTTP:** reqwest with rustls-tls
- **Serialization:** serde + serde_json + serde_yaml
- **Error handling:** anyhow + thiserror
- **OAuth:** oauth2 crate + axum (local callback server)
- **Caching:** file-based with sha2 hashing

## Project Structure

```
src/
  main.rs              — Entry point
  lib.rs               — Module declarations
  cli/                 — Clap command/arg definitions
  core/                — Shared types, helpers, coercion, glob filters
  error.rs             — Error types
  mcp/                 — MCP (Model Context Protocol) transport + execution
  openapi/             — OpenAPI spec parsing, ref resolution, command generation, execution
  graphql/             — GraphQL introspection, selection sets, command generation, execution
  oauth/               — OAuth2 flows (auth code + PKCE, client credentials, token storage)
  bake/                — "Bake" feature: snapshot API specs into standalone CLI wrappers
  cache/               — File-based response/spec caching
  output/              — Response formatting and display
  session/             — Session/connection management
```

## Key Design Decisions

- **Three backends:** MCP (JSON-RPC over stdio/SSE), OpenAPI (REST), GraphQL — all unified under one CLI surface
- **Bake:** Generates standalone shell scripts from API specs so users don't need mcp2cli at runtime
- **OAuth with local server:** Spins up a temporary axum server for OAuth callback handling
- **File-based caching:** Specs and responses cached by SHA256 hash in XDG dirs
- **camelCase → kebab-case:** API parameter names auto-converted to CLI-friendly format via regex

## Build & Test

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Querying Dependencies with kctx

Coding agents have access to **kctx** — a dependency knowledge service. Use the `mcp__kctx__query_dependency` and `mcp__kctx__list_dependencies` MCP tools to ask usage questions about external libraries.
