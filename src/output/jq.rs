use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::{AppError, Result};

/// Pipe JSON string through jq with the given expression and return the result.
pub fn run_jq(json: &str, expression: &str) -> Result<String> {
    let mut child = Command::new("jq")
        .arg(expression)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Execution(format!("Failed to spawn jq: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(json.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(AppError::Execution(format!("jq error: {stderr}")))
    }
}
