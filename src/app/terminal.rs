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
//!
//! There is one deliberate asymmetry, and it follows from that same sentence:
//! this program depends on mouse tracking being *off* unless it asked for a
//! wheel, so `setup` turns it off rather than assuming nobody else turned it
//! on. What entering switches off, leaving leaves off; the test states that as
//! its contract rather than a strict inversion.

use std::fmt;
use std::io::{self, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use crossterm::Command;
use crossterm::cursor::{Hide, Show};
#[cfg(windows)]
use crossterm::event::EnableMouseCapture;
use crossterm::event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste};
use crossterm::queue;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

/// Whether the wheel was asked for, for the panic hook to read.
///
/// The hook has no application to ask. Mouse tracking is turned off either way
/// now, so what this decides is only *which* undo goes out — the one that puts
/// a saved Windows console mode back, which fails if capture was never enabled,
/// or the plain escape sequences that are safe to send to anything.
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

/// Ask for the wheel, and for nothing that moves.
///
/// Crossterm's `EnableMouseCapture` also turns on button-event (`?1002h`) and
/// any-event (`?1003h`) tracking, which makes the terminal report every cell
/// the pointer crosses. Nothing here reads a mouse column or row:
/// `update::mouse_event` acts on the four scroll kinds and drops the rest, so
/// each of those reports buys a wakeup, a reconcile, and a whole frame drawn
/// and diffed to no effect — invisible, because the diff comes out empty, and
/// paid for the whole time a hand rests on the mouse.
///
/// `?1000h` alone still reports the wheel: it is buttons 4 to 7 of the same
/// press encoding, which is why `less --mouse` asks for exactly this pair.
/// `?1006h` is not a second feature but the fix for the first — the original
/// encoding puts the column in one byte and cannot express one past 223.
struct EnableWheel;

impl Command for EnableWheel {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        f.write_str("\x1b[?1000h\x1b[?1006h")
    }

    /// The legacy console has no escape sequence for this; crossterm reaches
    /// for the console API, where mouse input is one flag with no separate
    /// motion bit — so there is nothing finer to ask for and nothing to be
    /// gained by reimplementing it. Delegating also leaves behind the saved
    /// console mode that `DisableMouseCapture` restores from, which it fails
    /// without.
    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        EnableMouseCapture.execute_winapi()
    }

    /// Never the ANSI path on Windows, whatever the console claims to support:
    /// crossterm reads Windows input as console records rather than as bytes,
    /// so a mode set by writing to the screen is one nothing would report.
    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        false
    }
}

/// Turn off every mouse mode, including the ones this program never asks for.
///
/// Deliberately not the mirror of [`EnableWheel`], and deliberately sent even
/// when no wheel was asked for. What this program inherits is whatever the last
/// program left behind, and an editor opened with `e` and then killed leaves
/// any-event tracking on for good: the terminal then reports every pointer
/// movement to a reader that has no use for one and redraws for each. Clearing
/// the whole set costs five sequences, once.
///
/// The cost of being thorough is that a full-screen program which spawns this
/// one as its pager gets its own tracking cleared too. That program is already
/// obliged to re-initialize on the way back — the alternate screen alone sees
/// to that — so the trade is taken deliberately rather than by omission.
///
/// Nothing to do on the legacy console: mouse input there is a console mode,
/// not something another program can leave set through bytes on our screen.
struct DisableAllMouse;

impl Command for DisableAllMouse {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        // The same modes in the same order as crossterm's `DisableMouseCapture`,
        // so the two branches of [`teardown`] write identical bytes everywhere
        // the bytes are what the terminal sees.
        f.write_str("\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        false
    }
}

/// The escape sequences [`enter`] writes, after raw mode is on.
///
/// Split out from it so that a test can hold [`teardown`] to what it undoes
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
    // Before the enable below, which names mode 1000 again: written the other
    // way round, the clear would undo it.
    let _ = queue!(out, DisableAllMouse);
    if mouse {
        let _ = queue!(out, EnableWheel);
    }
    Ok(())
}

