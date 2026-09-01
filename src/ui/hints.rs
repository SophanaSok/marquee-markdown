//! The hint line: the handful of keys worth knowing, above the status bar.
//!
//! What it says comes from [`app::hints`](crate::app::hints), which reads the
//! live keymap — a hint naming a key the reader rebound would be worse than no
//! hint. What it looks like comes from the theme's status colors rather than
//! from a palette entry of its own, so the row reads as a second line of the
//! same chrome in all ten shipped themes and in `--style system`, and a
//! hand-written theme needs nothing new to support it.
//!
//! Like the status bar, the line is composed to exactly the width it is given.
//! A chip is drawn whole or not at all, and the row is padded to the edge, so
//! nothing wraps, nothing overflows, and every cell carries a background.

use ratatui::Frame;
use ratatui::text::{Line, Span};

use crate::app::hints::{INDENT, SEPARATOR};
use crate::app::state::App;
use crate::render::{measure, tui};

/// Draw the hint line, if the terminal had a row to spare for it.
pub fn draw(frame: &mut Frame, app: &App) {
    let Some(area) = app.panes.hints else {
        return;
    };
    if area.is_empty() {
        return;
    }
    let line = compose(app, area.width);
    tui::paint(frame.buffer_mut(), area, app.theme.status_bar());
    frame
        .buffer_mut()
        .set_line(area.x, area.y, &line, area.width);
}

/// Build the hint line, exactly `width` cells wide.
///
/// Separated from drawing the way the status bar's composition is, so which
/// hints survive a narrow terminal can be tested without a terminal.
#[must_use]
pub fn compose(app: &App, width: u16) -> Line<'static> {
    let theme = &app.theme;
    let chips = crate::app::hints::fitting(&app.keymap, app.mode(), width);
    if chips.is_empty() {
        // Not even one hint fits. The row is still the row, so it is painted
        // and left blank rather than half-written — and pane geometry has
        // usually given the line back to the document by this point anyway.
        return Line::from(Span::styled(
            " ".repeat(usize::from(width)),
            theme.status_bar(),
        ));
    }
    let mut spans = vec![Span::styled(INDENT, theme.status_bar())];
    let mut used = measure::width(INDENT);
    for (index, chip) in chips.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(SEPARATOR, theme.status_bar()));
            used += measure::width(SEPARATOR);
        }
        used += chip.width();
        spans.push(Span::styled(chip.keys.clone(), theme.status_message()));
        spans.push(Span::styled(format!(" {}", chip.label), theme.status_bar()));
    }
    // `fitting` promised to stay inside the width, so this is padding rather
    // than a clamp — but it is a saturating one, because a line wider than the
    // row it is set into is the defect this whole module has to not have.
    let gap = usize::from(width).saturating_sub(used);
    spans.push(Span::styled(" ".repeat(gap), theme.status_bar()));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::keymap::Mode;
    use crate::app::state::{Options, Overlay, Prompt, PromptKind};
    use crate::source::{Base, Source};
    use crate::theme::{Theme, ThemeVariant};
    use ratatui::layout::Rect;

    fn app() -> App {
        let mut app = App::new(
            Source::from_text(
                "# One\n\nbody\n\n## Two\n\nbody\n",
                None,
                "t.md".into(),
                Base::Cwd,
            ),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        crate::app::reconcile(&mut app, Rect::new(0, 0, 80, 24));
        app
    }

    fn text_of(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn the_line_is_exactly_the_width_it_was_asked_for() {
        let app = app();
        for width in 0..120u16 {
            assert_eq!(
                measure::width(&text_of(&compose(&app, width))),
                usize::from(width),
                "width {width}"
            );
        }
    }

    #[test]
    fn it_names_the_keys_a_reader_needs_first() {
        let text = text_of(&compose(&app(), 80));
        for expected in ["j/k scroll", "/ search", "t contents", "? help", "q quit"] {
            assert!(text.contains(expected), "{expected} missing from {text:?}");
        }
    }

    #[test]
    fn a_narrow_terminal_drops_hints_from_the_end_rather_than_wrapping() {
        let app = app();
        let wide = text_of(&compose(&app, 100));
        let narrow = text_of(&compose(&app, 30));
        assert!(narrow.contains("j/k scroll"), "{narrow:?}");
        assert!(!narrow.contains("q quit"), "{narrow:?}");
        assert!(wide.contains("q quit"), "{wide:?}");
    }

    #[test]
    fn rebinding_a_key_rebinds_the_hint() {
        let mut app = app();
        app.keymap.rebind(
            Mode::Document,
            "n".parse().unwrap(),
            crate::app::action::Action::LineDown,
        );
        for chord in ["j", "down"] {
            app.keymap.unbind(Mode::Document, chord.parse().unwrap());
        }
        let text = text_of(&compose(&app, 80));
        assert!(text.contains("n/k scroll"), "{text:?}");
        assert!(!text.contains("j/k"), "stale binding shown: {text:?}");
    }

    #[test]
    fn the_hints_follow_the_mode_in_force() {
        let mut app = app();
        app.prompt = Some(Prompt {
            kind: PromptKind::Search,
            input: String::new(),
        });
        let text = text_of(&compose(&app, 80));
        assert!(text.contains("enter accept"), "{text:?}");
        assert!(text.contains("esc cancel"), "{text:?}");
        assert!(
            !text.contains("q quit"),
            "a prompt's `q` is a letter: {text:?}"
        );

        app.prompt = None;
        app.overlay = Some(Overlay::Themes);
        let text = text_of(&compose(&app, 80));
        assert!(text.contains("preview"), "{text:?}");
    }

    #[test]
    fn the_keys_are_picked_out_from_what_they_do() {
        let app = app();
        let line = compose(&app, 80);
        let keys = line
            .spans
            .iter()
            .find(|span| span.content.as_ref() == "j/k")
            .expect("a scroll hint");
        assert_eq!(keys.style, app.theme.status_message());
        // And every span sits on the status bar's own background, so the row
        // reads as chrome rather than as a stripe of something else.
        for span in &line.spans {
            assert_eq!(span.style.bg, app.theme.status_bar().bg);
        }
    }
}
