//! Drawing. Every function here takes `&App` and returns nothing.
//!
//! The reader's house rule is that rendering is pure: widgets are derived from
//! state, and nothing is computed for the first time during a draw. Pane sizes
//! and the active section are already settled by the time this runs, so a
//! frame can be produced twice with identical results — which is what makes
//! the snapshot tests meaningful.

pub mod browser;
pub mod document;
pub mod help;
pub mod status;
pub mod themes;
pub mod toc;

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::app::state::{App, Overlay, Screen};

/// Draw one frame.
pub fn draw(frame: &mut Frame, app: &App) {
    match app.screen {
        Screen::Browser => browser::draw(frame, app),
        Screen::Document => {
            document::draw(frame, app);
            toc::draw(frame, app);
        }
    }
    status::draw(frame, app);
    match app.overlay {
        Some(Overlay::Help) => help::draw(frame, app),
        Some(Overlay::Themes) => themes::draw(frame, app),
        None => {}
    }
}

/// A rectangle of at most `width` by `height`, centered in `area`.
///
/// Shared by the overlay panels so they agree about where the middle is.
#[must_use]
pub(crate) fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}
