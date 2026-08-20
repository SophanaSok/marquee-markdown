//! Handing the terminal to another program and taking it back.
//!
//! Editing and suspending both need the terminal put back the way it was
//! found — out of raw mode, off the alternate screen — before the other
//! program or the shell touches it. Getting that wrong leaves the reader
//! typing blind into a wedged terminal, so both go through here.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use super::gate;
use super::terminal;

/// Something that needs the terminal to itself.
///
/// Requests are recorded on the application and carried out by the loop, which
/// is the only place that has the terminal. A headless run leaves them
/// recorded and unperformed, which is what lets a test assert that pressing
/// `e` asked to edit the right line without an editor opening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Open a file in the reader's editor, at a line.
    Edit { path: PathBuf, line: usize },
    /// Stop, and let the shell have the terminal back.
    #[cfg(unix)]
    Suspend,
}

/// Carry out a request, restoring the terminal around it.
///
/// # Errors
/// Returns an error when the terminal cannot be restored or the other program
/// cannot be started. The caller should show it rather than exit: failing to
/// open an editor is not a reason to lose the reader's place.
pub fn run(request: &Request, mouse: bool) -> Result<()> {
    // Stand the terminal reader down before the terminal changes hands, and
    // not a moment after: from here until the guard drops, this thread is the
    // only one in the process reading the terminal, so every byte the other
    // program is sent reaches it whole. Without this both processes block on
    // the same tty and each takes an arbitrary half of everything typed —
    // which is why an editor opened this way used to lose keystrokes and spend
    // its startup waiting out timeouts on questions whose answers had already
    // been eaten.
    //
    // A guard rather than a pair of calls, so that a failed restore, a failed
    // editor, or a panic still gives input back.
    let paused = gate::pause();
    terminal::restore(mouse).context("cannot hand back the terminal")?;
    let outcome = match request {
        Request::Edit { path, line } => edit(path, *line),
        #[cfg(unix)]
        Request::Suspend => suspend(),
    };
    // Take the terminal back whatever happened, so a failed editor does not
    // also cost the reader their session.
    let retaken = terminal::enter(mouse).context("cannot take the terminal back");
    // Still paused, and deliberately so: this is the one window in which it is
    // safe for this thread to read the terminal at all. After `enter`, so that
    // nothing queues up behind it and raw mode is back for the parse.
    super::event::discard_pending_input(&paused);
    drop(paused);
    outcome.and(retaken)
}

/// Open `path` in the reader's editor.
fn edit(path: &Path, line: usize) -> Result<()> {
    let (program, mut arguments) = editor();
    arguments.extend(line_arguments(&program, path, line));
    let status = Command::new(&program)
        .args(&arguments)
        .status()
        .with_context(|| format!("cannot run {}", program.to_string_lossy()))?;
    if !status.success() {
        anyhow::bail!("{} exited with {status}", program.to_string_lossy());
    }
    Ok(())
}

/// Stop this process, the way `ctrl+z` does in any other program.
///
/// Sent through `kill` rather than `libc::raise`, which is unsafe: the library
/// forbids unsafe code, and one convenience key is not a reason to give up a
/// guarantee that holds for the whole crate.
///
/// `Command::status` is the SIGCONT handler, structurally. `kill` targets this
/// pid rather than the process group, so the child is not stopped with us; the
/// signal is generated before it exits, so this process stops while blocked in
/// `wait`. The shell then saves the tty modes, and on `fg` restores them,
/// hands the terminal back, and sends SIGCONT — only then does `status` return
/// and the caller take the screen again.
///
/// A stop signal stops every thread, so the terminal reader is frozen here
/// too. The pause in [`run`] is still not redundant: it closes the window
/// between handing the terminal back and the signal actually landing, and it
/// leaves the reader parked on a condvar rather than frozen mid-read holding
/// crossterm's own lock.
#[cfg(unix)]
fn suspend() -> Result<()> {
    let pid = std::process::id().to_string();
    Command::new("kill")
        .args(["-TSTP", &pid])
        .status()
        .context("cannot suspend")?;
    Ok(())
}

/// Which editor to use, and any arguments that came with it.
///
/// `$VISUAL` first, then `$EDITOR`, then something that exists everywhere.
#[must_use]
pub fn editor() -> (OsString, Vec<OsString>) {
    let setting = std::env::var_os("VISUAL")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("EDITOR").filter(|value| !value.is_empty()));
    editor_from(setting.as_deref())
}

