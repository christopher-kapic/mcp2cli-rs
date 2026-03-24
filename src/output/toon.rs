use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::{AppError, Result};

/// Pipe JSON string through toon formatter and return the result.
/// Tries `toon` first, then falls back to `npx @toon-format/cli`.
pub fn run_toon(json: &str) -> Result<String> {
    // Try direct toon binary first
    if let Ok(output) = run_pipe("toon", &[], json) {
        return Ok(output);
    }

    // Fallback to npx
    run_pipe("npx", &["@toon-format/cli"], json)
}

fn run_pipe(program: &str, args: &[&str], input: &str) -> Result<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Execution(format!("Failed to spawn {program}: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(AppError::Execution(format!("{program} error: {stderr}")))
    }
}
