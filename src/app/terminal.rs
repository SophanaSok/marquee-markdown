//! Entering and leaving the full-screen terminal, exactly once each.
//!
//! Raw mode and the alternate screen are process-wide state: a program that
//! exits without undoing them leaves the shell wedged with no echo and no
//! cursor. Restoration therefore happens in a `Drop` impl, and a panic hook
//! runs the same restoration before the panic message is printed — otherwise
//! the message lands on the alternate screen and vanishes with it.

use std::io::{self, Stdout, Write};

use anyhow::{Context, Result};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{execute, queue};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

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
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))
            .context("cannot drive the terminal")?;
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

/// Put the terminal into raw mode and onto the alternate screen.
///
/// Separate from [`Screen`] so it can be called again after another program
/// has had the terminal — an editor, or the shell after a suspend.
///
/// # Errors
/// Returns an error when the terminal will not switch modes.
pub fn enter(mouse: bool) -> Result<()> {
    enable_raw_mode().context("cannot put the terminal into raw mode")?;
    let mut stdout = io::stdout();
    if let Err(error) = queue!(stdout, EnterAlternateScreen, Hide) {
        // Undo the half-applied state rather than leaving the shell in it.
        let _ = disable_raw_mode();
        return Err(error).context("cannot open the alternate screen");
    }
    if mouse {
        let _ = queue!(stdout, EnableMouseCapture);
    }
    stdout.flush().context("cannot set up the terminal")?;
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
    if mouse {
        let _ = queue!(stdout, DisableMouseCapture);
    }
    execute!(stdout, LeaveAlternateScreen, Show)?;
    disable_raw_mode()
}

/// Restore the terminal before any panic reaches the screen.
///
/// Called before entering full-screen mode so that a panic during setup is
/// still readable.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore(true);
        previous(info);
    }));
}
