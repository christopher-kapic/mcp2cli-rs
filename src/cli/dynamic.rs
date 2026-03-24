use crate::core::types::CommandDef;
use crate::error::{AppError, Result};
use std::collections::HashMap;

/// Build a dynamic clap Command from a list of CommandDefs.
/// Returns the parsed tool name and arguments.
pub fn parse_dynamic_args(
    commands: &[CommandDef],
    argv: &[String],
) -> Result<(String, HashMap<String, String>)> {
    if argv.is_empty() {
        return Err(AppError::Cli("no subcommand provided".into()));
    }

    let subcmd_name = &argv[0];
    let cmd = commands
        .iter()
        .find(|c| c.name == *subcmd_name)
        .ok_or_else(|| AppError::Cli(format!("unknown command: {subcmd_name}")))?;

    let mut app = clap::Command::new(cmd.name.clone()).about(cmd.description.clone());

    for param in &cmd.params {
        let mut arg = clap::Arg::new(param.name.clone())
            .long(param.name.clone())
            .help(param.description.clone());

        if param.required {
            arg = arg.required(true);
        }

        if let Some(ref choices) = param.choices {
            arg = arg.value_parser(choices.clone());
        }

        app = app.arg(arg);
    }

    // Add --stdin flag for commands with body
    if cmd.has_body {
        app = app.arg(
            clap::Arg::new("stdin")
                .long("stdin")
                .help("Read request body from stdin")
                .action(clap::ArgAction::SetTrue),
        );
    }

    let matches = app
        .try_get_matches_from(argv)
        .map_err(|e| AppError::Cli(e.to_string()))?;

    let mut args = HashMap::new();
    for param in &cmd.params {
        if let Some(val) = matches.get_one::<String>(&param.name) {
            args.insert(param.name.clone(), val.clone());
        }
    }

    Ok((subcmd_name.clone(), args))
}
