//! The only place application state is mutated.
//!
//! Keys reach this module already resolved to an [`Action`] by the keymap, so
//! nothing here matches on a key code and rebinding a key needs no change to
//! any of it.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use super::action::Action;
use super::event::Event;
use super::state::{App, Focus, Overlay, Prompt, PromptKind, Screen};

/// Apply one event.
pub fn handle(app: &mut App, event: Event) {
    match event {
        Event::Key(key) => {
            // A message answers the last key; the next key replaces it.
            app.message = None;
            match app.keymap.action(app.mode(), key) {
                Some(action) => apply(app, action),
                // Anything a prompt has not bound is text being typed into it.
                // This is what keeps `q` in a search box from quitting.
                None if app.prompt.is_some() => type_into_prompt(app, key),
                None => {}
            }
        }
        Event::Paste(text) => paste(app, text),
        Event::Mouse(mouse) => mouse_event(app, mouse),
        Event::Scan(scan) => scan_reported(app, scan),
        // Resizes are handled by recomputing pane geometry before the next
        // draw, which happens for every iteration anyway.
        Event::Resize(_, _) => {}
    }
}

/// Apply one action.
pub fn apply(app: &mut App, action: Action) {
    let extent = app.extent();
    match action {
        Action::Quit => app.should_quit = true,
        Action::Escape => escape(app),
        Action::ToggleHelp => {
            app.overlay = match app.overlay {
                Some(Overlay::Help) => None,
                None => Some(Overlay::Help),
            }
        }
        Action::ToggleTheme => {
            std::mem::swap(&mut app.theme, &mut app.alternate);
            // The layout cache notices the change on the next reconcile and
            // re-lays out the document, keeping the reading position.
        }
        Action::LineDown => app.view.scroll(1, extent),
        Action::LineUp => app.view.scroll(-1, extent),
        Action::HalfPageDown => app.view.half_page(1, extent),
        Action::HalfPageUp => app.view.half_page(-1, extent),
        Action::PageDown => app.view.page(1, extent),
        Action::PageUp => app.view.page(-1, extent),
        Action::Top => app.view.to_top(),
        Action::Bottom => app.view.to_bottom(extent),
        Action::ScrollLeft => app.view.pan(-1, extent),
        Action::ScrollRight => app.view.pan(1, extent),

        Action::ToggleToc => app.toc_visible = !app.toc_visible,
        Action::FocusNext => focus_next(app),
        Action::TocDown => move_cursor(app, 1),
        Action::TocUp => move_cursor(app, -1),
        Action::TocTop => set_cursor(app, app.toc.visible.first().copied()),
        Action::TocBottom => set_cursor(app, app.toc.visible.last().copied()),
        Action::TocCollapse => collapse(app),
        Action::TocExpand => expand(app),
        Action::TocOpen => open_selected(app),

        Action::SearchStart => {
            app.prompt = Some(Prompt {
                kind: PromptKind::Search,
                input: String::new(),
            });
        }
        Action::SearchNext => step_search(app, 1),
        Action::SearchPrevious => step_search(app, -1),
        Action::PromptAccept => accept_prompt(app),
        Action::PromptBackspace => backspace(app),
        Action::PromptClear => {
            if let Some(prompt) = app.prompt.as_mut() {
                prompt.input.clear();
            }
        }

        Action::BrowserDown => with_browser(app, |browser| browser.move_cursor(1)),
        Action::BrowserUp => with_browser(app, |browser| browser.move_cursor(-1)),
        Action::BrowserPageDown => browser_page(app, 1),
        Action::BrowserPageUp => browser_page(app, -1),
        Action::BrowserTop => with_browser(app, crate::browser::Browser::to_first),
        Action::BrowserBottom => with_browser(app, crate::browser::Browser::to_last),
        Action::BrowserOpen => open_selected_file(app),
        Action::FilterStart => {
            app.prompt = Some(Prompt {
                kind: PromptKind::Filter,
                // Filtering is incremental, so the prompt starts from what is
                // already in force rather than throwing it away.
                input: app
                    .browser
                    .as_ref()
                    .map(|browser| browser.filter.clone())
                    .unwrap_or_default(),
            });
        }
    }
}

/// Do something to the browser, if there is one.
fn with_browser(app: &mut App, change: impl FnOnce(&mut crate::browser::Browser)) {
    if let Some(browser) = app.browser.as_mut() {
        change(browser);
    }
}

/// Move a whole screen through the file list, which is what these keys do in
/// glow's browser even though the same keys move half a screen in its pager.
fn browser_page(app: &mut App, direction: isize) {
    let step = isize::try_from(app.panes.body.height.max(1)).unwrap_or(1);
    with_browser(app, |browser| browser.move_cursor(direction * step));
}

