//! Herdr plugin that deletes a worktree checkout in the background.
//!
//! Herdr's own "Delete worktree checkout..." keeps a modal up, and the client unusable, until
//! `git worktree remove` returns, and the workspace stays open until then. This plugin closes
//! the workspace as soon as the delete is confirmed and runs the removal from a detached process.
//!
//! Entry points, all launched by Herdr from `herdr-plugin.toml`:
//!   action   check the focused workspace and open the confirmation popup
//!   confirm  the popup; Enter starts the worker and exits
//!   worker   detached; closes the workspace, removes the checkout and reports a failure

mod confirm;
mod herdr;
mod theme;
mod themes;
mod worker;

use std::env;
use std::process::ExitCode;

use serde_json::Value;

/// Sidebar token set while a removal runs. Show it with `"$worktree_remove"` in
/// `[ui.sidebar.spaces] rows`.
pub const TOKEN: &str = "worktree_remove";

pub const ENV_WORKSPACE: &str = "DELETE_WORKTREE_WORKSPACE_ID";
pub const ENV_LABEL: &str = "DELETE_WORKTREE_LABEL";
pub const ENV_PATH: &str = "DELETE_WORKTREE_PATH";
pub const ENV_REPO: &str = "DELETE_WORKTREE_REPO_ROOT";
pub const ENV_FORCE: &str = "DELETE_WORKTREE_FORCE";

// Herdr's own text for a workspace it will not remove.
const NOT_LINKED: &str = "workspace is not a linked worktree checkout";

/// The worktree workspace a removal is aimed at, fixed when the action is invoked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub workspace_id: String,
    pub label: String,
    pub path: String,
    pub repo_root: String,
}

impl Target {
    /// Reads the target the action passed to the popup.
    pub fn from_env() -> Option<Target> {
        let workspace_id = env::var(ENV_WORKSPACE).ok().filter(|id| !id.is_empty())?;
        Some(Target {
            label: env::var(ENV_LABEL).unwrap_or_else(|_| workspace_id.clone()),
            path: env::var(ENV_PATH).ok().filter(|path| !path.is_empty())?,
            repo_root: env::var(ENV_REPO).ok().filter(|root| !root.is_empty())?,
            workspace_id,
        })
    }
}

/// What the invocation context says about the focused workspace.
#[derive(Debug, PartialEq, Eq)]
enum Focused {
    Linked(Target),
    NotLinked { label: String },
    Unknown,
}

fn focused(context: &str) -> Focused {
    let Ok(context) = serde_json::from_str::<Value>(context) else {
        return Focused::Unknown;
    };
    let text = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    let Some(workspace_id) = text(&context, "workspace_id") else {
        return Focused::Unknown;
    };
    let label = text(&context, "workspace_label").unwrap_or_else(|| workspace_id.clone());
    let worktree = context.get("worktree").unwrap_or(&Value::Null);
    let linked = worktree.get("is_linked_worktree").and_then(Value::as_bool) == Some(true);
    match (text(worktree, "checkout_path"), text(worktree, "repo_root")) {
        (Some(path), Some(repo_root)) if linked => Focused::Linked(Target {
            workspace_id,
            label,
            path,
            repo_root,
        }),
        _ => Focused::NotLinked { label },
    }
}

fn action() -> ExitCode {
    let context = env::var("HERDR_PLUGIN_CONTEXT_JSON").unwrap_or_default();
    let result = match focused(&context) {
        Focused::Linked(target) => herdr::open_confirm(&target, false),
        Focused::NotLinked { label } => herdr::notify(&label, NOT_LINKED),
        Focused::Unknown => {
            eprintln!("no focused workspace in HERDR_PLUGIN_CONTEXT_JSON");
            return ExitCode::FAILURE;
        }
    };
    // Lands in `herdr plugin log list`.
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {}", error.code, error.message);
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("action") => action(),
        Some("confirm") => confirm::run(),
        Some("worker") => worker::run(&args[1..]),
        _ => {
            eprintln!("usage: herdr-delete-worktree-async <action|confirm|worker>");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_worktree_is_a_target() {
        let context = r#"{"workspace_id":"wN","workspace_label":"feat-x","worktree":{"repo_root":"/r","checkout_path":"/w/feat-x","is_linked_worktree":true}}"#;
        assert_eq!(
            focused(context),
            Focused::Linked(Target {
                workspace_id: "wN".to_string(),
                label: "feat-x".to_string(),
                path: "/w/feat-x".to_string(),
                repo_root: "/r".to_string(),
            })
        );
    }

    #[test]
    fn primary_checkout_is_not_a_target() {
        let context = r#"{"workspace_id":"wC","workspace_label":"Jaca","worktree":{"checkout_path":"/r","is_linked_worktree":false}}"#;
        assert_eq!(
            focused(context),
            Focused::NotLinked {
                label: "Jaca".to_string()
            }
        );
    }

    #[test]
    fn plain_workspace_is_not_a_target() {
        let context = r#"{"workspace_id":"w2"}"#;
        assert_eq!(
            focused(context),
            Focused::NotLinked {
                label: "w2".to_string()
            }
        );
    }

    #[test]
    fn missing_or_broken_context_is_unknown() {
        assert_eq!(focused(""), Focused::Unknown);
        assert_eq!(focused("{}"), Focused::Unknown);
        assert_eq!(focused("not json"), Focused::Unknown);
    }
}
