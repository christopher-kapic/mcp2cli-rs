use assert_cmd::Command;
use predicates::prelude::*;

fn cmd() -> Command {
    Command::cargo_bin("mcp2cli").unwrap()
}

#[test]
fn help_exits_zero_with_usage() {
    cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Universal CLI adapter"))
        .stdout(predicate::str::contains("--spec"))
        .stdout(predicate::str::contains("--mcp"))
        .stdout(predicate::str::contains("--graphql"));
}

#[test]
fn version_prints_version() {
    cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("mcp2cli"))
        .stdout(predicate::str::contains("0.1.0"));
}

#[test]
fn list_with_no_source_exits_error() {
    cmd().arg("--list").assert().failure().stderr(
        predicate::str::contains("source")
            .or(predicate::str::contains("--spec").or(predicate::str::contains("--mcp"))),
    );
}

#[test]
fn spec_with_invalid_url_exits_error() {
    cmd()
        .args(["--spec", "http://localhost:1/nonexistent.json", "--list"])
        .assert()
        .failure();
}

#[test]
fn bake_list_with_no_configs_succeeds() {
    // bake list should succeed even with no configs
    cmd().args(["bake", "list"]).assert().success();
}

#[test]
fn bake_help_exits_zero() {
    cmd()
        .args(["bake", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("baked configurations"));
}

#[test]
fn no_source_no_subcommand_exits_error() {
    // Running with no source flags and no subcommand should fail
    cmd()
        .assert()
        .failure()
        .stderr(predicate::str::contains("No source specified"));
}

#[test]
fn search_with_no_source_exits_error() {
    cmd().args(["--search", "foo"]).assert().failure();
}
