//! Drawing. Every function here takes `&App` and returns nothing.
//!
//! The reader's house rule is that rendering is pure: widgets are derived from
//! state, and nothing is computed for the first time during a draw. Pane sizes
//! and the active section are already settled by the time this runs, so a
//! frame can be produced twice with identical results — which is what makes
//! the snapshot tests meaningful.

pub mod document;
pub mod help;
pub mod status;

use ratatui::Frame;

use crate::app::state::{App, Overlay};

/// Draw one frame.
pub fn draw(frame: &mut Frame, app: &App) {
    document::draw(frame, app);
    status::draw(frame, app);
    match app.overlay {
        Some(Overlay::Help) => help::draw(frame, app),
        None => {}
    }
}
