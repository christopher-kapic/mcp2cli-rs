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
pub async fn load_graphql_schema(
    url: &str,
    headers: &[(String, String)],
) -> Result<IntrospectionSchema> {
    let client = reqwest::Client::new();
    let mut builder = client.post(url);
    for (k, v) in headers {
        builder = builder.header(k, v);
    }

    let body = serde_json::json!({ "query": INTROSPECTION_QUERY });
    let resp: Value = builder.json(&body).send().await?.json().await?;

    let schema_val = resp
        .get("data")
        .and_then(|d| d.get("__schema"))
        .ok_or_else(|| {
            crate::error::AppError::Protocol("no __schema in introspection response".into())
        })?;

    let schema: IntrospectionSchema = serde_json::from_value(schema_val.clone())?;
    Ok(schema)
}
