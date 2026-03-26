use std::io::Write;
use std::process::{Command, Stdio};

/// Pipe JSON string through toon formatter and return the result.
/// Tries `toon` first, then falls back to `npx @toon-format/cli`.
/// If both fail, warns to stderr and returns the original JSON (matching Python behavior).
pub fn run_toon(json: &str) -> String {
    // Try direct toon binary first
    if let Ok(output) = run_pipe("toon", &[], json) {
        return output;
    }

    // Fallback to npx
    if let Ok(output) = run_pipe("npx", &["@toon-format/cli"], json) {
        return output;
    }

    // Graceful degradation: warn and return original JSON
    eprintln!("Warning: toon formatter not available, outputting raw JSON");
    json.to_string()
}

fn run_pipe(
    program: &str,
    args: &[&str],
    input: &str,
) -> std::result::Result<String, std::io::Error> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }

    let output = child.wait_with_output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "toon process failed",
        ))
    }
}
