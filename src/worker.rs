//! The detached half: closes the workspace, then waits for Git on the user's behalf.

use std::env;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::herdr::{self, HerdrError};
use crate::Target;

// Herdr's own text for a removal in progress.
const REMOVING: &str = "removing…";

/// Starts the worker in its own session with no terminal, so it outlives the popup that
/// launched it.
pub fn spawn(target: &Target, force: bool) -> io::Result<()> {
    let mut command = Command::new(env::current_exe()?);
    command
        .arg("worker")
        .arg(&target.workspace_id)
        .arg(&target.label)
        .arg(&target.path)
        .arg(&target.repo_root);
    if force {
        command.arg("--force");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and touches no state shared with the parent.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn().map(drop)
}

/// Whether Git refused for a reason `--force` overrides, the cases where Herdr's own delete
/// offers "delete anyway".
fn needs_force(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("contains modified or untracked files")
        || message.contains("submodules cannot be moved or removed")
}

// Git's own refusal for a dirty checkout, which is also what Herdr reports.
fn dirty_message(path: &str) -> String {
    format!("fatal: '{path}' contains modified or untracked files, use --force to delete it")
}

fn git(dir: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        format!("git {} failed", args.join(" "))
    } else {
        stderr
    })
}

/// The check `git worktree remove` makes before deleting. `None` when it could not be made, which
/// leaves the decision to Git.
fn is_dirty(path: &str) -> Option<bool> {
    git(path, &["status", "--porcelain"]).ok().map(|out| !out.trim().is_empty())
}

fn log(line: &str) {
    let Some(dir) = env::var_os("HERDR_PLUGIN_STATE_DIR") else {
        return;
    };
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(PathBuf::from(dir).join("worker.log"));
    if let Ok(mut file) = file {
        let _ = writeln!(file, "{seconds} {line}");
    }
}

fn parse_args(args: &[String]) -> Option<(Target, bool)> {
    let mut positional = args.iter().filter(|arg| arg.as_str() != "--force");
    let mut next = || positional.next().filter(|arg| !arg.is_empty()).cloned();
    let target = Target {
        workspace_id: next()?,
        label: next()?,
        path: next()?,
        repo_root: next()?,
    };
    Some((target, args.iter().any(|arg| arg == "--force")))
}

/// Reopens the confirmation in its "delete anyway" form, or reports Git's refusal when another
/// modal is in the way.
fn offer_force(target: &Target, message: &str) {
    if let Err(popup) = herdr::open_confirm(target, true) {
        log(&format!("confirm {} {}: {}", target.workspace_id, popup.code, popup.message));
        report(target, message);
    }
}

fn report(target: &Target, message: &str) {
    if let Err(toast) = herdr::notify(&target.label, message) {
        log(&format!("notify {} {}: {}", target.workspace_id, toast.code, toast.message));
    }
}

fn log_error(what: &str, id: &str, error: &HerdrError) {
    log(&format!("{what} {id} {}: {}", error.code, error.message));
}

pub fn run(args: &[String]) -> ExitCode {
    let Some((target, force)) = parse_args(args) else {
        eprintln!("usage: herdr-delete-worktree-async worker <workspace_id> <label> <path> <repo_root> [--force]");
        return ExitCode::from(2);
    };
    let id = target.workspace_id.as_str();
    log(&format!("remove {id} {} force={force}", target.path));

    // A dirty checkout keeps its workspace, as it does with Herdr's own delete. The token covers
    // the check; it is a courtesy and nothing depends on it.
    if !force {
        if let Err(error) = herdr::report_removing(id, Some(REMOVING)) {
            log_error("token", id, &error);
        }
        if is_dirty(&target.path) == Some(true) {
            log(&format!("dirty {id}"));
            if let Err(error) = herdr::report_removing(id, None) {
                log_error("token", id, &error);
            }
            offer_force(&target, &dirty_message(&target.path));
            return ExitCode::FAILURE;
        }
    }

    // Closing first is what frees the UI: the panes stop and the workspace leaves the sidebar
    // while Git is still deleting files.
    if let Err(error) = herdr::close_workspace(id) {
        log_error("close", id, &error);
        // After a refused removal the workspace is already gone, so a forced one carries on.
        // Otherwise another removal owns this checkout, or the workspace is not there to delete.
        if !force {
            if let Err(error) = herdr::report_removing(id, None) {
                log_error("token", id, &error);
            }
            report(&target, &error.message);
            return ExitCode::FAILURE;
        }
    }

    let mut remove = vec!["worktree", "remove"];
    if force {
        remove.push("--force");
    }
    remove.push(target.path.as_str());
    match git(&target.repo_root, &remove) {
        Ok(_) => {
            log(&format!("removed {id}"));
            ExitCode::SUCCESS
        }
        Err(message) => {
            log(&format!("failed {id} {message}"));
            if !force && needs_force(&message) {
                offer_force(&target, &message);
            } else {
                report(&target, &message);
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn dirty_checkout_needs_force() {
        assert!(needs_force(&dirty_message("/w/x")));
    }

    #[test]
    fn submodules_need_force() {
        assert!(needs_force(
            "fatal: working trees containing submodules cannot be moved or removed"
        ));
    }

    #[test]
    fn other_failures_do_not_offer_force() {
        assert!(!needs_force("fatal: '/w/x' is not a working tree"));
        assert!(!needs_force("fatal: cannot remove a locked working tree"));
    }

    #[test]
    fn args_carry_the_target() {
        let (target, force) = parse_args(&strings(&["wN", "feat-x", "/w/feat-x", "/r", "--force"])).unwrap();
        assert_eq!(target.workspace_id, "wN");
        assert_eq!(target.label, "feat-x");
        assert_eq!(target.path, "/w/feat-x");
        assert_eq!(target.repo_root, "/r");
        assert!(force);
    }

    #[test]
    fn incomplete_target_is_rejected() {
        assert!(parse_args(&[]).is_none());
        assert!(parse_args(&strings(&["--force"])).is_none());
        assert!(parse_args(&strings(&["wN", "feat-x", "/w/feat-x"])).is_none());
        assert!(parse_args(&strings(&["wN", "feat-x", "", "/r"])).is_none());
    }

    #[test]
    fn status_of_a_missing_path_is_unknown() {
        assert_eq!(is_dirty("/nonexistent/delete-worktree-async"), None);
    }
}
