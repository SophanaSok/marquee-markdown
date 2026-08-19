//! Where the reader is looking: a scroll position and the rules that keep it
//! inside the document.
//!
//! Pure arithmetic over indices, with no reference to a terminal or a theme,
//! so the whole of it is testable without drawing anything.

/// The dimensions a scroll position has to stay inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Extent {
    /// Total number of rendered lines.
    pub lines: usize,
    /// Visible rows.
    pub height: u16,
    /// Width of the rendered content column.
    pub doc_width: u16,
    /// Width available to show it in.
    pub area_width: u16,
}

impl Extent {
    /// The largest first-visible line that still fills the viewport.
    #[must_use]
    pub fn max_top(self) -> usize {
        self.lines.saturating_sub(usize::from(self.height))
    }

    /// The largest horizontal offset; zero unless the content is wider than
    /// the space available, which only happens with wrapping disabled.
    #[must_use]
    pub fn max_left(self) -> u16 {
        self.doc_width.saturating_sub(self.area_width)
    }
}

/// The visible window onto a rendered document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct View {
    /// Index of the first visible line.
    pub top: usize,
    /// First visible column of the content column.
    pub left: u16,
}

impl View {
    /// Scroll by `delta` lines, clamped to the document.
    pub fn scroll(&mut self, delta: isize, extent: Extent) {
        self.step(delta, delta.unsigned_abs(), extent);
    }

    /// Scroll by a whole viewport, keeping one line of context so the reader
    /// has something to pick the thread back up from.
    pub fn page(&mut self, direction: isize, extent: Extent) {
        let step = usize::from(extent.height).saturating_sub(1).max(1);
        self.step(direction, step, extent);
    }

    /// Scroll by half a viewport.
    pub fn half_page(&mut self, direction: isize, extent: Extent) {
        let step = (usize::from(extent.height) / 2).max(1);
        self.step(direction, step, extent);
    }

    /// Move `amount` lines in the direction `direction` points.
    fn step(&mut self, direction: isize, amount: usize, extent: Extent) {
        self.top = if direction >= 0 {
            self.top.saturating_add(amount)
        } else {
            self.top.saturating_sub(amount)
        };
        self.clamp(extent);
    }

    /// Jump to the first line.
    pub fn to_top(&mut self) {
        self.top = 0;
    }

    /// Jump to the last screenful.
    pub fn to_bottom(&mut self, extent: Extent) {
        self.top = extent.max_top();
    }

    /// Shift the view sideways by `delta` columns.
    pub fn pan(&mut self, delta: i16, extent: Extent) {
        self.left = self.left.saturating_add_signed(delta);
        self.clamp(extent);
    }

    /// Put `line` at the top of the view.
    pub fn go_to(&mut self, line: usize, extent: Extent) {
        self.top = line;
        self.clamp(extent);
    }

    /// Bring `line` into view, leaving the position alone if it already is.
    ///
    /// A line that has to be scrolled to lands a third of the way down rather
    /// than at the very edge, so there is context above it — which is what you
    /// want when the line is a search hit.
    pub fn reveal(&mut self, line: usize, extent: Extent) {
        let height = usize::from(extent.height).max(1);
        if (self.top..self.top + height).contains(&line) {
            return;
        }
        self.top = line.saturating_sub(height / 3);
        self.clamp(extent);
    }

    /// Pull the position back inside the document.
    ///
    /// Called after every mutation and once per frame, so a resize or a
    /// re-render can never leave the view pointing past the end.
    pub fn clamp(&mut self, extent: Extent) {
        self.top = self.top.min(extent.max_top());
        self.left = self.left.min(extent.max_left());
    }

