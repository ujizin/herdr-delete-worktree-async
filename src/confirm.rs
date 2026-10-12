//! The confirmation popup. It mirrors Herdr's own delete overlay, text included, and differs in
//! one way: Enter hands the removal to a detached worker and the popup closes immediately.

use std::env;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use crate::theme::{self, Palette, Rgb};
use crate::{worker, Target, ENV_FORCE};

// Herdr's own overlay text. The title is the pane title in herdr-plugin.toml, drawn on the frame.
const REMOVES: &str = " This removes the checkout folder:";
const BRANCH_KEPT: &str = " The branch is not deleted. The Herdr workspace will close.";
const DIRTY: &str = " Dirty or untracked files will be permanently deleted.";
const REMOVE: &str = " ↵ remove ";
const DELETE_ANYWAY: &str = " ↵ delete anyway ";
const CANCEL: &str = " esc cancel ";

#[derive(Debug, PartialEq, Eq)]
enum Key {
    Confirm,
    Cancel,
    Other,
}

fn key(bytes: &[u8]) -> Key {
    match bytes {
        [b'\r'] | [b'\n'] => Key::Confirm,
        // A lone Escape, Ctrl-C, or q. Longer sequences starting with Escape are other keys.
        [0x1b] | [0x03] | [b'q'] => Key::Cancel,
        _ => Key::Other,
    }
}

fn sgr(fg: Option<Rgb>, bg: Option<Rgb>, bold: bool) -> String {
    let mut out = String::from("\x1b[0");
    if bold {
        out.push_str(";1");
    }
    if let Some(Rgb(r, g, b)) = fg {
        out.push_str(&format!(";38;2;{r};{g};{b}"));
    }
    if let Some(Rgb(r, g, b)) = bg {
        out.push_str(&format!(";48;2;{r};{g};{b}"));
    }
    out.push('m');
    out
}

/// Draws the overlay over the whole popup, so every cell carries Herdr's panel background.
fn screen(path: &str, force: bool, palette: &Palette, (cols, rows): (usize, usize)) -> String {
    let bg = palette.panel_bg;
    let base = sgr(palette.text, bg, false);
    // Herdr falls back to reverse video the same way when a theme has no colors.
    let (primary_style, cancel_style) = match (palette.red, palette.surface0) {
        (Some(red), Some(surface0)) => (sgr(bg, Some(red), true), sgr(palette.text, Some(surface0), true)),
        _ => ("\x1b[0;1;7m".to_string(), "\x1b[0;1;7m".to_string()),
    };
    let path: String = path.chars().take(cols.saturating_sub(1)).collect();
    let primary = if force { DELETE_ANYWAY } else { REMOVE };

    let mut lines = [
        format!("{base}{REMOVES}"),
        format!("{} {path}", sgr(palette.subtext0, bg, false)),
        format!("{base}{BRANCH_KEPT}"),
        String::new(),
        String::new(),
        format!("{base}  {primary_style}{primary}{base}  {cancel_style}{CANCEL}"),
    ];
    if force {
        lines[3] = format!("{}{DIRTY}", sgr(palette.red, bg, false));
    }

    let mut out = String::from("\x1b[?25l");
    let blank = " ".repeat(cols);
    for row in 0..rows.max(lines.len()) {
        let line = lines.get(row).map(String::as_str).unwrap_or_default();
        out.push_str(&format!("\x1b[{};1H{base}{blank}\x1b[{};1H{line}", row + 1, row + 1));
    }
    out.push_str("\x1b[0m");
    out
}

/// The popup's size in cells, or Herdr's delete overlay size without its border.
fn size() -> (usize, usize) {
    // SAFETY: winsize is plain data, and it is only read when the ioctl succeeds.
    let size = unsafe {
        let mut size: libc::winsize = std::mem::zeroed();
        (libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) == 0).then_some(size)
    };
    match size {
        Some(size) if size.ws_col > 0 && size.ws_row > 0 => (size.ws_col as usize, size.ws_row as usize),
        _ => (70, 8),
    }
}

