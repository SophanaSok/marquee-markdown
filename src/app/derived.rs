//! State that is recomputed rather than tracked.
//!
//! Runs once per iteration, after the update and before drawing. Everything
//! here is a function of the document, the view, and the pane sizes; putting
//! it in one place is what keeps drawing free of mutation.

use super::state::{App, Focus, PromptKind};

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
    sync_browser(app);
    sync_help(app);

    // Matches are line indices, so a re-layout invalidates every one of them.
    // They are re-found here rather than remapped separately, which is what
    // keeps them in step with the scroll position. A search prompt's live
    // input shadows the committed query — the same shape as the browser
    // filter — so the highlight narrows as the reader types, and cancelling
    // the prompt reverts on the next frame with no code of its own.
    let query = match &app.prompt {
        Some(prompt) if prompt.kind == PromptKind::Search => prompt.input.clone(),
        _ => app.search.query().to_owned(),
    };
    app.search
        .ensure(app.doc.doc(), app.doc.revision(), &query, app.view.top);
    app.links.refresh(app.doc.doc(), app.doc.revision());
}

/// Re-filter the file list and keep its cursor on screen.
///
/// The filter runs from here rather than from the keystroke that changed it,
/// so a query being typed and a query already committed take the same path —
/// filtering is idempotent and only does work when something actually changed.
fn sync_browser(app: &mut App) {
    let query = match &app.prompt {
        Some(prompt) if prompt.kind == PromptKind::Filter => prompt.input.clone(),
        _ => app
            .browser
            .as_ref()
            .map(|browser| browser.filter.clone())
            .unwrap_or_default(),
    };
    let height = app.panes.body.height;
    if let Some(browser) = app.browser.as_mut() {
        browser.refresh(&query);
        browser.clamp(height);
    }
}

/// Keep the key reference's scroll inside its rows.
///
/// The offset is only moved in `update`; the clamp lives here because it
/// needs the terminal height and the row count, both of which can change
/// under an open overlay (a resize, or focus moving between panes).
fn sync_help(app: &mut App) {
    if app.overlay != Some(crate::app::state::Overlay::Help) {
        return;
    }
    let rows = app.keymap.help_rows(app.pane_mode()).len();
    let terminal = app.panes.height();
    let visible = usize::from(terminal.saturating_sub(2)).min(rows);
    let max = rows.saturating_sub(visible);
    app.help_scroll = app.help_scroll.min(u16::try_from(max).unwrap_or(u16::MAX));
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
            preserve_new_lines: false,
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
