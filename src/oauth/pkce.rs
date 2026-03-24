use crate::error::{AppError, Result};
use crate::oauth::storage::{FileTokenStorage, OAuthClientInfo, OAuthTokens};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

/// PKCE OAuth flow (Authorization Code + PKCE).
///
/// Used when only client_id is provided (no client_secret).
/// Launches a local HTTP server for the OAuth callback.
pub struct PkceFlow {
    storage: FileTokenStorage,
    client_id: String,
    authorization_endpoint: String,
    token_endpoint: String,
    scope: Option<String>,
}

impl PkceFlow {
    pub fn new(
        storage: FileTokenStorage,
        client_id: String,
        authorization_endpoint: String,
        token_endpoint: String,
        scope: Option<String>,
    ) -> Self {
        Self {
            storage,
            client_id,
            authorization_endpoint,
            token_endpoint,
            scope,
        }
    }

    /// Get a valid access token, performing the PKCE flow if needed.
    pub async fn get_access_token(&self) -> Result<String> {
        // Check cached tokens first
        if let Some(tokens) = self.storage.get_tokens().await {
            if !tokens.is_expired() {
                return Ok(tokens.access_token);
            }
            // Try refresh if we have a refresh token
            if let Some(ref refresh_token) = tokens.refresh_token {
                match self.refresh_token(refresh_token).await {
                    Ok(new_tokens) => {
                        self.storage.set_tokens(&new_tokens).await?;
                        return Ok(new_tokens.access_token);
                    }
                    Err(_) => {
                        // Refresh failed, fall through to new auth
                    }
                }
            }
        }

        // Perform full PKCE authorization flow
        let tokens = self.authorize().await?;
        self.storage.set_tokens(&tokens).await?;

        // Cache client info
        let client_info = OAuthClientInfo {
            client_id: self.client_id.clone(),
            client_secret: None,
            token_endpoint: Some(self.token_endpoint.clone()),
            authorization_endpoint: Some(self.authorization_endpoint.clone()),
        };
        self.storage.set_client_info(&client_info).await?;

        Ok(tokens.access_token)
    }

    /// Get an Authorization header value.
    pub async fn get_auth_header(&self) -> Result<String> {
        let token = self.get_access_token().await?;
        Ok(format!("Bearer {token}"))
    }

    /// Perform the full PKCE authorization flow:
    /// 1. Generate code_verifier and code_challenge
    /// 2. Start local callback server
    /// 3. Open browser / print auth URL
    /// 4. Wait for callback with authorization code
    /// 5. Exchange code for tokens
    async fn authorize(&self) -> Result<OAuthTokens> {
        let (code_verifier, code_challenge) = generate_pkce_pair();

        // Start local callback server on random port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(AppError::Io)?;
        let local_addr = listener.local_addr().map_err(AppError::Io)?;
        let redirect_uri = format!("http://127.0.0.1:{}/callback", local_addr.port());

        // Build authorization URL
        let auth_url = build_authorization_url(
            &self.authorization_endpoint,
            &self.client_id,
            &redirect_uri,
            &code_challenge,
            self.scope.as_deref(),
        );

        eprintln!("Open this URL to authorize:");
        eprintln!("{auth_url}");

        // Wait for the callback
        let (tx, rx) = oneshot::channel::<String>();
        let tx = Arc::new(tokio::sync::Mutex::new(Some(tx)));

        let app = build_callback_router(tx);
        let server = axum::serve(listener, app);

        // Run server until we get the code
        tokio::select! {
            result = server => {
                result.map_err(AppError::Io)?;
                Err(AppError::Execution("Callback server shut down unexpectedly".into()))
            }
            code = rx => {
                let code = code.map_err(|_| AppError::Execution("Failed to receive authorization code".into()))?;
                // Exchange code for tokens
                self.exchange_code(&code, &code_verifier, &redirect_uri).await
            }
        }
    }

    /// Exchange authorization code for tokens.
    async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<OAuthTokens> {
        let mut params = HashMap::new();
        params.insert("grant_type", "authorization_code");
        params.insert("code", code);
        params.insert("redirect_uri", redirect_uri);
        params.insert("client_id", &self.client_id);
        params.insert("code_verifier", code_verifier);

        let client = reqwest::Client::new();
        let resp = client
            .post(&self.token_endpoint)
            .form(&params)
            .send()
            .await
            .map_err(AppError::Network)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Execution(format!(
                "Token exchange failed ({status}): {body}"
            )));
        }

        let token_resp: TokenResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Protocol(format!("Failed to parse token response: {e}")))?;

        Ok(token_response_to_tokens(token_resp))
    }

    /// Refresh an existing token.
    async fn refresh_token(&self, refresh_token: &str) -> Result<OAuthTokens> {
        let mut params = HashMap::new();
        params.insert("grant_type", "refresh_token");
        params.insert("refresh_token", refresh_token);
        params.insert("client_id", &self.client_id);

        let client = reqwest::Client::new();
        let resp = client
            .post(&self.token_endpoint)
            .form(&params)
            .send()
            .await
            .map_err(AppError::Network)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Execution(format!(
                "Token refresh failed ({status}): {body}"
            )));
        }

        let token_resp: TokenResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Protocol(format!("Failed to parse token response: {e}")))?;

        Ok(token_response_to_tokens(token_resp))
    }
}

