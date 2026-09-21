//! Shared command-line parsing for backend processes.

use std::{ffi::OsString, path::PathBuf};

use anyhow::{Result, anyhow, bail};

/// Action requested through command-line arguments.
#[derive(Debug, Eq, PartialEq)]
pub enum Command {
    Run { config_path: PathBuf },
    Help,
}

/// Parses process arguments after the binary name.
///
/// # Errors
///
/// Returns an error for unknown arguments, a missing `--config` value, or a
/// missing required `--config` argument.
pub fn parse() -> Result<Command> {
    parse_args(std::env::args_os().skip(1))
}

/// Prints command usage in the same form for API and worker processes.
pub fn print_usage(binary_name: &str) {
    println!("Usage: {binary_name} --config <path-to-config.toml>");
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Command> {
    let mut args = args.into_iter();
    let mut config_path = None;

    while let Some(argument) = args.next() {
        let argument = argument
            .into_string()
            .map_err(|_| anyhow!("arguments must be valid UTF-8"))?;
        match argument.as_str() {
            "--config" | "-c" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("missing value after {argument}"))?;
                config_path = Some(PathBuf::from(value));
            }
            "--help" | "-h" => return Ok(Command::Help),
            _ if argument.starts_with("--config=") => {
                config_path = Some(PathBuf::from(&argument["--config=".len()..]));
            }
            _ => bail!("unknown argument: {argument}"),
        }
    }

    config_path.map_or_else(
        || bail!("missing required --config <path> argument"),
        |config_path| Ok(Command::Run { config_path }),
    )
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_args};
    use std::{ffi::OsString, path::PathBuf};

    #[test]
    fn parses_supported_config_forms() {
        assert_eq!(
            parse_args(["--config", "Config.toml"].map(OsString::from)).unwrap(),
            Command::Run {
                config_path: PathBuf::from("Config.toml")
            }
        );
        assert_eq!(
            parse_args(["--config=staging.toml"].map(OsString::from)).unwrap(),
            Command::Run {
                config_path: PathBuf::from("staging.toml")
            }
        );
    }

    #[test]
    fn config_path_is_required() {
        assert!(parse_args([]).is_err());
        assert!(parse_args([OsString::from("--config")]).is_err());
    }
}
