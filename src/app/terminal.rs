//! Entering and leaving the full-screen terminal, exactly once each.
//!
//! Raw mode and the alternate screen are process-wide state: a program that
//! exits without undoing them leaves the shell wedged with no echo and no
//! cursor. Restoration therefore happens in a `Drop` impl, and a panic hook
//! runs the same restoration before the panic message is printed — otherwise
//! the message lands on the alternate screen and vanishes with it.
//!
//! `setup` and `teardown` are mirrors, and a test holds them to it. Every
//! mode this program depends on has to be asked for here, because the state it
//! inherits is whatever the last program to run happened to leave behind — and
//! an editor opened with `e` leaves behind its own idea of the terminal, not
//! ours.

use std::io::{self, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::queue;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

/// Whether mouse capture is on, for the panic hook to read.
///
/// The hook has no application to ask, and guessing means sending a terminal
/// the undo for a mode it never had. Harmless for the modes used here, but a
/// mirror is cheap and does not have to be re-reasoned about the next time one
/// is added.
static MOUSE_CAPTURED: AtomicBool = AtomicBool::new(false);

/// A terminal in full-screen mode, restored when dropped.
pub struct Screen {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    mouse: bool,
}

impl Screen {
    /// Take over the terminal.
    ///
    /// # Errors
    /// Returns an error when the terminal cannot be switched into raw mode or
    /// the alternate screen.
    pub fn enter(mouse: bool) -> Result<Self> {
        enter(mouse)?;
        // Restoring by hand rather than leaning on `Drop`: there is no
        // `Screen` yet to drop, so failing here would otherwise exit with raw
        // mode and the alternate screen still on — the wedged shell this
        // module exists to prevent.
        let terminal = match Terminal::new(CrosstermBackend::new(io::stdout())) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = restore(mouse);
                return Err(error).context("cannot drive the terminal");
            }
        };
        Ok(Self { terminal, mouse })
    }

    /// The underlying terminal, for drawing.
    pub fn terminal(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        // Nothing here can be reported: the caller may already be unwinding,
        // and failing to restore is not something a reader can act on.
        let _ = restore(self.mouse);
    }
}

/// The escape sequences [`enter`] writes, after raw mode is on.
///
/// Split out from it so that a test can check [`teardown`] is the exact mirror
/// without a terminal: raw mode is an ioctl, but all of this is bytes.
fn setup(out: &mut impl Write, mouse: bool) -> io::Result<()> {
    queue!(out, EnterAlternateScreen, Hide)?;
    // Pasted text arrives as one event only if the terminal is told to bracket
    // it. Without this the newline in a two-line paste is an ordinary Enter,
    // which submits the search prompt and leaves the rest of the paste being
    // dispatched as bindings — `q` quits, `e` opens an editor.
    //
    // Best-effort, like the mouse below: the legacy Windows console refuses
    // this outright, and a terminal without bracketed paste is still one this
    // program has to run on.
    let _ = queue!(out, EnableBracketedPaste);
    if mouse {
        let _ = queue!(out, EnableMouseCapture);
    }
    Ok(())
}

/// The exact reverse of [`setup`].
///
/// Except for the cursor: `Show` comes after leaving the alternate screen
/// rather than in mirror position, because visibility is per-screen on some
/// terminals and the reader has to be left able to see it on the screen they
/// are actually looking at.
fn teardown(out: &mut impl Write, mouse: bool) -> io::Result<()> {
    if mouse {
        // Deliberately not part of the chain below. On Windows this reads a
        // console mode saved by `EnableMouseCapture` and fails if capture was
        // never enabled, and a `?` here would abandon the alternate screen.
        let _ = queue!(out, DisableMouseCapture);
    }
    let _ = queue!(out, DisableBracketedPaste);
    queue!(out, LeaveAlternateScreen, Show)
}

/// Put the terminal into raw mode and onto the alternate screen.
///
/// Separate from [`Screen`] so it can be called again after another program
/// has had the terminal — an editor, or the shell after a suspend.
///
/// # Errors
/// Returns an error when the terminal will not switch modes.
pub fn enter(mouse: bool) -> Result<()> {
    enable_raw_mode().context("cannot put the terminal into raw mode")?;
    MOUSE_CAPTURED.store(mouse, Ordering::SeqCst);
    let mut stdout = io::stdout();
    if let Err(error) = setup(&mut stdout, mouse) {
        // Undo the half-applied state rather than leaving the shell in it.
        let _ = restore(mouse);
        return Err(error).context("cannot open the alternate screen");
    }
    if let Err(error) = stdout.flush() {
        // The alternate screen may already be showing, so this needs the full
        // restoration and not just raw mode.
        let _ = restore(mouse);
        return Err(error).context("cannot set up the terminal");
    }
    Ok(())
}

