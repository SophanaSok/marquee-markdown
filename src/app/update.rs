//! The only place application state is mutated.
//!
//! Keys reach this module already resolved to an [`Action`] by the keymap, so
//! nothing here matches on a key code and rebinding a key needs no change to
//! any of it.

use crossterm::event::{MouseEvent, MouseEventKind};

use super::action::Action;
use super::event::Event;
use super::state::{App, Overlay};

/// Apply one event.
// Taken by value because an event is consumed here: pasted text moves into the
// state it lands in rather than being copied out of a borrow.
#[allow(clippy::needless_pass_by_value)]
pub fn handle(app: &mut App, event: Event) {
    match event {
        Event::Key(key) => {
            // A message is a response to the last key; the next key replaces it.
            app.message = None;
            if let Some(action) = app.keymap.action(app.mode(), key) {
                apply(app, action);
            }
        }
        Event::Mouse(mouse) => mouse_event(app, mouse),
        // Resizes are handled by recomputing pane geometry before the next
        // draw, which happens for every iteration anyway.
        Event::Resize(_, _) | Event::Paste(_) => {}
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
    }
}

/// Step back out of whatever is innermost.
///
/// The ladder is explicit so that adding a layer later — a prompt, a table of
/// contents with focus — means adding a rung rather than reordering a
/// condition, and so the last rung stays a hint rather than an exit: quitting
/// on a stray escape loses the reader's place.
fn escape(app: &mut App) {
    if app.overlay.take().is_some() {
        return;
    }
    app.message = Some("press q to quit".to_owned());
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
