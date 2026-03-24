use crate::error::Result;
use hex::encode as hex_encode;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Compute a cache key from a source string: first 16 hex chars of SHA-256.
pub fn cache_key_for(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    let hash = hasher.finalize();
    hex_encode(hash)[..16].to_string()
}

/// Return the cache directory, respecting MCP2CLI_CACHE_DIR env var.
pub fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MCP2CLI_CACHE_DIR") {
        PathBuf::from(dir)
    } else {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("mcp2cli")
    }
}

/// Load a cached value if it exists and is within the TTL (in seconds).
pub async fn load_cached(key: &str, ttl_secs: u64) -> Option<Value> {
    load_cached_from(&cache_dir(), key, ttl_secs).await
}

/// Load a cached value from a specific directory.
pub async fn load_cached_from(dir: &Path, key: &str, ttl_secs: u64) -> Option<Value> {
    let path = dir.join(format!("{key}.json"));
    let metadata = tokio::fs::metadata(&path).await.ok()?;
    let modified = metadata.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age > Duration::from_secs(ttl_secs) {
        return None;
    }
    let data = tokio::fs::read_to_string(&path).await.ok()?;
    serde_json::from_str(&data).ok()
}

/// Save a value to the cache.
pub async fn save_cache(key: &str, data: &Value) -> Result<()> {
    save_cache_to(&cache_dir(), key, data).await
}

/// Save a value to a specific cache directory.
pub async fn save_cache_to(dir: &Path, key: &str, data: &Value) -> Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    let path = dir.join(format!("{key}.json"));
    let json = serde_json::to_string(data)?;
    tokio::fs::write(&path, json).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_deterministic() {
        let key1 = cache_key_for("https://example.com/api");
        let key2 = cache_key_for("https://example.com/api");
        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 16);
    }

    #[test]
    fn test_cache_key_different_inputs() {
        let key1 = cache_key_for("input-a");
        let key2 = cache_key_for("input-b");
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_is_hex() {
        let key = cache_key_for("test");
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn test_save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();

        let data = serde_json::json!({"tools": [{"name": "test"}]});
        save_cache_to(dir.path(), "test_key", &data).await.unwrap();
        let loaded = load_cached_from(dir.path(), "test_key", 60).await;
        assert_eq!(loaded, Some(data));
    }

    #[tokio::test]
    async fn test_load_expired_ttl() {
        let dir = tempfile::tempdir().unwrap();

        let data = serde_json::json!({"expired": true});
        save_cache_to(dir.path(), "expired_key", &data)
            .await
            .unwrap();
        // TTL of 0 seconds means it's immediately expired
        let loaded = load_cached_from(dir.path(), "expired_key", 0).await;
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn test_load_missing_key() {
        let dir = tempfile::tempdir().unwrap();

        let loaded = load_cached_from(dir.path(), "nonexistent", 60).await;
        assert!(loaded.is_none());
    }
}
