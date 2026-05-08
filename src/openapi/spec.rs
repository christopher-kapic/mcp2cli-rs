use crate::error::{AppError, Result};
use serde_json::Value;
use yaml_rust2::{Yaml, YamlLoader};

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
        parse_yaml(&content).map_err(|e| AppError::Protocol(format!("invalid YAML: {e}")))
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
        parse_yaml(&text).map_err(|e| AppError::Protocol(format!("invalid spec format: {e}")))
    }
}

/// Parse YAML text into `serde_json::Value` via yaml-rust2.
fn parse_yaml(text: &str) -> std::result::Result<Value, String> {
    let docs = YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
    let doc = docs.into_iter().next().unwrap_or(Yaml::Null);
    yaml_to_json(doc)
}

fn yaml_to_json(node: Yaml) -> std::result::Result<Value, String> {
    match node {
        Yaml::Null => Ok(Value::Null),
        Yaml::Boolean(b) => Ok(Value::Bool(b)),
        Yaml::Integer(i) => Ok(Value::Number(i.into())),
        Yaml::Real(s) => s
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| format!("invalid YAML real: {s}")),
        Yaml::String(s) => Ok(Value::String(s)),
        Yaml::Array(items) => items
            .into_iter()
            .map(yaml_to_json)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map(Value::Array),
        Yaml::Hash(map) => {
            let mut obj = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                let key = match k {
                    Yaml::String(s) => s,
                    Yaml::Integer(i) => i.to_string(),
                    Yaml::Real(s) => s,
                    Yaml::Boolean(b) => b.to_string(),
                    Yaml::Null => "null".to_string(),
                    other => return Err(format!("unsupported YAML key: {other:?}")),
                };
                obj.insert(key, yaml_to_json(v)?);
            }
            Ok(Value::Object(obj))
        }
        Yaml::Alias(_) => Err("YAML aliases are not supported".into()),
        Yaml::BadValue => Err("invalid YAML value".into()),
    }
}