/// Generate a PKCE code_verifier and code_challenge pair.
///
/// code_verifier: 32 random bytes, base64url-encoded (no padding)
/// code_challenge: SHA-256 of code_verifier, base64url-encoded (no padding)
pub fn generate_pkce_pair() -> (String, String) {
    use rand::RngCore;
    let mut verifier_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut verifier_bytes);
    let code_verifier = base64url_encode(&verifier_bytes);

    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let challenge_bytes = hasher.finalize();
    let code_challenge = base64url_encode(&challenge_bytes);

    (code_verifier, code_challenge)
}

/// Base64url encoding without padding (per RFC 7636).
fn base64url_encode(data: &[u8]) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    URL_SAFE_NO_PAD.encode(data)
}

/// Build the authorization URL with PKCE parameters.
fn build_authorization_url(
    authorization_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
    scope: Option<&str>,
) -> String {
    let mut url = format!(
        "{authorization_endpoint}?response_type=code&client_id={}&redirect_uri={}&code_challenge={code_challenge}&code_challenge_method=S256",
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
    );
    if let Some(scope) = scope {
        url.push_str(&format!("&scope={}", urlencoding::encode(scope)));
    }
    url
}

/// Build the axum router for the OAuth callback server.
fn build_callback_router(
    tx: Arc<tokio::sync::Mutex<Option<oneshot::Sender<String>>>>,
) -> axum::Router {
    use axum::extract::Query;
    use axum::response::Html;
    use axum::routing::get;

    axum::Router::new().route(
        "/callback",
        get(move |Query(params): Query<HashMap<String, String>>| {
            let tx = tx.clone();
            async move {
                if let Some(code) = params.get("code") {
                    if let Some(sender) = tx.lock().await.take() {
                        let _ = sender.send(code.clone());
                    }
                    Html("<html><body><h1>Authorization successful!</h1><p>You can close this window.</p></body></html>".to_string())
                } else {
                    let error = params
                        .get("error_description")
                        .or_else(|| params.get("error"))
                        .cloned()
                        .unwrap_or_else(|| "Unknown error".to_string());
                    Html(format!("<html><body><h1>Authorization failed</h1><p>{error}</p></body></html>"))
                }
            }
        }),
    )
}

/// Standard OAuth2 token response.
#[derive(Debug, serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
}

/// Convert a token response into our storage format.
fn token_response_to_tokens(resp: TokenResponse) -> OAuthTokens {
    let expires_at = resp.expires_in.map(|secs| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + secs
    });

    OAuthTokens {
        access_token: resp.access_token,
        token_type: resp.token_type,
        refresh_token: resp.refresh_token,
        expires_at,
        scope: resp.scope,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_pkce_pair() {
        let (verifier, challenge) = generate_pkce_pair();
        // Verifier should be base64url-encoded 32 bytes = 43 chars
        assert_eq!(verifier.len(), 43);
        // Challenge should be base64url-encoded SHA-256 = 43 chars
        assert_eq!(challenge.len(), 43);
        // They should be different
        assert_ne!(verifier, challenge);
    }

    #[test]
    fn test_generate_pkce_pair_unique() {
        let (v1, _) = generate_pkce_pair();
        let (v2, _) = generate_pkce_pair();
        assert_ne!(v1, v2, "Each PKCE pair should be unique");
    }

    #[test]
    fn test_pkce_challenge_is_sha256_of_verifier() {
        let (verifier, challenge) = generate_pkce_pair();

        // Recompute the challenge from the verifier
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let expected = base64url_encode(&hasher.finalize());

        assert_eq!(challenge, expected);
    }

    #[test]
    fn test_build_authorization_url_basic() {
        let url = build_authorization_url(
            "https://auth.example.com/authorize",
            "my-client",
            "http://localhost:8080/callback",
            "test-challenge",
            None,
        );
        assert!(url.starts_with("https://auth.example.com/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=my-client"));
        assert!(url.contains("code_challenge=test-challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(!url.contains("scope="));
    }

    #[test]
    fn test_build_authorization_url_with_scope() {
        let url = build_authorization_url(
            "https://auth.example.com/authorize",
            "my-client",
            "http://localhost:8080/callback",
            "test-challenge",
            Some("read write"),
        );
        assert!(url.contains("scope=read%20write"));
    }
}
