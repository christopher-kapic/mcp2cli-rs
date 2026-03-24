# Contributing to mcp2cli

Thanks for your interest in contributing! Here's how to get started.

## Development setup

1. Install [Rust](https://www.rust-lang.org/tools/install) 1.80+
2. Clone the repo and build:

```bash
git clone https://github.com/christopher-kapic/mcp2cli-rs.git
cd mcp2cli-rs
cargo build
```

3. Run the test suite:

```bash
cargo test
```

## Making changes

1. Fork the repo and create a branch from `master`.
2. Make your changes.
3. Ensure your code passes all checks:

```bash
cargo fmt --check   # Formatting
cargo clippy         # Lints
cargo test           # Tests
```

4. Open a pull request against `master`.

## CI

Pull requests run formatting, clippy, and the full test suite automatically. All checks must pass before merging.

## Code style

- Run `cargo fmt` before committing.
- Fix any `cargo clippy` warnings.
- Add tests for new functionality.
- Keep changes focused — one feature or fix per PR.

## Releases

Releases are automated. When the `version` in `Cargo.toml` is updated on `master`, a GitHub release is created and precompiled binaries are built for macOS and Linux.

## Reporting issues

Open an issue on GitHub with steps to reproduce the problem and any relevant error output.