/// Undo everything [`enter`] did.
///
/// Safe to call when the terminal was never taken over, which is what lets the
/// panic hook call it unconditionally.
///
/// # Errors
/// Propagates failures writing to standard output.
pub fn restore(mouse: bool) -> io::Result<()> {
    let mut stdout = io::stdout();
    let written = teardown(&mut stdout, mouse).and_then(|()| stdout.flush());
    MOUSE_CAPTURED.store(false, Ordering::SeqCst);
    // Unconditionally, and last. A write that failed part way through is
    // exactly when leaving raw mode matters most: reporting the error into a
    // shell with no echo and no line editing helps nobody.
    let left_raw_mode = disable_raw_mode();
    written.and(left_raw_mode)
}

/// Restore the terminal before any panic reaches the screen.
///
/// Called before entering full-screen mode so that a panic during setup is
/// still readable.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore(MOUSE_CAPTURED.load(Ordering::SeqCst));
        previous(info);
    }));
}

// Unix only, and not because the invariant is: crossterm drives some of these
// modes through the console API on Windows rather than as escape sequences, so
// queueing them into a buffer would reach for the real console and write
// nothing to inspect. The mirror still has to hold there; it just cannot be
// read off the bytes.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Every DEC private mode the given bytes set or unset, and which way.
    ///
    /// Parsing what was written rather than trusting the call list is the
    /// point: it is the bytes the terminal sees that have to balance.
    fn private_modes(bytes: &[u8]) -> BTreeMap<u16, bool> {
        let text = String::from_utf8(bytes.to_vec()).expect("escape sequences are ascii");
        let mut modes = BTreeMap::new();
        for sequence in text.split('\u{1b}').skip(1) {
            let Some(body) = sequence.strip_prefix("[?") else {
                continue;
            };
            let Some(set) = body.chars().find(|c| *c == 'h' || *c == 'l') else {
                continue;
            };
            // A sequence may carry several modes at once, as the mouse ones do.
            for mode in body
                .split(|c: char| !c.is_ascii_digit())
                .filter(|part| !part.is_empty())
            {
                if let Ok(mode) = mode.parse::<u16>() {
                    modes.insert(mode, set == 'h');
                }
            }
        }
        modes
    }

    #[test]
    fn leaving_undoes_exactly_what_entering_did() {
        // The regression this catches is adding a mode to `setup` and
        // forgetting it in `teardown`, which leaves the reader's shell in a
        // state they did not choose and cannot see.
        for mouse in [false, true] {
            let (mut on, mut off) = (Vec::new(), Vec::new());
            setup(&mut on, mouse).expect("setup");
            teardown(&mut off, mouse).expect("teardown");
            let set = private_modes(&on);
            let unset = private_modes(&off);
            assert_eq!(
                set.keys().collect::<Vec<_>>(),
                unset.keys().collect::<Vec<_>>(),
                "the modes touched differ (mouse: {mouse})"
            );
            for (mode, on) in &set {
                assert_eq!(unset[mode], !on, "mode {mode} was not undone");
            }
        }
    }

    #[test]
    fn bracketed_paste_is_asked_for_so_a_pasted_newline_is_not_a_keypress() {
        // Without it the newline in a multi-line paste submits the search
        // prompt, and the rest of the paste runs as bindings.
        let mut on = Vec::new();
        setup(&mut on, false).expect("setup");
        assert_eq!(private_modes(&on).get(&2004), Some(&true));
    }

    #[test]
    fn the_mouse_is_only_touched_when_it_was_asked_for() {
        let mut without = Vec::new();
        setup(&mut without, false).expect("setup");
        assert!(!private_modes(&without).contains_key(&1000));
        let mut with = Vec::new();
        setup(&mut with, true).expect("setup");
        assert_eq!(private_modes(&with).get(&1000), Some(&true));
    }
}
