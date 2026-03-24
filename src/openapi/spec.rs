use crate::error::{AppError, Result};
use serde_json::Value;

/// Load an OpenAPI spec from a file path or URL.
pub async fn load_spec(source: &str, headers: &[(String, String)]) -> Result<Value> {
    if source.starts_with("http://") || source.starts_with("https://") {
        load_remote(source, headers).await
    } else {
        load_local(source)
    }
}

fn load_local(path: &str) -> Result<Value> {
    let content = std::fs::read_to_string(path)?;
    if path.ends_with(".yaml") || path.ends_with(".yml") {
        serde_yaml::from_str(&content).map_err(|e| AppError::Protocol(format!("invalid YAML: {e}")))
    } else {
        serde_json::from_str(&content).map_err(Into::into)
    }
}

async fn load_remote(url: &str, headers: &[(String, String)]) -> Result<Value> {
    let client = reqwest::Client::new();
    let mut builder = client.get(url);
    for (k, v) in headers {
        builder = builder.header(k, v);
    }
    let text = builder.send().await?.text().await?;

    // Try JSON first, then YAML
    if let Ok(v) = serde_json::from_str(&text) {
        Ok(v)
    } else {
        serde_yaml::from_str(&text)
            .map_err(|e| AppError::Protocol(format!("invalid spec format: {e}")))
    }
}
