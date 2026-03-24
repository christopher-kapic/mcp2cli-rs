use crate::error::{AppError, Result};
use crate::oauth::storage::{FileTokenStorage, OAuthClientInfo, OAuthTokens};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Client credentials OAuth flow.
///
/// Exchanges client_id + client_secret for an access token at the token endpoint.
/// Caches tokens via FileTokenStorage and refreshes on expiry.
pub struct ClientCredentialsFlow {
    storage: FileTokenStorage,
    client_id: String,
    client_secret: String,
    token_endpoint: String,
    scope: Option<String>,
}

impl ClientCredentialsFlow {
    pub fn new(
        storage: FileTokenStorage,
        client_id: String,
        client_secret: String,
        token_endpoint: String,
        scope: Option<String>,
    ) -> Self {
        Self {
            storage,
            client_id,
            client_secret,
            token_endpoint,
            scope,
        }
    }

    /// Get a valid access token, fetching or refreshing as needed.
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
                        // Refresh failed, fall through to new token request
                    }
                }
            }
        }

        // Request new token
        let tokens = self.request_token().await?;
        self.storage.set_tokens(&tokens).await?;

        // Also cache client info for future reference
        let client_info = OAuthClientInfo {
            client_id: self.client_id.clone(),
            client_secret: Some(self.client_secret.clone()),
            token_endpoint: Some(self.token_endpoint.clone()),
            authorization_endpoint: None,
        };
        self.storage.set_client_info(&client_info).await?;

        Ok(tokens.access_token)
    }

    /// Get an Authorization header value (e.g., "Bearer <token>").
    pub async fn get_auth_header(&self) -> Result<String> {
        let token = self.get_access_token().await?;
        Ok(format!("Bearer {token}"))
    }

    /// Request a new token using client credentials grant.
    async fn request_token(&self) -> Result<OAuthTokens> {
        let mut params = HashMap::new();
        params.insert("grant_type", "client_credentials");
        params.insert("client_id", &self.client_id);
        params.insert("client_secret", &self.client_secret);

        let scope_str;
        if let Some(ref scope) = self.scope {
            scope_str = scope.clone();
            params.insert("scope", &scope_str);
        }

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
                "Token request failed ({status}): {body}"
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
        params.insert("client_secret", &self.client_secret);

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

/// Convert a token response into our storage format, computing expires_at from expires_in.
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
    fn test_token_response_to_tokens_with_expiry() {
        let resp = TokenResponse {
            access_token: "test-token".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: Some("refresh-tok".to_string()),
            expires_in: Some(3600),
            scope: Some("read".to_string()),
        };

        let tokens = token_response_to_tokens(resp);
        assert_eq!(tokens.access_token, "test-token");
        assert_eq!(tokens.token_type, "Bearer");
        assert_eq!(tokens.refresh_token.as_deref(), Some("refresh-tok"));
        assert!(tokens.expires_at.is_some());
        assert_eq!(tokens.scope.as_deref(), Some("read"));

        // expires_at should be roughly now + 3600
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let exp = tokens.expires_at.unwrap();
        assert!(exp >= now + 3599 && exp <= now + 3601);
    }

    #[test]
    fn test_token_response_to_tokens_no_expiry() {
        let resp = TokenResponse {
            access_token: "tok".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_in: None,
            scope: None,
        };

        let tokens = token_response_to_tokens(resp);
        assert!(tokens.expires_at.is_none());
        assert!(tokens.refresh_token.is_none());
    }
}
