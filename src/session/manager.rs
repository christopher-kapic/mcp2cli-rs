use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Metadata about a running session, written as JSON next to the socket.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub name: String,
    pub pid: u32,
    pub source: String,
    pub transport: String,
    pub created_at: u64,
}

/// Return the directory where session sockets and metadata are stored.
pub fn sessions_dir() -> PathBuf {
    let base = if let Ok(dir) = std::env::var("MCP2CLI_CACHE_DIR") {
        PathBuf::from(dir)
    } else {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("mcp2cli")
    };
    base.join("sessions")
}

/// Return the Unix socket path for a named session.
pub fn session_socket_path(name: &str) -> PathBuf {
    sessions_dir().join(format!("{name}.sock"))
}

/// Return the metadata file path for a named session.
fn session_metadata_path(name: &str) -> PathBuf {
    sessions_dir().join(format!("{name}.json"))
}

/// Start a new session daemon as a background subprocess.
///
/// Spawns the current binary with an internal `--session-daemon` flag,
/// writes metadata JSON with the PID.
pub async fn session_start(
    name: &str,
    source: &str,
    transport: &str,
    headers: &std::collections::HashMap<String, String>,
    env_vars: &[(String, String)],
) -> Result<()> {
    let dir = sessions_dir();
    tokio::fs::create_dir_all(&dir).await?;

    // Check if a session with this name is already running
    if is_session_alive(name).await {
        return Err(AppError::Cli(format!(
            "Session '{name}' is already running"
        )));
    }

    let headers_json = serde_json::to_string(headers)?;
    let env_json = serde_json::to_string(env_vars)?;

    // Get the path to the current binary
    let exe = std::env::current_exe().map_err(AppError::Io)?;

    // Spawn the daemon subprocess
    let child = tokio::process::Command::new(exe)
        .arg("--session-daemon")
        .arg(name)
        .arg(source)
        .arg(transport)
        .arg(&headers_json)
        .arg(&env_json)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    let pid = child
        .id()
        .ok_or_else(|| AppError::Execution("Failed to get daemon PID".into()))?;

    // Write metadata
    let metadata = SessionMetadata {
        name: name.to_string(),
        pid,
        source: source.to_string(),
        transport: transport.to_string(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };

    let metadata_path = session_metadata_path(name);
    let json = serde_json::to_string_pretty(&metadata)?;
    tokio::fs::write(&metadata_path, json).await?;

    // Wait briefly for the socket to appear
    let socket_path = session_socket_path(name);
    for _ in 0..20 {
        if socket_path.exists() {
            eprintln!("Session '{name}' started (PID {pid})");
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    eprintln!("Session '{name}' started (PID {pid}), but socket not yet ready");
    Ok(())
}

/// Stop a named session by sending SIGTERM and cleaning up.
pub async fn session_stop(name: &str) -> Result<()> {
    let metadata_path = session_metadata_path(name);
    let socket_path = session_socket_path(name);

    // Load metadata to get PID
    match tokio::fs::read_to_string(&metadata_path).await {
        Ok(json) => {
            if let Ok(metadata) = serde_json::from_str::<SessionMetadata>(&json) {
                // Send SIGTERM
                let pid = metadata.pid as i32;
                libc_kill(pid);
                eprintln!("Session '{name}' stopped (PID {})", metadata.pid);
            }
        }
        Err(_) => {
            eprintln!("Session '{name}' metadata not found");
        }
    }

    // Cleanup files
    let _ = tokio::fs::remove_file(&socket_path).await;
    let _ = tokio::fs::remove_file(&metadata_path).await;

    Ok(())
}

/// Send SIGTERM to a process by PID.
/// Uses raw syscall to avoid adding libc dependency.
fn libc_kill(pid: i32) {
    use std::process::Command;
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
}

/// List all sessions with their alive/dead status.
pub async fn session_list() -> Result<Vec<SessionListEntry>> {
    let dir = sessions_dir();

    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&dir).await?;

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let json = match tokio::fs::read_to_string(&path).await {
            Ok(j) => j,
            Err(_) => continue,
        };

        let metadata: SessionMetadata = match serde_json::from_str(&json) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let alive = is_pid_alive(metadata.pid);
        entries.push(SessionListEntry {
            name: metadata.name,
            pid: metadata.pid,
            source: metadata.source,
            transport: metadata.transport,
            alive,
        });
    }

    Ok(entries)
}

/// Information about a session for display.
#[derive(Debug)]
pub struct SessionListEntry {
    pub name: String,
    pub pid: u32,
    pub source: String,
    pub transport: String,
    pub alive: bool,
}

/// Check if a session with the given name is alive.
async fn is_session_alive(name: &str) -> bool {
    let metadata_path = session_metadata_path(name);
    if let Ok(json) = tokio::fs::read_to_string(&metadata_path).await {
        if let Ok(metadata) = serde_json::from_str::<SessionMetadata>(&json) {
            return is_pid_alive(metadata.pid);
        }
    }
    false
}

/// Check if a process is alive by sending signal 0.
fn is_pid_alive(pid: u32) -> bool {
    // Use kill -0 to check if process exists
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