/// Read the selected file.
fn open_selected_file(app: &mut App) {
    let Some(path) = app
        .browser
        .as_ref()
        .and_then(|browser| browser.selected())
        .map(|entry| entry.path.clone())
    else {
        app.message = Some("nothing to open".to_owned());
        return;
    };
    // A browser only ever offers local files, so nothing here reaches the
    // network; the fetcher is inert until something asks it for a URL.
    let fetcher = crate::source::HttpFetcher::new();
    match crate::source::resolve(&crate::source::SourceSpec::File(path.clone()), &fetcher) {
        Ok(source) => app.read(source),
        // A file that vanished mid-scan, or one that is not readable, is not
        // worth ending the session over.
        Err(error) => app.message = Some(format!("cannot open {}: {error}", path.display())),
    }
}

/// Take a batch of results from the directory walk.
fn scan_reported(app: &mut App, scan: crate::browser::Scan) {
    let Some(browser) = app.browser.as_mut() else {
        return;
    };
    match scan {
        crate::browser::Scan::Found(entries) => browser.extend(entries),
        crate::browser::Scan::Done => browser.scanning = false,
    }
}

/// Step back out of whatever is innermost.
///
/// The ladder is explicit so that adding a layer later means adding a rung
/// rather than reordering a condition, and so the last rung stays a hint
/// rather than an exit: quitting on a stray escape loses the reader's place.
fn escape(app: &mut App) {
    if app.overlay.take().is_some() {
        return;
    }
    if app.prompt.take().is_some() {
        return;
    }
    if app.focus != Focus::Document {
        app.focus = Focus::Document;
        return;
    }
    if app.search.is_active() {
        app.search.clear();
        return;
    }
    if let Some(browser) = app.browser.as_mut() {
        if !browser.filter.is_empty() {
            browser.filter.clear();
            return;
        }
        if app.screen == Screen::Document {
            app.screen = Screen::Browser;
            return;
        }
    }
    app.message = Some("press q to quit".to_owned());
}

/// Move focus between the document and the contents pane.
fn focus_next(app: &mut App) {
    if app.panes.sidebar.is_none() {
        app.message = Some("the contents pane is hidden; press t to show it".to_owned());
        return;
    }
    app.focus = match app.focus {
        Focus::Document => Focus::Toc,
        Focus::Toc => Focus::Document,
    };
}

/// Move the contents cursor `delta` entries through the rows on show, so a
/// folded section is stepped over rather than into.
fn move_cursor(app: &mut App, delta: isize) {
    let Some(position) = app
        .toc
        .visible
        .iter()
        .position(|&row| row == app.toc.cursor)
    else {
        set_cursor(app, app.toc.visible.first().copied());
        return;
    };
    let next = position
        .saturating_add_signed(delta)
        .min(app.toc.visible.len().saturating_sub(1));
    set_cursor(app, app.toc.visible.get(next).copied());
}

fn set_cursor(app: &mut App, row: Option<usize>) {
    if let Some(row) = row {
        app.toc.cursor = row;
    }
}

/// Fold the selected section, or step out to its parent when there is nothing
/// to fold — the behavior every tree view has, and the reason `h` means this
/// here and something else in the document.
fn collapse(app: &mut App) {
    let cursor = app.toc.cursor;
    let foldable = app
        .doc
        .outline()
        .rows()
        .get(cursor)
        .is_some_and(|row| row.has_children() && !is_collapsed(app, cursor));
    if foldable {
        set_collapsed(app, cursor, true);
    } else if let Some(parent) = app.doc.outline().parent(cursor) {
        app.toc.cursor = parent;
    }
}

/// Unfold the selected section, or step into it when it is already open.
fn expand(app: &mut App) {
    let cursor = app.toc.cursor;
    if is_collapsed(app, cursor) {
        set_collapsed(app, cursor, false);
    } else if let Some(first) = app
        .doc
        .outline()
        .rows()
        .get(cursor)
        .filter(|row| row.has_children())
        .map(|row| row.subtree.start)
    {
        app.toc.cursor = first;
    }
}

fn is_collapsed(app: &App, row: usize) -> bool {
    app.toc.collapsed.get(row).copied().unwrap_or(false)
}

fn set_collapsed(app: &mut App, row: usize, collapsed: bool) {
    let rows = app.doc.outline().len();
    app.toc.collapsed.resize(rows, false);
    if let Some(flag) = app.toc.collapsed.get_mut(row) {
        *flag = collapsed;
    }
}

/// Jump the document to the selected entry and hand focus back to it, which is
/// what choosing an entry is for.
fn open_selected(app: &mut App) {
    let Some(line) = app.anchor_of(app.toc.cursor).map(|anchor| anchor.line) else {
        return;
    };
    let extent = app.extent();
    app.view.go_to(line, extent);
    app.focus = Focus::Document;
}

/// Go to the next or previous hit, bringing it into view.
fn step_search(app: &mut App, direction: isize) {
    if !app.search.is_active() {
        app.message = Some("press / to search".to_owned());
        return;
    }
    let line = if direction >= 0 {
        app.search.select_next()
    } else {
        app.search.select_previous()
    };
    match line {
        Some(line) => {
            let extent = app.extent();
            app.view.reveal(line, extent);
        }
        None => app.message = Some(format!("no match for `{}`", app.search.query())),
    }
}

