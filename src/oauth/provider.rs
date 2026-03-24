use crate::error::Result;
use crate::oauth::client_creds::ClientCredentialsFlow;
use crate::oauth::pkce::PkceFlow;
use crate::oauth::storage::FileTokenStorage;

/// Trait for OAuth providers that can supply an Authorization header.
#[async_trait::async_trait]
pub trait OAuthProvider: Send + Sync {
    /// Get an Authorization header value (e.g., "Bearer <token>").
    /// Returns None if no token is available (shouldn't normally happen).
    async fn get_auth_header(&self) -> Result<String>;
}

/// PKCE-based OAuth provider.
struct PkceProvider {
    flow: PkceFlow,
}

#[async_trait::async_trait]
impl OAuthProvider for PkceProvider {
    async fn get_auth_header(&self) -> Result<String> {
        self.flow.get_auth_header().await
    }
}

/// Client credentials OAuth provider.
struct ClientCredentialsProvider {
    flow: ClientCredentialsFlow,
}

#[async_trait::async_trait]
impl OAuthProvider for ClientCredentialsProvider {
    async fn get_auth_header(&self) -> Result<String> {
        self.flow.get_auth_header().await
    }
}

/// Build an OAuth provider based on the available credentials.
///
/// - If both `client_id` and `client_secret` are provided → client credentials flow
/// - If only `client_id` is provided (no `client_secret`) → PKCE flow
///
/// `oauth_server` is the OAuth server URL used for token storage hashing.
/// `token_endpoint` and `authorization_endpoint` are the OAuth endpoints.
pub fn build_oauth_provider(
    oauth_server: &str,
    client_id: &str,
    client_secret: Option<&str>,
    token_endpoint: &str,
    authorization_endpoint: Option<&str>,
    scope: Option<String>,
) -> Box<dyn OAuthProvider> {
    let storage = FileTokenStorage::new(oauth_server);

    if let Some(secret) = client_secret {
        // Client credentials flow
        let flow = ClientCredentialsFlow::new(
            storage,
            client_id.to_string(),
            secret.to_string(),
            token_endpoint.to_string(),
            scope,
        );
        Box::new(ClientCredentialsProvider { flow })
    } else {
        // PKCE flow (no client_secret)
        let auth_endpoint = authorization_endpoint.unwrap_or(token_endpoint).to_string();
        let flow = PkceFlow::new(
            storage,
            client_id.to_string(),
            auth_endpoint,
            token_endpoint.to_string(),
            scope,
        );
        Box::new(PkceProvider { flow })
    }
}
