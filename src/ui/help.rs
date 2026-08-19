//! The key reference.
//!
//! Every row comes from the live keymap, so a rebound key documents itself and
//! the overlay cannot drift out of date the way a hand-written list would.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::app::state::App;
use crate::render::measure;

/// Draw the help overlay over the middle of the screen.
pub fn draw(frame: &mut Frame, app: &App) {
    let rows = rows(app);
    if rows.is_empty() {
        return;
    }
    let key_width = rows
        .iter()
        .map(|(keys, _)| measure::width(keys))
        .max()
        .unwrap_or(0);
    let description_width = rows
        .iter()
        .map(|(_, description)| measure::width(description))
        .max()
        .unwrap_or(0);
    let widest = key_width + 2 + description_width;

    let area = centered(
        frame.area(),
        u16::try_from(widest + 4).unwrap_or(u16::MAX),
        u16::try_from(rows.len() + 2).unwrap_or(u16::MAX),
    );
    if area.width < 4 || area.height < 3 {
        return;
    }

    let theme = &app.theme;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.overlay_border())
        .style(theme.overlay_body())
        .title(Span::styled(" keys ", theme.overlay_title()));
    let inner = block.inner(area);

    let lines: Vec<Line<'static>> = rows
        .into_iter()
        .map(|(keys, description)| {
            let pad = key_width - measure::width(&keys);
            Line::from(vec![
                Span::styled(" ".repeat(pad), theme.overlay_body()),
                Span::styled(keys, theme.overlay_key()),
                Span::styled("  ", theme.overlay_body()),
                Span::styled(description, theme.overlay_body()),
            ])
        })
        .collect();

    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines).style(theme.overlay_body()), inner);
}

/// The rows to show: the document bindings, since those are what the reader is
/// looking the overlay up for.
fn rows(app: &App) -> Vec<(String, String)> {
    app.keymap
        .help_rows(app.pane_mode())
        .into_iter()
        .map(|(keys, action)| (keys, action.describe().to_owned()))
        .collect()
}

/// A rectangle of at most `width` by `height`, centered in `area`.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::action::Action;
    use crate::app::keymap::{Keymap, Mode};
    use crate::app::state::{App, Options, Overlay};
    use crate::source::{Base, Source};
    use crate::theme::{Theme, ThemeVariant};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn app() -> App {
        let mut app = App::new(
            Source::from_text("# T\n\nbody\n", None, "t.md".into(), Base::Cwd),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        app.overlay = Some(Overlay::Help);
        crate::app::reconcile(&mut app, Rect::new(0, 0, 80, 24));
        app
    }

    fn frame_text(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        crate::app::reconcile(app, Rect::new(0, 0, width, height));
        terminal.draw(|frame| crate::ui::draw(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn the_overlay_lists_the_keys_that_are_actually_bound() {
        let text = frame_text(&mut app(), 80, 24);
        assert!(text.contains("down a line"), "{text}");
        assert!(text.contains("j down"), "{text}");
    }

    #[test]
    fn rebinding_a_key_rebinds_what_the_overlay_says() {
        let mut app = app();
        let mut keymap = Keymap::default();
        keymap
            .bind(Mode::Document, "n".parse().unwrap(), Action::LineDown)
            .unwrap();
        app.keymap = keymap;
        let text = frame_text(&mut app, 80, 24);
        assert!(text.contains("down a line"), "{text}");
        assert!(!text.contains("j down"), "stale binding shown:\n{text}");
    }

    #[test]
    fn a_terminal_too_small_for_the_overlay_still_draws() {
        for (width, height) in [(80, 24), (20, 6), (8, 3), (4, 2), (1, 1)] {
            let _ = frame_text(&mut app(), width, height);
        }
    }
}