/// Split an editor setting into a program and its arguments.
///
/// `EDITOR="emacsclient -nw"` and `EDITOR="code -w"` are ordinary settings, so
/// the whole string cannot be treated as a program name. Pure, so the
/// fallbacks are testable without setting environment variables.
#[must_use]
pub fn editor_from(setting: Option<&std::ffi::OsStr>) -> (OsString, Vec<OsString>) {
    let Some(setting) = setting.map(|value| value.to_string_lossy().into_owned()) else {
        return (default_editor(), Vec::new());
    };
    let mut parts = setting.split_whitespace().map(OsString::from);
    match parts.next() {
        Some(program) => (program, parts.collect()),
        None => (default_editor(), Vec::new()),
    }
}

#[cfg(unix)]
fn default_editor() -> OsString {
    OsString::from("vi")
}

#[cfg(not(unix))]
fn default_editor() -> OsString {
    OsString::from("notepad")
}

/// How to tell an editor which line to open at.
///
/// Every editor spells this differently and most ignore what they do not
/// understand, so an unrecognized one is given the path alone rather than a
/// flag it might treat as a filename.
#[must_use]
pub fn line_arguments(editor: &OsStr, path: &Path, line: usize) -> Vec<OsString> {
    let name = Path::new(editor)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let line = line.max(1);
    let path = OsString::from(path);

    match name.as_str() {
        "vi" | "vim" | "nvim" | "nano" | "emacs" | "emacsclient" | "kak" | "joe" | "pico" => {
            vec![OsString::from(format!("+{line}")), path]
        }
        "hx" | "helix" | "micro" => {
            let mut spec = path;
            spec.push(format!(":{line}"));
            vec![spec]
        }
        "code" | "codium" | "code-insiders" => {
            let mut spec = path;
            spec.push(format!(":{line}"));
            vec![OsString::from("--goto"), spec]
        }
        _ => vec![path],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(editor: &str, line: usize) -> Vec<String> {
        line_arguments(OsStr::new(editor), Path::new("/notes/doc.md"), line)
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn vi_family_editors_take_a_plus_line() {
        for editor in ["vi", "vim", "nvim", "nano", "emacs"] {
            assert_eq!(args(editor, 42), vec!["+42", "/notes/doc.md"], "{editor}");
        }
    }

    #[test]
    fn a_path_with_a_directory_still_resolves_the_editor_name() {
        assert_eq!(args("/usr/bin/nvim", 7), vec!["+7", "/notes/doc.md"]);
    }

    #[test]
    fn helix_takes_the_line_on_the_path() {
        assert_eq!(args("hx", 7), vec!["/notes/doc.md:7"]);
    }

    #[test]
    fn vscode_needs_a_flag_as_well() {
        assert_eq!(args("code", 7), vec!["--goto", "/notes/doc.md:7"]);
    }

    #[test]
    fn an_unknown_editor_gets_the_path_and_nothing_else() {
        // A flag it does not understand would be taken for a second filename.
        assert_eq!(args("my-editor", 7), vec!["/notes/doc.md"]);
    }

    #[test]
    fn line_numbers_start_at_one() {
        assert_eq!(args("vim", 0), vec!["+1", "/notes/doc.md"]);
    }

    #[test]
    fn an_editor_setting_with_arguments_is_split_up() {
        // `EDITOR="emacsclient -nw"` is an ordinary setting, and treating the
        // whole string as a program name would fail to run anything.
        let (program, arguments) = editor_from(Some(OsStr::new("emacsclient -nw")));
        assert_eq!(program, OsString::from("emacsclient"));
        assert_eq!(arguments, vec![OsString::from("-nw")]);
    }

    #[test]
    fn an_unset_or_empty_editor_falls_back() {
        assert!(!editor_from(None).0.is_empty());
        assert!(!editor_from(Some(OsStr::new("   "))).0.is_empty());
        assert!(editor_from(None).1.is_empty());
    }

    #[test]
    fn an_editor_is_always_chosen() {
        // Whatever the environment says, something is returned to run.
        assert!(!editor().0.is_empty());
    }
}
