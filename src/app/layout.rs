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
    /// The contents pane, when it is on show. `None` rather than a zero-width
    /// rectangle, so a widget cannot draw into a pane that is not there.
    pub sidebar: Option<Rect>,
    /// Width the document is laid out at, which may exceed `body` when
    /// wrapping is disabled.
    pub content_width: u16,
}

impl Panes {
    /// The single-row hint line above the status bar, when it is on show.
    ///
    /// Derived rather than stored, the way everything else in the reader that
    /// can be is: the rows below the document are contiguous, so the gap
    /// between where the document stops and where the status bar starts *is*
    /// the hint line — there is no second place for the two to disagree. Like
    /// `sidebar` it is `None` rather than a zero-height rectangle, so a widget
    /// cannot draw into a row that is not there.
    ///
    /// It spans the whole terminal, status-bar width rather than body width,
    /// so the line reads as a second row of the same chrome rather than as
    /// something belonging to the document column.
    #[must_use]
    pub fn hints(&self) -> Option<Rect> {
        let height = self.status.y.saturating_sub(self.body.bottom());
        (height > 0).then(|| Rect {
            y: self.body.bottom(),
            height,
            ..self.status
        })
    }

    /// Height of the terminal these panes were computed for.
    ///
    /// The rows below the document are easy to forget one of — the hint line
    /// is the second — so anything that needs the whole screen asks here
    /// rather than adding two of the three up at the call site.
    #[must_use]
    pub fn height(&self) -> u16 {
        self.body.height + self.hints().map_or(0, |row| row.height) + self.status.height
    }
}

/// Height of the status bar.
const STATUS_HEIGHT: u16 = 1;
/// Rows the document keeps for itself before the hint line may take one. A
/// line of hints over no document at all is a worse trade than no hints.
const HINTS_MIN_BODY: u16 = 1;
/// Below this the contents pane hides itself: the document column left over
/// would be too narrow to read.
const SIDEBAR_MIN_TERMINAL: u16 = 60;
/// Narrowest useful contents pane, including its divider.
const SIDEBAR_MIN: u16 = 18;
/// Widest the contents pane grows to; past this it is just white space.
const SIDEBAR_MAX: u16 = 32;

/// Work out the pane rectangles for a terminal of size `area`.
#[must_use]
pub fn compute(area: Rect, app: &App) -> Panes {
    let status_height = STATUS_HEIGHT.min(area.height);
    let below_status = area.height - status_height;
    // The row the hint line takes, if it gets one. Left as a gap between the
    // document and the status bar rather than recorded: `Panes::hints` reads
    // it back off the geometry.
    let hints_height = u16::from(shows_hints(area.width, below_status, app));
    let full = Rect {
        height: below_status - hints_height,
        ..area
    };
    let status = Rect {
        y: area.y + full.height + hints_height,
        height: status_height,
        ..area
    };

    let sidebar = sidebar_width(area.width, app).map(|width| Rect { width, ..full });
    let body = match sidebar {
        Some(pane) => Rect {
            x: full.x + pane.width,
            width: full.width - pane.width,
            ..full
        },
        None => full,
    };

    let requested = app.options.width;
    let resolved = width::resolve(requested, Some(body.width.max(width::MIN)));
    Panes {
        body,
        status,
        sidebar,
        // An explicit `-w` is honored exactly, even past the edge of the
        // screen; that is what makes horizontal scrolling meaningful.
        content_width: if requested.is_some() {
            resolved
        } else {
            resolved.min(body.width.max(width::MIN))
        },
    }
}

/// Whether the hint line gets a row of its own.
///
/// It asks what would actually be drawn rather than guessing at a minimum
/// width: the chips come from the keymap, so a reader who rebound their keys
/// to longer chords moves the threshold with them, and a terminal too narrow
/// for even the first chip spends its row on the document instead.
fn shows_hints(width: u16, below_status: u16, app: &App) -> bool {
    app.hints
        && below_status > HINTS_MIN_BODY
        && !crate::app::hints::fitting(&app.keymap, app.mode(), width).is_empty()
}

