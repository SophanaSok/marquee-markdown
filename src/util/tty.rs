//! Terminal detection and sizing.

use std::ffi::OsStr;
use std::io::IsTerminal;

/// Whether standard output is a terminal (as opposed to a pipe or file).
#[must_use]
pub fn stdout_is_terminal() -> bool {
    std::io::stdout().is_terminal()
}

/// Whether standard error is a terminal — whether anyone is there to read
/// a parting notice.
#[must_use]
pub fn stderr_is_terminal() -> bool {
    std::io::stderr().is_terminal()
}

/// Whether standard input carries a document to read.
///
/// This is deliberately narrower than "stdin is not a terminal". A
/// non-interactive shell hands every child a stdin of `/dev/null`, so testing
/// only for a terminal would make `prog file.md` silently render an empty
/// document whenever it runs from a script, a cron job, or a build step.
///
/// Input counts when it is a pipe (`cat x.md | prog`) or a regular file
/// (`prog < x.md`); character devices such as `/dev/null` and terminals do
/// not.
#[must_use]
pub fn stdin_is_pipe() -> bool {
    if std::io::stdin().is_terminal() {
        return false;
    }
    stdin_has_content()
}

#[cfg(unix)]
fn stdin_has_content() -> bool {
    use std::os::unix::fs::FileTypeExt;
    // `/dev/stdin` resolves to this process's own descriptor 0, which lets us
    // inspect its type without unsafe code.
    std::fs::metadata("/dev/stdin").is_ok_and(|meta| {
        let kind = meta.file_type();
        kind.is_fifo() || kind.is_file()
    })
}

#[cfg(not(unix))]
fn stdin_has_content() -> bool {
    // Windows has no equivalent path to stat; a non-terminal stdin is taken at
    // face value, which matches how console applications behave there.
    true
}

/// Terminal width in columns, if a terminal is attached.
#[must_use]
pub fn terminal_width() -> Option<u16> {
    crossterm::terminal::size()
        .ok()
        .map(|(w, _)| w)
        .filter(|w| *w > 0)
}

/// Whether color output should be suppressed.
///
/// Honors the `NO_COLOR` and `CLICOLOR_FORCE`/`FORCE_COLOR` conventions and
/// `TERM=dumb`, then falls back to terminal detection. Forcing is what makes
/// `mmd doc.md | less -R` able to keep its color: the pipe is not a terminal,
/// and before the escape hatch existed only `-p` could say "color anyway".
#[must_use]
pub fn color_disabled() -> bool {
    let force = std::env::var_os("CLICOLOR_FORCE").or_else(|| std::env::var_os("FORCE_COLOR"));
    color_choice(
        std::env::var_os("NO_COLOR").as_deref(),
        force.as_deref(),
        std::env::var_os("TERM").as_deref(),
        stdout_is_terminal(),
    )
}

/// The color decision, pure so it is testable without touching the process
/// environment (which the 2024 edition rightly makes unsafe to mutate).
///
/// Precedence: `NO_COLOR` set and non-empty wins outright — it is the
/// reader's own hand on the switch, per <https://no-color.org>. Then a
/// non-empty, non-`"0"` force variable turns color on, per
/// <https://bixense.com/clicolors/>. Then a terminal that declared itself
/// `dumb` gets none, and finally redirected output gets none.
fn color_choice(
    no_color: Option<&OsStr>,
    force: Option<&OsStr>,
    term: Option<&OsStr>,
    stdout_is_terminal: bool,
) -> bool {
    if no_color.is_some_and(|value| !value.is_empty()) {
        return true;
    }
    if force.is_some_and(|value| !value.is_empty() && value != "0") {
        return false;
    }
    if term.is_some_and(|value| value == "dumb") {
        return true;
    }
    !stdout_is_terminal
}

/// Whether the terminal has declared it understands no escape sequences.
///
/// `TERM=dumb` is how an editor's embedded shell or a captive environment
/// says "send me plain text"; hyperlinks and centering are as unwelcome there
/// as color.
#[must_use]
pub fn term_is_dumb() -> bool {
    std::env::var_os("TERM").is_some_and(|value| value == "dumb")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(value: &str) -> Option<&OsStr> {
        Some(OsStr::new(value))
    }

    #[test]
    fn redirected_output_gets_no_color() {
        assert!(color_choice(None, None, os("xterm-256color"), false));
        assert!(!color_choice(None, None, os("xterm-256color"), true));
    }

    #[test]
    fn no_color_wins_over_everything() {
        assert!(color_choice(os("1"), os("1"), os("xterm-256color"), true));
        // Set but empty is unset, per the convention.
        assert!(!color_choice(os(""), None, os("xterm-256color"), true));
    }

    #[test]
    fn forcing_turns_color_on_for_a_pipe() {
        assert!(!color_choice(None, os("1"), os("xterm-256color"), false));
        // A force of "0" is a way of spelling "do not force".
        assert!(color_choice(None, os("0"), os("xterm-256color"), false));
        assert!(color_choice(None, os(""), os("xterm-256color"), false));
    }

    #[test]
    fn a_dumb_terminal_gets_no_color_unless_forced() {
        assert!(color_choice(None, None, os("dumb"), true));
        assert!(!color_choice(None, os("1"), os("dumb"), true));
    }
}
