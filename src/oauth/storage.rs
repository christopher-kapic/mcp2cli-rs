use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// OAuth token data stored on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub token_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Expiry as Unix timestamp (seconds since epoch).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// OAuth client registration info stored on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientInfo {
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_endpoint: Option<String>,
}

/// File-based token storage for OAuth credentials.
///
/// Tokens are stored under `<base_dir>/oauth/<hash>/` where hash is
/// derived from the OAuth server URL.
pub struct FileTokenStorage {
    dir: PathBuf,
}

impl FileTokenStorage {
    /// Create storage for a given OAuth server URL, using the default cache dir.
    pub fn new(server_url: &str) -> Self {
        let base = oauth_cache_dir();
        Self::with_dir(base, server_url)
    }

    /// Create storage with an explicit base directory (for testing).
    pub fn with_dir(base_dir: PathBuf, server_url: &str) -> Self {
        let hash = crate::cache::file_cache::cache_key_for(server_url);
        Self {
            dir: base_dir.join(hash),
        }
    }

    fn tokens_path(&self) -> PathBuf {
        self.dir.join("tokens.json")
    }

    fn client_info_path(&self) -> PathBuf {
        self.dir.join("client_info.json")
    }

    /// Read stored tokens, if they exist.
    pub async fn get_tokens(&self) -> Option<OAuthTokens> {
        let data = tokio::fs::read_to_string(self.tokens_path()).await.ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Write tokens to disk.
    pub async fn set_tokens(&self, tokens: &OAuthTokens) -> Result<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        let json = serde_json::to_string_pretty(tokens)?;
        tokio::fs::write(self.tokens_path(), json).await?;
        Ok(())
    }

    /// Read stored client info, if it exists.
    pub async fn get_client_info(&self) -> Option<OAuthClientInfo> {
        let data = tokio::fs::read_to_string(self.client_info_path())
            .await
            .ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Write client info to disk.
    pub async fn set_client_info(&self, info: &OAuthClientInfo) -> Result<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        let json = serde_json::to_string_pretty(info)?;
        tokio::fs::write(self.client_info_path(), json).await?;
        Ok(())
    }
}

impl OAuthTokens {
    /// Check if the token has expired (with a 30-second buffer).
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                now + 30 >= exp
            }
            None => false, // No expiry means we assume it's valid
        }
    }
}

/// Return the OAuth cache directory, respecting MCP2CLI_CACHE_DIR env var.
fn oauth_cache_dir() -> PathBuf {
    let base = if let Ok(dir) = std::env::var("MCP2CLI_CACHE_DIR") {
        PathBuf::from(dir)
    } else {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("mcp2cli")
    };
    base.join("oauth")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_token_storage_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            FileTokenStorage::with_dir(dir.path().to_path_buf(), "https://auth.example.com");

        let tokens = OAuthTokens {
            access_token: "abc123".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: Some("refresh456".to_string()),
            expires_at: Some(9999999999),
            scope: Some("read write".to_string()),
        };

        storage.set_tokens(&tokens).await.unwrap();
        let loaded = storage.get_tokens().await.unwrap();
        assert_eq!(loaded.access_token, "abc123");
        assert_eq!(loaded.refresh_token.as_deref(), Some("refresh456"));
        assert_eq!(loaded.scope.as_deref(), Some("read write"));
    }

    #[tokio::test]
    async fn test_client_info_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            FileTokenStorage::with_dir(dir.path().to_path_buf(), "https://auth.example.com");

        let info = OAuthClientInfo {
            client_id: "my-client".to_string(),
            client_secret: Some("my-secret".to_string()),
            token_endpoint: Some("https://auth.example.com/token".to_string()),
            authorization_endpoint: None,
        };

        storage.set_client_info(&info).await.unwrap();
        let loaded = storage.get_client_info().await.unwrap();
        assert_eq!(loaded.client_id, "my-client");
        assert_eq!(loaded.client_secret.as_deref(), Some("my-secret"));
    }

    #[tokio::test]
    async fn test_get_tokens_missing() {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            FileTokenStorage::with_dir(dir.path().to_path_buf(), "https://nonexistent.com");
        assert!(storage.get_tokens().await.is_none());
    }

    #[test]
    fn test_token_not_expired() {
        let future_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;
        let tokens = OAuthTokens {
            access_token: "tok".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_at: Some(future_ts),
            scope: None,
        };
        assert!(!tokens.is_expired());
    }

    #[test]
    fn test_token_expired() {
        let tokens = OAuthTokens {
            access_token: "tok".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_at: Some(1000), // Long past
            scope: None,
        };
        assert!(tokens.is_expired());
    }

    #[test]
    fn test_token_no_expiry_not_expired() {
        let tokens = OAuthTokens {
            access_token: "tok".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_at: None,
            scope: None,
        };
        assert!(!tokens.is_expired());
    }
}