/// How wide the contents pane should be, or `None` when it should not show.
///
/// It hides itself on a narrow terminal and on a document with nothing to
/// list, so it never costs the reader room it is not earning.
fn sidebar_width(terminal: u16, app: &App) -> Option<u16> {
    // One entry is not a table of contents; it is a heading the reader can
    // already see.
    if app.screen != crate::app::state::Screen::Document
        || !app.toc_visible
        || terminal < SIDEBAR_MIN_TERMINAL
        || app.doc.heading_count() < 2
    {
        return None;
    }
    Some((terminal / 4).clamp(SIDEBAR_MIN, SIDEBAR_MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{App, Options};
    use crate::source::{Base, Source};
    use crate::theme::{Theme, ThemeVariant};

    fn app(options: Options) -> App {
        app_over("# T\n", options)
    }

    /// An app whose document has been laid out, so the outline exists and the
    /// contents pane has something to list.
    fn app_over(text: &str, options: Options) -> App {
        let source = Source::from_text(text, None, "t.md".into(), Base::Cwd);
        let mut app = App::new(source, Theme::new(ThemeVariant::Slate), options);
        crate::app::reconcile(&mut app, Rect::new(0, 0, 100, 30));
        app
    }

    fn outlined() -> App {
        app_over("# One\n\nbody\n\n## Two\n\nbody\n", Options::default())
    }

    /// The reader as it is by default, minus the hint line, so the geometry
    /// tests that predate it still say what they were written to say.
    fn without_hints(options: Options) -> App {
        let mut app = app(options);
        app.hints = false;
        app
    }

    #[test]
    fn the_status_bar_takes_one_row_off_the_bottom() {
        let panes = compute(Rect::new(0, 0, 80, 24), &without_hints(Options::default()));
        assert_eq!(panes.body, Rect::new(0, 0, 80, 23));
        assert_eq!(panes.status, Rect::new(0, 23, 80, 1));
    }

    #[test]
    fn the_hint_line_sits_between_the_document_and_the_status_bar() {
        let panes = compute(Rect::new(0, 0, 80, 24), &app(Options::default()));
        assert_eq!(panes.body, Rect::new(0, 0, 80, 22));
        assert_eq!(panes.hints(), Some(Rect::new(0, 22, 80, 1)));
        assert_eq!(panes.status, Rect::new(0, 23, 80, 1));
        assert_eq!(panes.height(), 24);
    }

    #[test]
    fn turning_the_hints_off_gives_the_row_back_to_the_document() {
        let panes = compute(Rect::new(0, 0, 80, 24), &without_hints(Options::default()));
        assert_eq!(panes.hints(), None);
        assert_eq!(panes.body.height, 23);
        assert_eq!(panes.height(), 24);
    }

    #[test]
    fn the_document_keeps_the_last_row_it_has() {
        // Two rows: one is the status bar, and the hint line will not take the
        // only one left.
        let panes = compute(Rect::new(0, 0, 80, 2), &app(Options::default()));
        assert_eq!(panes.hints(), None);
        assert_eq!(panes.body.height, 1);
        let panes = compute(Rect::new(0, 0, 80, 3), &app(Options::default()));
        assert_eq!(panes.hints().map(|row| row.height), Some(1));
        assert_eq!(panes.body.height, 1);
    }

    #[test]
    fn a_terminal_too_narrow_for_a_single_hint_spends_the_row_on_the_document() {
        let app = app(Options::default());
        for width in 0..=8 {
            let panes = compute(Rect::new(0, 0, width, 24), &app);
            assert_eq!(panes.hints(), None, "width {width}");
        }
        assert!(compute(Rect::new(0, 0, 80, 24), &app).hints().is_some());
    }

    /// The row is derived from the gap between the document and the status
    /// bar, so what matters is that the three stay contiguous at every size —
    /// a gap that is not the hint line, or a hint line that is not the gap,
    /// would both show up here.
    #[test]
    fn the_hint_line_never_leaves_the_terminal_it_was_given() {
        let app = app(Options::default());
        for height in 0..6 {
            for width in [0, 1, 12, 40, 80] {
                let area = Rect::new(0, 0, width, height);
                let panes = compute(area, &app);
                assert_eq!(panes.height(), height, "{width}x{height}");
                match panes.hints() {
                    Some(row) => {
                        assert_eq!(row.y, panes.body.bottom(), "{width}x{height}");
                        assert_eq!(row.bottom(), panes.status.y, "{width}x{height}");
                        assert_eq!(row.height, 1, "{width}x{height}");
                        // The whole terminal, not the document column: the
                        // line is chrome, and the contents pane is beside it.
                        assert_eq!(row.x, panes.status.x, "{width}x{height}");
                        assert_eq!(row.width, width, "{width}x{height}");
                    }
                    None => assert_eq!(
                        panes.body.bottom(),
                        panes.status.y,
                        "a gap at {width}x{height} that is not the hint line"
                    ),
                }
            }
        }
    }

    /// The line spans the contents pane as well as the document, which is
    /// what makes it read as a second row of the status bar.
    #[test]
    fn the_hint_line_runs_the_full_width_beside_a_contents_pane() {
        let panes = compute(Rect::new(0, 0, 100, 30), &outlined());
        let row = panes.hints().expect("a hint line");
        let sidebar = panes.sidebar.expect("a sidebar");
        assert_eq!(row.x, 0);
        assert_eq!(row.width, 100);
        assert!(row.x < sidebar.width, "the line starts after the sidebar");
        assert_eq!(row.y, sidebar.bottom(), "the sidebar stops above it");
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

    #[test]
    fn the_contents_pane_takes_its_room_from_the_document() {
        let app = outlined();
        let panes = compute(Rect::new(0, 0, 100, 30), &app);
        let sidebar = panes.sidebar.expect("a sidebar");
        assert_eq!(sidebar.x, 0);
        assert_eq!(sidebar.width, 25);
        assert_eq!(panes.body.x, 25);
        assert_eq!(panes.body.width, 75);
        assert_eq!(sidebar.height, panes.body.height);
    }

    #[test]
    fn a_narrow_terminal_hides_the_contents_pane() {
        let app = outlined();
        assert!(compute(Rect::new(0, 0, 59, 30), &app).sidebar.is_none());
        assert!(compute(Rect::new(0, 0, 60, 30), &app).sidebar.is_some());
    }

    #[test]
    fn a_document_with_nothing_to_list_hides_the_contents_pane() {
        // A pane listing one heading costs a quarter of the screen and tells
        // the reader nothing.
        assert!(
            compute(Rect::new(0, 0, 100, 30), &app(Options::default()))
                .sidebar
                .is_none()
        );
        let prose = app_over("just prose, no headings\n", Options::default());
        assert!(compute(Rect::new(0, 0, 100, 30), &prose).sidebar.is_none());
    }

    #[test]
    fn hiding_the_contents_pane_gives_the_room_back() {
        let mut app = outlined();
        app.toc_visible = false;
        let panes = compute(Rect::new(0, 0, 100, 30), &app);
        assert!(panes.sidebar.is_none());
        assert_eq!(panes.body.width, 100);
    }

    #[test]
    fn the_contents_pane_stays_between_its_bounds_at_any_terminal_width() {
        let app = outlined();
        for width in SIDEBAR_MIN_TERMINAL..400 {
            let panes = compute(Rect::new(0, 0, width, 30), &app);
            let sidebar = panes.sidebar.expect("a sidebar").width;
            assert!(
                (SIDEBAR_MIN..=SIDEBAR_MAX).contains(&sidebar),
                "{sidebar} columns at terminal width {width}"
            );
            assert_eq!(sidebar + panes.body.width, width);
        }
    }
}
