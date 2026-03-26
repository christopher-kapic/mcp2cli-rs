use crate::bake::config::{load_baked_from, validate_name};
use crate::error::{AppError, Result};
use std::path::{Path, PathBuf};

/// Install a baked config as a shell wrapper script.
/// Generates: #!/bin/sh\nexec mcp2cli @NAME "$@"
pub async fn bake_install(config_dir: &Path, name: &str, install_dir: Option<&str>) -> Result<()> {
    validate_name(name)?;

    // Verify the config exists
    let _config = load_baked_from(config_dir, name)
        .await?
        .ok_or_else(|| AppError::Cli(format!("Baked config '{name}' not found")))?;

    let dir = match install_dir {
        Some(d) => PathBuf::from(d),
        None => default_install_dir()?,
    };

    tokio::fs::create_dir_all(&dir).await?;

    let script_path = dir.join(name);
    // Resolve mcp2cli binary path (matching Python's shutil.which behavior)
    let binary = resolve_mcp2cli_path();
    let script_content = format!("#!/bin/sh\nexec {binary} @{name} \"$@\"\n");

    tokio::fs::write(&script_path, &script_content).await?;

    // Set executable permission (chmod +x)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        tokio::fs::set_permissions(&script_path, perms).await?;
    }

    eprintln!("Installed wrapper script: {}", script_path.display());
    eprintln!("Make sure {} is in your PATH", dir.display());
    Ok(())
}

/// Default install directory: ~/.local/bin/
fn default_install_dir() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".local/bin"))
        .ok_or_else(|| AppError::Cli("Could not determine home directory".into()))
}

/// Resolve the full path to the mcp2cli binary using PATH lookup.
/// Falls back to "mcp2cli" if not found in PATH.
fn resolve_mcp2cli_path() -> String {
    which::which("mcp2cli")
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "mcp2cli".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bake::config::save_baked_all_to;
    use crate::core::types::BakeConfig;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_bake_install_generates_script() {
        let config_dir = tempfile::tempdir().unwrap();
        let install_dir = tempfile::tempdir().unwrap();

        // Save a config first
        let mut configs = HashMap::new();
        configs.insert(
            "my-api".to_string(),
            BakeConfig {
                source_type: "mcp".to_string(),
                source: "https://example.com".to_string(),
                ..Default::default()
            },
        );
        save_baked_all_to(config_dir.path(), &configs)
            .await
            .unwrap();

        // Install
        bake_install(
            config_dir.path(),
            "my-api",
            Some(install_dir.path().to_str().unwrap()),
        )
        .await
        .unwrap();

        // Verify script content
        let script_path = install_dir.path().join("my-api");
        let content = tokio::fs::read_to_string(&script_path).await.unwrap();
        assert_eq!(content, "#!/bin/sh\nexec mcp2cli @my-api \"$@\"\n");

        // Verify executable permission on unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = tokio::fs::metadata(&script_path).await.unwrap();
            let mode = meta.permissions().mode();
            assert_eq!(mode & 0o755, 0o755);
        }
    }

    #[tokio::test]
    async fn test_bake_install_missing_config() {
        let config_dir = tempfile::tempdir().unwrap();
        let install_dir = tempfile::tempdir().unwrap();

        let result = bake_install(
            config_dir.path(),
            "nonexistent",
            Some(install_dir.path().to_str().unwrap()),
        )
        .await;
        assert!(result.is_err());
    }
}