/// The reverse of [`setup`], except where [`setup`] was already undoing
/// somebody else's state.
///
/// The cursor is the other exception: `Show` comes after leaving the alternate
/// screen rather than in mirror position, because visibility is per-screen on
/// some terminals and the reader has to be left able to see it on the screen
/// they are actually looking at.
fn teardown(out: &mut impl Write, mouse: bool) -> io::Result<()> {
    // Deliberately not part of the chain below. On Windows the first of these
    // reads a console mode saved by `EnableWheel` and fails if capture was
    // never enabled, and a `?` here would abandon the alternate screen. The
    // two branches write the same bytes on every platform where bytes are what
    // the terminal sees; they differ only in whether there is a console mode
    // to put back.
    if mouse {
        let _ = queue!(out, DisableMouseCapture);
    } else {
        let _ = queue!(out, DisableAllMouse);
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

    /// The modes [`setup`] writes, and which way it writes them.
    fn entering(mouse: bool) -> BTreeMap<u16, bool> {
        let mut out = Vec::new();
        setup(&mut out, mouse).expect("setup");
        private_modes(&out)
    }

    #[test]
    fn leaving_undoes_exactly_what_entering_did() {
        // The regression this catches is adding a mode to `setup` and
        // forgetting it in `teardown`, which leaves the reader's shell in a
        // state they did not choose and cannot see.
        //
        // Not a strict inversion, and the difference is the point: `setup`
        // turns some modes *off* — mouse tracking it never asked for and may
        // have inherited — and `teardown` has to leave those off rather than
        // politely putting somebody else's junk back. So what is held to is
        // that both directions name the same modes, that nothing entering
        // turned on is left on, and that nothing leaving turns on was not
        // something entering turned off. The cursor is what satisfies the
        // last of those; an undo for a mode `setup` never mentions is what
        // would fail it.
        for mouse in [false, true] {
            let mut bytes = Vec::new();
            teardown(&mut bytes, mouse).expect("teardown");
            let entering = entering(mouse);
            let leaving = private_modes(&bytes);
            assert_eq!(
                entering.keys().collect::<Vec<_>>(),
                leaving.keys().collect::<Vec<_>>(),
                "the modes touched differ (mouse: {mouse})"
            );
            for mode in entering.iter().filter(|(_, on)| **on).map(|(mode, _)| mode) {
                assert_eq!(leaving.get(mode), Some(&false), "mode {mode} was left on");
            }
            for mode in leaving.iter().filter(|(_, on)| **on).map(|(mode, _)| mode) {
                assert_eq!(
                    entering.get(mode),
                    Some(&false),
                    "leaving turns on mode {mode}, which entering never turned off"
                );
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
    fn the_wheel_is_only_reported_when_it_was_asked_for() {
        assert_eq!(entering(false).get(&1000), Some(&false));
        assert_eq!(entering(true).get(&1000), Some(&true));
    }

    #[test]
    fn motion_reporting_is_never_asked_for() {
        // Button-event and any-event tracking, which crossterm's
        // `EnableMouseCapture` bundles in with the wheel. Nothing here reads a
        // mouse column, so every report they produce is a frame drawn to no
        // effect — for as long as a hand rests on the mouse.
        for mode in [1002, 1003] {
            assert_eq!(entering(true).get(&mode), Some(&false), "mode {mode}");
        }
    }

    #[test]
    fn mouse_tracking_left_on_by_another_program_is_cleared_on_the_way_in() {
        // Not decoration: `enter` also runs on the way back from an editor,
        // and an editor that died without cleaning up leaves any-event
        // tracking on. Held for the run that never wanted a mouse, which is
        // the one that would otherwise redraw for every pointer movement for
        // as long as it stays open.
        for mode in [1000, 1002, 1003, 1006, 1015] {
            assert_eq!(entering(false).get(&mode), Some(&false), "mode {mode}");
        }
    }

    #[test]
    fn the_inherited_clear_comes_before_the_wheel_is_asked_for() {
        // Both name mode 1000, and the mode map cannot see the difference: it
        // keeps the last write per mode, which is exactly what the terminal
        // does. Written the wrong way round the clear would undo the enable
        // and every test above would still pass.
        let mut on = Vec::new();
        setup(&mut on, true).expect("setup");
        let text = String::from_utf8(on).expect("escape sequences are ascii");
        let clear = text.find("?1000l").expect("the clear");
        let enable = text.find("?1000h").expect("the enable");
        assert!(clear < enable, "the clear would undo the enable");
    }
}
