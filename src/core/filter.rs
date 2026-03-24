use crate::core::types::CommandDef;

/// Filter commands by include/exclude glob patterns and HTTP methods.
pub fn filter_commands(
    commands: Vec<CommandDef>,
    include: &[String],
    exclude: &[String],
    methods: &[String],
) -> Vec<CommandDef> {
    commands
        .into_iter()
        .filter(|cmd| {
            // Include filter
            if !include.is_empty()
                && !include
                    .iter()
                    .any(|pat| glob_match::glob_match(pat, &cmd.name))
            {
                return false;
            }
            // Exclude filter
            if exclude
                .iter()
                .any(|pat| glob_match::glob_match(pat, &cmd.name))
            {
                return false;
            }
            // Methods filter (OpenAPI only)
            if !methods.is_empty() {
                if let Some(ref method) = cmd.method {
                    if !methods.iter().any(|m| m.eq_ignore_ascii_case(method)) {
                        return false;
                    }
                }
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::CommandDef;

    fn make_cmd(name: &str, method: Option<&str>) -> CommandDef {
        CommandDef {
            name: name.to_string(),
            description: String::new(),
            params: vec![],
            has_body: false,
            method: method.map(String::from),
            path: None,
            content_type: None,
            tool_name: None,
            graphql_operation_type: None,
            graphql_field_name: None,
            graphql_return_type: None,
        }
    }

    #[test]
    fn test_include_filter() {
        let cmds = vec![
            make_cmd("get-users", None),
            make_cmd("get-posts", None),
            make_cmd("delete-user", None),
        ];
        let result = filter_commands(cmds, &["get-*".into()], &[], &[]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_exclude_filter() {
        let cmds = vec![make_cmd("get-users", None), make_cmd("delete-user", None)];
        let result = filter_commands(cmds, &[], &["delete-*".into()], &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "get-users");
    }

    #[test]
    fn test_methods_filter() {
        let cmds = vec![
            make_cmd("get-users", Some("GET")),
            make_cmd("create-user", Some("POST")),
            make_cmd("delete-user", Some("DELETE")),
        ];
        let result = filter_commands(cmds, &[], &[], &["GET".into(), "POST".into()]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "get-users");
        assert_eq!(result[1].name, "create-user");
    }

    #[test]
    fn test_combined_include_exclude() {
        let cmds = vec![
            make_cmd("get-users", None),
            make_cmd("get-posts", None),
            make_cmd("get-users-admin", None),
        ];
        let result = filter_commands(cmds, &["get-users*".into()], &["*admin*".into()], &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "get-users");
    }

    #[test]
    fn test_empty_filters_pass_all() {
        let cmds = vec![
            make_cmd("a", None),
            make_cmd("b", None),
            make_cmd("c", None),
        ];
        let result = filter_commands(cmds, &[], &[], &[]);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_methods_filter_case_insensitive() {
        let cmds = vec![
            make_cmd("get-users", Some("GET")),
            make_cmd("create-user", Some("POST")),
        ];
        let result = filter_commands(cmds, &[], &[], &["get".into()]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "get-users");
    }
}
