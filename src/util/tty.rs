//! Terminal detection and sizing.

use std::io::IsTerminal;

/// Whether standard output is a terminal (as opposed to a pipe or file).
#[must_use]
pub fn stdout_is_terminal() -> bool {
    std::io::stdout().is_terminal()
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
/// Honors the `NO_COLOR` convention and falls back to terminal detection.
#[must_use]
pub fn color_disabled() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) || !stdout_is_terminal()
}
