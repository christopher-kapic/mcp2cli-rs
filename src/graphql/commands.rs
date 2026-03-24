use crate::core::helpers::to_kebab;
use crate::core::types::{CommandDef, ParamDef, ParamLocation};
use crate::graphql::introspection::IntrospectionSchema;
use crate::graphql::types::graphql_type_to_rust;

/// Extract CommandDefs from a GraphQL introspection schema.
pub fn extract_graphql_commands(schema: &IntrospectionSchema) -> Vec<CommandDef> {
    let mut commands = Vec::new();
    let mut names_seen = std::collections::HashSet::new();

    let query_type_name = schema.query_type.as_ref().map(|t| t.name.as_str());
    let mutation_type_name = schema.mutation_type.as_ref().map(|t| t.name.as_str());

    for type_def in &schema.types {
        let type_name = match &type_def.name {
            Some(n) => n.as_str(),
            None => continue,
        };

        let op_type = if Some(type_name) == query_type_name {
            "query"
        } else if Some(type_name) == mutation_type_name {
            "mutation"
        } else {
            continue;
        };

        if let Some(ref fields) = type_def.fields {
            for field in fields {
                let base_name = to_kebab(&field.name);
                let name = if names_seen.contains(&base_name) {
                    format!("{op_type}-{base_name}")
                } else {
                    base_name.clone()
                };
                names_seen.insert(base_name);

                let params: Vec<ParamDef> = field
                    .args
                    .iter()
                    .map(|arg| {
                        let rust_type = graphql_type_to_rust(&arg.input_type);
                        let (_, is_non_null, _) =
                            crate::graphql::types::unwrap_type(&arg.input_type);
                        ParamDef {
                            name: to_kebab(&arg.name),
                            original_name: arg.name.clone(),
                            rust_type,
                            required: is_non_null,
                            description: arg.description.clone().unwrap_or_default(),
                            choices: None,
                            location: ParamLocation::GraphqlArg,
                            schema: arg.input_type.clone(),
                        }
                    })
                    .collect();

                commands.push(CommandDef {
                    name,
                    description: field.description.clone().unwrap_or_default(),
                    params,
                    has_body: false,
                    method: None,
                    path: None,
                    content_type: None,
                    tool_name: None,
                    graphql_operation_type: Some(op_type.to_string()),
                    graphql_field_name: Some(field.name.clone()),
                    graphql_return_type: Some(field.field_type.clone()),
                });
            }
        }
    }

    commands
}
