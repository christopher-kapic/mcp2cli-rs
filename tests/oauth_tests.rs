//! Integration tests for OAuth: FileTokenStorage round-trip, secret resolution,
//! and client credentials token request with mocked endpoint.

use mcp2cli::oauth::storage::{FileTokenStorage, OAuthClientInfo, OAuthTokens};
use std::time::{SystemTime, UNIX_EPOCH};

// ── FileTokenStorage: read/write round-trip ──────────────────────────────────

#[tokio::test]
async fn test_token_storage_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let storage = FileTokenStorage::with_dir(dir.path().to_path_buf(), "https://auth.example.com");

    let tokens = OAuthTokens {
        access_token: "access-tok-123".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: Some("refresh-tok-456".to_string()),
        expires_at: Some(9999999999),
        scope: Some("read write".to_string()),
    };

    storage.set_tokens(&tokens).await.unwrap();
    let loaded = storage.get_tokens().await.unwrap();
    assert_eq!(loaded.access_token, "access-tok-123");
    assert_eq!(loaded.token_type, "Bearer");
    assert_eq!(loaded.refresh_token.as_deref(), Some("refresh-tok-456"));
    assert_eq!(loaded.expires_at, Some(9999999999));
    assert_eq!(loaded.scope.as_deref(), Some("read write"));
}

#[tokio::test]
async fn test_token_storage_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let storage = FileTokenStorage::with_dir(dir.path().to_path_buf(), "https://auth.example.com");

    let tokens1 = OAuthTokens {
        access_token: "first".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: None,
        expires_at: None,
        scope: None,
    };
    storage.set_tokens(&tokens1).await.unwrap();

    let tokens2 = OAuthTokens {
        access_token: "second".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: Some("new-refresh".to_string()),
        expires_at: Some(1234567890),
        scope: Some("admin".to_string()),
    };
    storage.set_tokens(&tokens2).await.unwrap();

    let loaded = storage.get_tokens().await.unwrap();
    assert_eq!(loaded.access_token, "second");
    assert_eq!(loaded.refresh_token.as_deref(), Some("new-refresh"));
}

#[tokio::test]
async fn test_client_info_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let storage = FileTokenStorage::with_dir(dir.path().to_path_buf(), "https://auth.example.com");

    let info = OAuthClientInfo {
        client_id: "my-client-id".to_string(),
        client_secret: Some("my-client-secret".to_string()),
        token_endpoint: Some("https://auth.example.com/token".to_string()),
        authorization_endpoint: Some("https://auth.example.com/authorize".to_string()),
    };

    storage.set_client_info(&info).await.unwrap();
    let loaded = storage.get_client_info().await.unwrap();
    assert_eq!(loaded.client_id, "my-client-id");
    assert_eq!(loaded.client_secret.as_deref(), Some("my-client-secret"));
    assert_eq!(
        loaded.token_endpoint.as_deref(),
        Some("https://auth.example.com/token")
    );
    assert_eq!(
        loaded.authorization_endpoint.as_deref(),
        Some("https://auth.example.com/authorize")
    );
}

#[tokio::test]
async fn test_get_tokens_missing() {
    let dir = tempfile::tempdir().unwrap();
    let storage = FileTokenStorage::with_dir(dir.path().to_path_buf(), "https://nonexistent.com");
    assert!(storage.get_tokens().await.is_none());
}

#[tokio::test]
async fn test_get_client_info_missing() {
    let dir = tempfile::tempdir().unwrap();
    let storage = FileTokenStorage::with_dir(dir.path().to_path_buf(), "https://nonexistent.com");
    assert!(storage.get_client_info().await.is_none());
}

// ── Token storage isolation by server URL ────────────────────────────────────

#[tokio::test]
async fn test_storage_isolated_by_url() {
    let dir = tempfile::tempdir().unwrap();
    let storage_a =
        FileTokenStorage::with_dir(dir.path().to_path_buf(), "https://server-a.example.com");
    let storage_b =
        FileTokenStorage::with_dir(dir.path().to_path_buf(), "https://server-b.example.com");

    let tokens_a = OAuthTokens {
        access_token: "token-a".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: None,
        expires_at: None,
        scope: None,
    };
    storage_a.set_tokens(&tokens_a).await.unwrap();

    let tokens_b = OAuthTokens {
        access_token: "token-b".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: None,
        expires_at: None,
        scope: None,
    };
    storage_b.set_tokens(&tokens_b).await.unwrap();

    // Each storage returns its own token
    let loaded_a = storage_a.get_tokens().await.unwrap();
    assert_eq!(loaded_a.access_token, "token-a");

    let loaded_b = storage_b.get_tokens().await.unwrap();
    assert_eq!(loaded_b.access_token, "token-b");
}

