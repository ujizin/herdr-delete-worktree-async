//! Calls back into Herdr through its CLI, which is the plugin API.

use std::env;
use std::process::{Command, Stdio};

use serde_json::Value;

/// An error reported by the Herdr server, or the reason the CLI could not be run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrError {
    pub code: String,
    pub message: String,
}

pub fn plugin_id() -> String {
    env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "dev.ujizin.delete-worktree-async".to_string())
}

fn bin() -> String {
    env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".to_string())
}

/// Runs `herdr <args>` to completion.
pub fn run(args: &[&str]) -> Result<Value, HerdrError> {
    let output = Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| HerdrError {
            code: "spawn_failed".to_string(),
            message: err.to_string(),
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_output(output.status.success(), &stdout, &stderr)
}

/// Server errors are JSON on stderr with exit status 1. CLI syntax errors are plain text.
fn parse_output(success: bool, stdout: &str, stderr: &str) -> Result<Value, HerdrError> {
    for text in [stderr, stdout] {
        if let Some(error) = parse_error(text) {
            return Err(error);
        }
    }
    if !success {
        let message = [stderr.trim(), stdout.trim()]
            .into_iter()
            .find(|text| !text.is_empty())
            .unwrap_or("herdr exited with an error")
            .to_string();
        return Err(HerdrError {
            code: "command_failed".to_string(),
            message,
        });
    }
    Ok(serde_json::from_str(stdout.trim()).unwrap_or(Value::Null))
}

fn parse_error(text: &str) -> Option<HerdrError> {
    let value: Value = serde_json::from_str(text.trim()).ok()?;
    let error = value.get("error")?;
    Some(HerdrError {
        code: error.get("code")?.as_str()?.to_string(),
        message: error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

pub fn close_workspace(workspace_id: &str) -> Result<Value, HerdrError> {
    run(&["workspace", "close", workspace_id])
}

pub fn notify(title: &str, body: &str) -> Result<Value, HerdrError> {
    run(&["notification", "show", title, "--body", body])
}

/// Sets or clears the sidebar token that marks a workspace as being removed.
pub fn report_removing(workspace_id: &str, value: Option<&str>) -> Result<Value, HerdrError> {
    let source = plugin_id();
    let mut args = vec![
        "workspace",
        "report-metadata",
        workspace_id,
        "--source",
        source.as_str(),
    ];
    let token;
    match value {
        Some(value) => {
            token = format!("{}={value}", crate::TOKEN);
            args.extend(["--token", token.as_str()]);
        }
        None => args.extend(["--clear-token", crate::TOKEN]),
    }
    run(&args)
}

pub fn open_confirm(target: &crate::Target, force: bool) -> Result<Value, HerdrError> {
    let plugin = plugin_id();
    let workspace = format!("{}={}", crate::ENV_WORKSPACE, target.workspace_id);
    let label = format!("{}={}", crate::ENV_LABEL, target.label);
    let path = format!("{}={}", crate::ENV_PATH, target.path);
    let repo = format!("{}={}", crate::ENV_REPO, target.repo_root);
    let forced = format!("{}={}", crate::ENV_FORCE, if force { "1" } else { "0" });
    run(&[
        "plugin",
        "pane",
        "open",
        "--plugin",
        plugin.as_str(),
        "--entrypoint",
        "confirm",
        "--env",
        workspace.as_str(),
        "--env",
        label.as_str(),
        "--env",
        path.as_str(),
        "--env",
        repo.as_str(),
        "--env",
        forced.as_str(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_error_on_stderr_is_parsed() {
        let stderr = r#"{"id":"cli:worktree:remove","error":{"code":"dirty_worktree_requires_force","message":"fatal: 'x' contains modified or untracked files, use --force to delete it"}}"#;
        let error = parse_output(false, "", stderr).unwrap_err();
        assert_eq!(error.code, "dirty_worktree_requires_force");
        assert!(error.message.contains("use --force"));
    }

    #[test]
    fn plain_text_failure_keeps_the_text() {
        let error = parse_output(false, "", "error: unexpected argument\n").unwrap_err();
        assert_eq!(error.code, "command_failed");
        assert_eq!(error.message, "error: unexpected argument");
    }

    #[test]
    fn silent_failure_still_reports_an_error() {
        let error = parse_output(false, "", "").unwrap_err();
        assert_eq!(error.code, "command_failed");
        assert!(!error.message.is_empty());
    }

    #[test]
    fn success_returns_the_result() {
        let stdout = r#"{"id":"x","result":{"type":"worktree_removed","workspace_id":"w1"}}"#;
        let value = parse_output(true, stdout, "").unwrap();
        assert_eq!(value["result"]["type"], "worktree_removed");
    }

    #[test]
    fn success_without_json_is_not_an_error() {
        assert_eq!(parse_output(true, "", "").unwrap(), Value::Null);
    }
}
