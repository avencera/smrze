use std::process::{Command, Output};

use crate::error::{BuildSupportError, Result};

pub fn developer_dir() -> String {
    let mut command = Command::new("xcode-select");
    command.arg("--print-path");
    command_output(&mut command, "xcode-select --print-path")
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| output.trim().to_owned())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| "/Applications/Xcode.app/Contents/Developer".to_owned())
}

pub fn run_checked_command(command: &mut Command, action: &str) -> Result<Output> {
    let output = command_output(command, action)?;
    if output.status.success() {
        return Ok(output);
    }

    Err(BuildSupportError::new(format!(
        "{action} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )))
}

pub(crate) fn command_output(command: &mut Command, action: &str) -> Result<Output> {
    command
        .output()
        .map_err(|error| BuildSupportError::new(format!("{action}: {error}")))
}