/// Run what was typed at the prompt.
fn accept_prompt(app: &mut App) {
    let Some(prompt) = app.prompt.take() else {
        return;
    };
    match prompt.kind {
        PromptKind::Filter => {
            if let Some(browser) = app.browser.as_mut() {
                browser.filter = prompt.input;
            }
        }
        PromptKind::Search => {
            if prompt.input.is_empty() {
                app.search.clear();
                return;
            }
            app.search.search(
                app.doc.doc(),
                app.doc.revision(),
                &prompt.input,
                app.view.top,
            );
            match app.search.current_match().map(|hit| hit.line) {
                Some(line) => {
                    let extent = app.extent();
                    app.view.reveal(line, extent);
                }
                None => app.message = Some(format!("no match for `{}`", prompt.input)),
            }
        }
    }
}

/// Delete the last character, cancelling the prompt when there is nothing left
/// — backspacing out of an empty prompt is how a reader expects to leave it.
fn backspace(app: &mut App) {
    let Some(prompt) = app.prompt.as_mut() else {
        return;
    };
    if prompt.input.pop().is_none() {
        app.prompt = None;
    }
}

/// Add a typed character to the open prompt.
///
/// Only unmodified characters: a chord the prompt has not bound is not text.
fn type_into_prompt(app: &mut App, key: KeyEvent) {
    let printable = key.modifiers - KeyModifiers::SHIFT == KeyModifiers::NONE;
    if let (KeyCode::Char(c), true) = (key.code, printable)
        && let Some(prompt) = app.prompt.as_mut()
    {
        prompt.input.push(c);
    }
}

/// Pasted text goes into an open prompt and is ignored otherwise.
fn paste(app: &mut App, mut text: String) {
    let Some(prompt) = app.prompt.as_mut() else {
        return;
    };
    // A paste carrying newlines would otherwise submit itself line by line.
    text.retain(|c| !c.is_control());
    prompt.input.push_str(&text);
}

/// Mouse wheel scrolling, when `-m` asked for it.
fn mouse_event(app: &mut App, mouse: MouseEvent) {
    if !app.options.mouse {
        return;
    }
    let extent = app.extent();
    match mouse.kind {
        MouseEventKind::ScrollDown => app.view.scroll(3, extent),
        MouseEventKind::ScrollUp => app.view.scroll(-3, extent),
        MouseEventKind::ScrollLeft => app.view.pan(-3, extent),
        MouseEventKind::ScrollRight => app.view.pan(3, extent),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::Options;
    use crate::source::{Base, Source};
    use crate::theme::{Theme, ThemeVariant};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn app() -> App {
        let text: String = (1..=200).map(|n| format!("line {n}\n\n")).collect();
        let mut app = App::new(
            Source::from_text(&text, None, "t.md".into(), Base::Cwd),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        crate::app::reconcile(&mut app, ratatui::layout::Rect::new(0, 0, 60, 24));
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        handle(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    #[test]
    fn quitting_sets_the_flag_rather_than_exiting() {
        let mut app = app();
        press(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn help_opens_and_closes_and_changes_the_mode_with_it() {
        let mut app = app();
        press(&mut app, KeyCode::Char('?'));
        assert_eq!(app.overlay, Some(Overlay::Help));
        assert_eq!(app.mode(), crate::app::keymap::Mode::Help);
        press(&mut app, KeyCode::Char('?'));
        assert_eq!(app.overlay, None);
    }

    #[test]
    fn q_closes_the_help_overlay_instead_of_quitting() {
        let mut app = app();
        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Char('q'));
        assert_eq!(app.overlay, None);
        assert!(!app.should_quit, "help swallowed the reader's document");
    }

    #[test]
    fn scrolling_keys_do_nothing_while_help_is_open() {
        let mut app = app();
        press(&mut app, KeyCode::Char('j'));
        let top = app.view.top;
        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('G'));
        assert_eq!(app.view.top, top);
    }

    #[test]
    fn escape_with_nothing_open_hints_rather_than_quitting() {
        let mut app = app();
        press(&mut app, KeyCode::Esc);
        assert!(!app.should_quit);
        assert!(app.message.is_some());
        // The next key clears it again.
        press(&mut app, KeyCode::Char('j'));
        assert!(app.message.is_none());
    }

    #[test]
    fn toggling_the_theme_swaps_both_ways() {
        let mut app = app();
        assert_eq!(app.theme.name, "slate");
        press(&mut app, KeyCode::Char('T'));
        assert_eq!(app.theme.name, "paper");
        press(&mut app, KeyCode::Char('T'));
        assert_eq!(app.theme.name, "slate");
    }

    #[test]
    fn the_wheel_is_ignored_unless_mouse_support_was_asked_for() {
        let mut app = app();
        let wheel = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        handle(&mut app, Event::Mouse(wheel));
        assert_eq!(app.view.top, 0);

        app.options.mouse = true;
        handle(&mut app, Event::Mouse(wheel));
        assert_eq!(app.view.top, 3);
    }

    #[test]
    fn an_unbound_key_is_simply_ignored() {
        let untouched = app().summary();
        let mut app = app();
        press(&mut app, KeyCode::Char('z'));
        assert_eq!(app.summary(), untouched);
    }
}