/// Puts the terminal in raw mode and restores it on drop.
struct RawMode {
    original: libc::termios,
}

impl RawMode {
    fn enter() -> Option<RawMode> {
        // SAFETY: termios is plain data, and tcgetattr fills it before it is read.
        unsafe {
            let mut original: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut original) != 0 {
                return None;
            }
            let mut raw = original;
            libc::cfmakeraw(&mut raw);
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) != 0 {
                return None;
            }
            Some(RawMode { original })
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // SAFETY: restores the settings read in `enter`.
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original);
        }
    }
}

fn wait_for_key() -> Key {
    let mut stdin = io::stdin().lock();
    let mut buffer = [0u8; 16];
    loop {
        match stdin.read(&mut buffer) {
            // The popup went away under us.
            Ok(0) => return Key::Cancel,
            Ok(read) => match key(&buffer[..read]) {
                Key::Other => continue,
                decided => return decided,
            },
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Key::Cancel,
        }
    }
}

pub fn run() -> ExitCode {
    let Some(target) = Target::from_env() else {
        // Opened directly rather than through the action, so there is nothing to confirm.
        eprintln!("no worktree to delete: open this through the delete action");
        return ExitCode::FAILURE;
    };
    let force = env::var(ENV_FORCE).is_ok_and(|value| value == "1");

    // Without raw mode Enter still arrives as a line, so the prompt keeps working.
    let raw = RawMode::enter();
    let mut stdout = io::stdout();
    let _ = stdout.write_all(screen(&target.path, force, &theme::load(), size()).as_bytes());
    let _ = stdout.flush();
    let decision = wait_for_key();
    let _ = stdout.write_all(b"\x1b[0m\x1b[?25h\x1b[2J\x1b[H");
    let _ = stdout.flush();
    drop(raw);

    if decision != Key::Confirm {
        return ExitCode::SUCCESS;
    }
    match worker::spawn(&target, force) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = crate::herdr::notify(&target.label, &error.to_string());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_confirms() {
        assert_eq!(key(b"\r"), Key::Confirm);
        assert_eq!(key(b"\n"), Key::Confirm);
    }

    #[test]
    fn escape_cancels() {
        assert_eq!(key(&[0x1b]), Key::Cancel);
        assert_eq!(key(&[0x03]), Key::Cancel);
        assert_eq!(key(b"q"), Key::Cancel);
    }

    #[test]
    fn arrow_keys_and_text_do_nothing() {
        assert_eq!(key(b"\x1b[A"), Key::Other);
        assert_eq!(key(b"y"), Key::Other);
        assert_eq!(key(b"\r\r"), Key::Other);
    }

    #[test]
    fn dirty_warning_only_when_forcing() {
        let plain = Palette::default();
        assert!(!screen("/w/x", false, &plain, (70, 8)).contains(DIRTY));
        assert!(screen("/w/x", false, &plain, (70, 8)).contains(REMOVE));
        assert!(screen("/w/x", true, &plain, (70, 8)).contains(DIRTY));
        assert!(screen("/w/x", true, &plain, (70, 8)).contains(DELETE_ANYWAY));
    }

    #[test]
    fn every_row_is_filled_with_the_panel_background() {
        let palette = Palette {
            panel_bg: Some(Rgb(1, 2, 3)),
            ..Palette::default()
        };
        let out = screen("/w/x", false, &palette, (40, 9));
        let filled = format!("\x1b[0;48;2;1;2;3m{}", " ".repeat(40));
        assert_eq!(out.matches(&filled).count(), 9);
    }

    #[test]
    fn long_path_is_cut_to_the_popup_width() {
        let out = screen(&"p".repeat(200), false, &Palette::default(), (20, 8));
        assert!(out.contains(&"p".repeat(19)));
        assert!(!out.contains(&"p".repeat(20)));
    }
}
