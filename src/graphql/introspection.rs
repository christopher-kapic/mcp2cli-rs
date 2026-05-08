use crate::error::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Standard GraphQL introspection query.
pub const INTROSPECTION_QUERY: &str = r#"
query IntrospectionQuery {
  __schema {
    queryType { name }
    mutationType { name }
    types {
      kind
      name
      description
      fields(includeDeprecated: true) {
        name
        description
        args {
          name
          description
          type { ...TypeRef }
          defaultValue
        }
        type { ...TypeRef }
        isDeprecated
        deprecationReason
      }
      inputFields {
        name
        description
        type { ...TypeRef }
        defaultValue
      }
      enumValues(includeDeprecated: true) {
        name
        description
        isDeprecated
        deprecationReason
      }
    }
  }
}

fragment TypeRef on __Type {
  kind
  name
  ofType {
    kind
    name
    ofType {
      kind
      name
      ofType {
        kind
        name
        ofType {
          kind
          name
        }
      }
    }
  }
}
"#;

#[derive(Debug, Deserialize, Serialize)]
pub struct IntrospectionSchema {
    #[serde(rename = "queryType")]
    pub query_type: Option<TypeName>,
    #[serde(rename = "mutationType")]
    pub mutation_type: Option<TypeName>,
    pub types: Vec<IntrospectionType>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TypeName {
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IntrospectionType {
    pub kind: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub fields: Option<Vec<IntrospectionField>>,
    #[serde(rename = "inputFields")]
    pub input_fields: Option<Vec<IntrospectionInputValue>>,
    #[serde(rename = "enumValues")]
    pub enum_values: Option<Vec<IntrospectionEnumValue>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IntrospectionField {
    pub name: String,
    pub description: Option<String>,
    pub args: Vec<IntrospectionInputValue>,
    #[serde(rename = "type")]
    pub field_type: Value,
    #[serde(rename = "isDeprecated")]
    pub is_deprecated: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IntrospectionInputValue {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub input_type: Value,
    #[serde(rename = "defaultValue")]
    pub default_value: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IntrospectionEnumValue {
    pub name: String,
    pub description: Option<String>,
}

/// Load a GraphQL schema via introspection.
///
/// Surfaces the underlying failure when introspection cannot return a schema:
/// HTTP errors, non-JSON responses, and GraphQL `errors` arrays are all
/// reported with their actual content instead of a generic "no __schema" message.
pub async fn load_graphql_schema(
    url: &str,
    headers: &[(String, String)],
) -> Result<IntrospectionSchema> {
    use crate::error::AppError;

    let client = reqwest::Client::new();
    let mut builder = client.post(url);
    for (k, v) in headers {
        builder = builder.header(k, v);
    }

    let body = serde_json::json!({ "query": INTROSPECTION_QUERY });
    let response = builder.json(&body).send().await?;

    let status = response.status();
    let body_text = response.text().await?;

    if !status.is_success() {
        let snippet = truncate_for_error(&body_text);
        return Err(AppError::Protocol(format!(
            "GraphQL introspection HTTP {}: {snippet}",
            status.as_u16()
        )));
    }

    let resp: Value = serde_json::from_str(&body_text).map_err(|e| {
        let snippet = truncate_for_error(&body_text);
        AppError::Protocol(format!(
            "GraphQL introspection returned non-JSON response: {e} (body: {snippet})"
        ))
    })?;

    if let Some(errors) = resp.get("errors").and_then(|e| e.as_array()) {
        if !errors.is_empty() {
            let messages: Vec<String> = errors
                .iter()
                .map(|err| {
                    err.get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("(no message)")
                        .to_string()
                })
                .collect();
            return Err(AppError::Protocol(format!(
                "GraphQL introspection errors: {}",
                messages.join("; ")
            )));
        }
    }

    let schema_val = resp
        .get("data")
        .and_then(|d| d.get("__schema"))
        .ok_or_else(|| {
            AppError::Protocol(
            "introspection response missing data.__schema (server may have introspection disabled)"
                .into(),
        )
        })?;

    let schema: IntrospectionSchema = serde_json::from_value(schema_val.clone())?;
    Ok(schema)
}

fn truncate_for_error(s: &str) -> String {
    const MAX: usize = 300;
    let trimmed = s.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        let head: String = trimmed.chars().take(MAX).collect();
        format!("{head}…")
    }
}