// ── Token expiry checks ──────────────────────────────────────────────────────

#[test]
fn test_token_not_expired() {
    let future_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
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

#[test]
fn test_token_expired_within_buffer() {
    // Token expires in 20 seconds — within the 30-second buffer, so considered expired
    let almost_expired = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 20;
    let tokens = OAuthTokens {
        access_token: "tok".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: None,
        expires_at: Some(almost_expired),
        scope: None,
    };
    assert!(tokens.is_expired());
}

// ── Secret resolution in OAuth context ───────────────────────────────────────

#[test]
fn test_resolve_secret_literal() {
    let result = mcp2cli::core::helpers::resolve_secret("my-literal-token");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "my-literal-token");
}

#[test]
fn test_resolve_secret_env_var() {
    // Set env var for test
    std::env::set_var("TEST_OAUTH_SECRET_VAR", "secret-from-env");
    let result = mcp2cli::core::helpers::resolve_secret("env:TEST_OAUTH_SECRET_VAR");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "secret-from-env");
    std::env::remove_var("TEST_OAUTH_SECRET_VAR");
}

#[test]
fn test_resolve_secret_env_var_missing() {
    let result = mcp2cli::core::helpers::resolve_secret("env:NONEXISTENT_OAUTH_VAR_12345");
    assert!(result.is_err());
}

#[test]
fn test_resolve_secret_file() {
    let dir = tempfile::tempdir().unwrap();
    let secret_file = dir.path().join("secret.txt");
    std::fs::write(&secret_file, "file-secret-value\n").unwrap();

    let result = mcp2cli::core::helpers::resolve_secret(&format!("file:{}", secret_file.display()));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "file-secret-value"); // trimmed
}

// ── Client credentials token request with mock ───────────────────────────────

#[tokio::test]
async fn test_client_credentials_token_request() {
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    // Mock token endpoint
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=client_credentials"))
        .and(body_string_contains("client_id=test-client"))
        .and(body_string_contains("client_secret=test-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "mock-access-token",
            "token_type": "Bearer",
            "expires_in": 3600,
            "scope": "read"
        })))
        .mount(&mock_server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let storage = FileTokenStorage::with_dir(dir.path().to_path_buf(), &mock_server.uri());

    let flow = mcp2cli::oauth::client_creds::ClientCredentialsFlow::new(
        storage,
        "test-client".to_string(),
        "test-secret".to_string(),
        format!("{}/token", mock_server.uri()),
        Some("read".to_string()),
    );

    let token = flow.get_access_token().await.unwrap();
    assert_eq!(token, "mock-access-token");
}

#[tokio::test]
async fn test_client_credentials_cached_token() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "cached-token",
            "token_type": "Bearer",
            "expires_in": 3600
        })))
        .expect(1) // Should only be called once — second call uses cache
        .mount(&mock_server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let storage = FileTokenStorage::with_dir(dir.path().to_path_buf(), &mock_server.uri());

    let flow = mcp2cli::oauth::client_creds::ClientCredentialsFlow::new(
        storage,
        "test-client".to_string(),
        "test-secret".to_string(),
        format!("{}/token", mock_server.uri()),
        None,
    );

    // First call: hits the server
    let token1 = flow.get_access_token().await.unwrap();
    assert_eq!(token1, "cached-token");

    // Second call: should use cached token (mock expects only 1 request)
    let token2 = flow.get_access_token().await.unwrap();
    assert_eq!(token2, "cached-token");
}

#[tokio::test]
async fn test_client_credentials_auth_header() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "my-token",
            "token_type": "Bearer",
            "expires_in": 3600
        })))
        .mount(&mock_server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let storage = FileTokenStorage::with_dir(dir.path().to_path_buf(), &mock_server.uri());

    let flow = mcp2cli::oauth::client_creds::ClientCredentialsFlow::new(
        storage,
        "client-id".to_string(),
        "client-secret".to_string(),
        format!("{}/token", mock_server.uri()),
        None,
    );

    let header = flow.get_auth_header().await.unwrap();
    assert_eq!(header, "Bearer my-token");
}

#[tokio::test]
async fn test_client_credentials_server_error() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .mount(&mock_server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let storage = FileTokenStorage::with_dir(dir.path().to_path_buf(), &mock_server.uri());

    let flow = mcp2cli::oauth::client_creds::ClientCredentialsFlow::new(
        storage,
        "bad-client".to_string(),
        "bad-secret".to_string(),
        format!("{}/token", mock_server.uri()),
        None,
    );

    let result = flow.get_access_token().await;
    assert!(result.is_err());
}