    /// How far through the document the bottom of the view is, as a percentage.
    #[must_use]
    pub fn progress(self, extent: Extent) -> u16 {
        let max = extent.max_top();
        if max == 0 {
            return 100;
        }
        // Saturates rather than overflowing on a document of millions of lines.
        let ratio = (self.top as f64 / max as f64) * 100.0;
        ratio.round().clamp(0.0, 100.0) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extent(lines: usize, height: u16) -> Extent {
        Extent {
            lines,
            height,
            doc_width: 80,
            area_width: 80,
        }
    }

    #[test]
    fn scrolling_stops_at_the_last_screenful() {
        let e = extent(100, 20);
        let mut view = View::default();
        view.scroll(1_000, e);
        assert_eq!(view.top, 80);
        assert_eq!(view.progress(e), 100);
    }

    #[test]
    fn scrolling_stops_at_the_first_line() {
        let mut view = View { top: 3, left: 0 };
        view.scroll(-100, extent(100, 20));
        assert_eq!(view.top, 0);
    }

    #[test]
    fn a_document_shorter_than_the_screen_never_scrolls() {
        let e = extent(5, 20);
        let mut view = View::default();
        view.scroll(10, e);
        view.page(1, e);
        view.to_bottom(e);
        assert_eq!(view.top, 0);
        assert_eq!(view.progress(e), 100);
    }

    #[test]
    fn a_page_keeps_one_line_of_context() {
        let e = extent(100, 20);
        let mut view = View::default();
        view.page(1, e);
        assert_eq!(view.top, 19);
        view.page(-1, e);
        assert_eq!(view.top, 0);
    }

    #[test]
    fn a_half_page_is_half_the_height() {
        let e = extent(100, 20);
        let mut view = View::default();
        view.half_page(1, e);
        assert_eq!(view.top, 10);
    }

    #[test]
    fn paging_a_single_row_viewport_still_moves() {
        let e = extent(100, 1);
        let mut view = View::default();
        view.page(1, e);
        view.half_page(1, e);
        assert_eq!(view.top, 2);
    }

    #[test]
    fn panning_is_bounded_by_the_overhang() {
        let e = Extent {
            lines: 10,
            height: 5,
            doc_width: 200,
            area_width: 80,
        };
        let mut view = View::default();
        view.pan(-1, e);
        assert_eq!(view.left, 0, "cannot pan left of the first column");
        view.pan(1_000, e);
        assert_eq!(view.left, 120);
    }

    #[test]
    fn wrapped_content_cannot_be_panned_at_all() {
        let e = extent(10, 5);
        let mut view = View::default();
        view.pan(20, e);
        assert_eq!(view.left, 0);
    }

    #[test]
    fn clamping_rescues_a_position_left_over_from_a_larger_document() {
        let mut view = View { top: 900, left: 40 };
        view.clamp(extent(100, 20));
        assert_eq!(view, View { top: 80, left: 0 });
    }

    #[test]
    fn progress_reads_zero_at_the_top_of_a_long_document() {
        assert_eq!(View::default().progress(extent(1_000, 20)), 0);
    }

    #[test]
    fn revealing_a_line_already_on_screen_does_not_move_the_view() {
        let e = extent(100, 20);
        let mut view = View { top: 30, left: 0 };
        view.reveal(35, e);
        assert_eq!(view.top, 30);
        view.reveal(30, e);
        assert_eq!(view.top, 30);
        view.reveal(49, e);
        assert_eq!(view.top, 30);
    }

    #[test]
    fn revealing_a_line_off_screen_leaves_context_above_it() {
        let e = extent(100, 30);
        let mut view = View::default();
        view.reveal(60, e);
        assert!(view.top < 60, "the hit landed at the very top");
        assert!((view.top..view.top + 30).contains(&60), "not revealed");
    }

    #[test]
    fn revealing_a_line_near_the_start_does_not_underflow() {
        let mut view = View { top: 50, left: 0 };
        view.reveal(1, extent(100, 20));
        assert_eq!(view.top, 0);
    }

    #[test]
    fn going_to_a_line_past_the_end_lands_on_the_last_screenful() {
        let e = extent(100, 20);
        let mut view = View::default();
        view.go_to(9_000, e);
        assert_eq!(view.top, e.max_top());
    }
}
