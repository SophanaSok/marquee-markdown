//! Pane geometry.
//!
//! Pure: pane rectangles are computed from the terminal size and application
//! state once per iteration, before drawing, so widgets never have to work out
//! where they are and drawing never has to mutate anything to find out.

use ratatui::layout::Rect;

use super::state::App;
use crate::util::width;

/// Where each part of the screen goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Panes {
    /// The document.
    pub body: Rect,
    /// The single-row status bar.
    pub status: Rect,
    /// Width the document is laid out at, which may exceed `body` when
    /// wrapping is disabled.
    pub content_width: u16,
}

/// Height of the status bar.
const STATUS_HEIGHT: u16 = 1;

/// Work out the pane rectangles for a terminal of size `area`.
#[must_use]
pub fn compute(area: Rect, app: &App) -> Panes {
    let status_height = STATUS_HEIGHT.min(area.height);
    let body = Rect {
        height: area.height - status_height,
        ..area
    };
    let status = Rect {
        y: area.y + body.height,
        height: status_height,
        ..area
    };
    let requested = app.options.width;
    let resolved = width::resolve(requested, Some(body.width.max(width::MIN)));
    Panes {
        body,
        status,
        // An explicit `-w` is honored exactly, even past the edge of the
        // screen; that is what makes horizontal scrolling meaningful.
        content_width: if requested.is_some() {
            resolved
        } else {
            resolved.min(body.width.max(width::MIN))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{App, Options};
    use crate::source::{Base, Source};
    use crate::theme::{Theme, ThemeVariant};

    fn app(options: Options) -> App {
        let source = Source::from_text("# T\n", None, "t.md".into(), Base::Cwd);
        App::new(source, Theme::new(ThemeVariant::Slate), options)
    }

    #[test]
    fn the_status_bar_takes_one_row_off_the_bottom() {
        let panes = compute(Rect::new(0, 0, 80, 24), &app(Options::default()));
        assert_eq!(panes.body, Rect::new(0, 0, 80, 23));
        assert_eq!(panes.status, Rect::new(0, 23, 80, 1));
    }

    #[test]
    fn a_one_row_terminal_is_all_status_bar_and_no_body() {
        let panes = compute(Rect::new(0, 0, 80, 1), &app(Options::default()));
        assert_eq!(panes.body.height, 0);
        assert_eq!(panes.status.height, 1);
    }

    #[test]
    fn a_zero_height_terminal_does_not_underflow() {
        let panes = compute(Rect::new(0, 0, 80, 0), &app(Options::default()));
        assert_eq!(panes.body.height, 0);
        assert_eq!(panes.status.height, 0);
    }

    #[test]
    fn content_fills_a_narrow_terminal_and_is_capped_on_a_wide_one() {
        let narrow = compute(Rect::new(0, 0, 60, 24), &app(Options::default()));
        assert_eq!(narrow.content_width, 60);
        let wide = compute(Rect::new(0, 0, 300, 24), &app(Options::default()));
        assert_eq!(wide.content_width, width::AUTO_MAX);
    }

    #[test]
    fn an_explicit_width_is_kept_even_when_it_overflows_the_screen() {
        let options = Options {
            width: Some(200),
            ..Options::default()
        };
        let panes = compute(Rect::new(0, 0, 80, 24), &app(options));
        assert_eq!(panes.content_width, 200);
    }

    #[test]
    fn disabling_wrapping_produces_a_column_wider_than_any_screen() {
        let options = Options {
            width: Some(0),
            ..Options::default()
        };
        let panes = compute(Rect::new(0, 0, 80, 24), &app(options));
        assert!(panes.content_width > 80);
    }
}
