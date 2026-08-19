//! State that is recomputed rather than tracked.
//!
//! Runs once per iteration, after the update and before drawing. Everything
//! here is a function of the document, the view, and the pane sizes; putting
//! it in one place is what keeps drawing free of mutation.

use super::state::{App, Focus};

/// Bring derived state back in line with the document and the view.
pub fn sync(app: &mut App) {
    let extent = app.extent();
    app.view.clamp(extent);
    // The active section is scroll-derived and never written back into the
    // contents cursor: scrolling moves the highlight without dragging the
    // cursor around, and moving the cursor does not pretend the reader
    // scrolled.
    app.active = app.doc.doc().active_anchor(app.view.top);

    // A pane that is not on screen cannot hold focus, or keys would go
    // somewhere the reader cannot see.
    if app.panes.sidebar.is_none() {
        app.focus = Focus::Document;
    }

    sync_toc(app);

    // Matches are line indices, so a re-layout invalidates every one of them.
    // They are re-found here rather than remapped separately, which is what
    // keeps them in step with the scroll position.
    app.search
        .refresh(app.doc.doc(), app.doc.revision(), app.view.top);
}

/// Rebuild what the contents pane shows, and keep the cursor somewhere real.
fn sync_toc(app: &mut App) {
    let rows = app.doc.outline().len();
    app.toc.collapsed.resize(rows, false);
    app.toc.visible = app.doc.outline().visible(&app.toc.collapsed);

    // Folding a section can hide the cursor; it moves to the row that hid it
    // rather than disappearing.
    if let Some(parent) = app
        .doc
        .outline()
        .hidden_behind(app.toc.cursor, &app.toc.collapsed)
    {
        app.toc.cursor = parent;
    }
    if app.toc.cursor >= rows {
        app.toc.cursor = rows.saturating_sub(1);
    }
    app.toc.offset = toc_offset(app);
}

/// Scroll the contents pane the least amount that puts the cursor on screen.
fn toc_offset(app: &App) -> usize {
    let Some(sidebar) = app.panes.sidebar else {
        return 0;
    };
    let height = usize::from(sidebar.height);
    let Some(position) = app
        .toc
        .visible
        .iter()
        .position(|&row| row == app.toc.cursor)
    else {
        return 0;
    };
    if height == 0 {
        return 0;
    }
    let mut offset = app
        .toc
        .offset
        .min(app.toc.visible.len().saturating_sub(height));
    if position < offset {
        offset = position;
    } else if position >= offset + height {
        offset = position + 1 - height;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::Options;
    use crate::render::LayoutOptions;
    use crate::source::{Base, Source};
    use crate::theme::{Theme, ThemeVariant};
    use ratatui::layout::Rect;

    fn app_with(text: &str, height: u16) -> App {
        let mut app = App::new(
            Source::from_text(text, None, "t.md".into(), Base::Cwd),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        app.panes = crate::app::layout::compute(Rect::new(0, 0, 60, height), &app);
        let options = LayoutOptions {
            width: app.panes.content_width,
            code_line_numbers: false,
        };
        app.view.top = app.doc.ensure_rendered(options, &app.theme, 0);
        app
    }

    /// Long enough that the second heading can actually be scrolled to; a
    /// document that fits on screen is clamped to the top and would make this
    /// test pass for the wrong reason.
    fn two_sections() -> String {
        let filler: String = (1..=40).map(|n| format!("body line {n}\n\n")).collect();
        format!("# One\n\n{filler}# Two\n\n{filler}")
    }

    #[test]
    fn the_active_section_follows_the_scroll_position() {
        let text = two_sections();
        let mut app = app_with(&text, 10);
        sync(&mut app);
        assert_eq!(
            app.active_heading().map(|a| a.id.clone()).as_deref(),
            Some("one")
        );

        let second = app.doc.doc().outline[1].line;
        app.view.top = second;
        sync(&mut app);
        assert_eq!(
            app.active_heading().map(|a| a.id.clone()).as_deref(),
            Some("two")
        );
    }

    #[test]
    fn folding_a_section_moves_a_hidden_cursor_to_what_hid_it() {
        let mut app = app_with("# One\n\n## Under one\n\n# Two\n\nbody\n", 20);
        sync(&mut app);
        app.toc.cursor = 1;
        app.toc.collapsed = vec![true, false, false];
        sync(&mut app);
        assert_eq!(app.toc.cursor, 0, "the cursor was left on a hidden row");
        assert_eq!(app.toc.visible, vec![0, 2]);
    }

    #[test]
    fn the_contents_cursor_and_the_active_section_move_independently() {
        let filler: String = (1..=40).map(|n| format!("body {n}\n\n")).collect();
        let text = format!("# One\n\n{filler}# Two\n\n{filler}");
        let mut app = app_with(&text, 20);
        app.toc.cursor = 1;
        sync(&mut app);
        // Scrolling changes the active section but leaves the cursor alone.
        assert_eq!(app.active, Some(0));
        assert_eq!(app.toc.cursor, 1);
        app.view.top = app.doc.doc().outline[1].line;
        sync(&mut app);
        assert_eq!(app.active, Some(1));
        assert_eq!(app.toc.cursor, 1);
    }

    #[test]
    fn a_hidden_contents_pane_cannot_keep_focus() {
        let mut app = app_with("# One\n\n## Two\n", 20);
        app.focus = Focus::Toc;
        // The pane computed at 60 columns is present; at 40 it is not.
        app.panes = crate::app::layout::compute(Rect::new(0, 0, 40, 20), &app);
        sync(&mut app);
        assert_eq!(app.focus, Focus::Document);
    }

    #[test]
    fn a_position_past_the_end_is_pulled_back_before_drawing() {
        let mut app = app_with("# One\n\nbody\n", 10);
        app.view.top = 9_000;
        sync(&mut app);
        assert_eq!(app.view.top, app.extent().max_top());
    }
}
