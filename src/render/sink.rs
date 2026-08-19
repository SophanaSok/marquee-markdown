//! The line sink: the single funnel through which every rendered line passes.
//!
//! The sink owns the width invariant — each pushed line is padded to exactly
//! the content width and checked in debug builds. It also assigns line
//! metadata, collapses runs of blank lines into the block spacing rhythm, and
//! accumulates the plain-text mirror used by search.

use std::ops::Range;

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use smallvec::SmallVec;

use super::doc::{Anchor, LineKind, LineMeta, RenderedDoc};
use super::frag::Frag;
use super::measure;

/// Accumulates rendered lines while enforcing the width invariant.
pub struct LineSink {
    width: usize,
    /// Style used to pad short lines out to the content width.
    page_fill: Style,
    lines: Vec<Line<'static>>,
    meta: Vec<LineMeta>,
    outline: Vec<Anchor>,
    links: Vec<String>,
    plain: String,
    /// Whether the last emitted line was blank (for blank collapsing).
    last_blank: bool,
}

impl LineSink {
    #[must_use]
    pub fn new(width: usize, page_fill: Style) -> Self {
        Self {
            width: width.max(1),
            page_fill,
            lines: Vec::new(),
            meta: Vec::new(),
            outline: Vec::new(),
            links: Vec::new(),
            plain: String::new(),
            last_blank: true, // suppress leading blanks
        }
    }

    /// Content width lines must fill.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Number of lines emitted so far — the index the next line will get.
    #[must_use]
    pub fn next_line_index(&self) -> usize {
        self.lines.len()
    }

    /// Intern a link destination, deduplicating exact repeats.
    pub fn intern_link(&mut self, dest: &str) -> u32 {
        if let Some(idx) = self.links.iter().position(|l| l == dest) {
            return idx as u32;
        }
        self.links.push(dest.to_owned());
        (self.links.len() - 1) as u32
    }

    /// Record a heading anchor at the next line to be emitted.
    pub fn push_anchor(&mut self, level: u8, id: String, text: String) {
        self.outline.push(Anchor {
            line: self.lines.len(),
            level,
            id,
            text,
        });
    }

    /// Emit one blank spacing line. Consecutive blanks collapse to one.
    pub fn blank(&mut self) {
        if self.last_blank {
            return;
        }
        self.push_spans(Vec::new(), LineKind::Blank, None);
        self.last_blank = true;
    }

    /// Emit a line assembled from wrapped fragments, with `indent` cells of
    /// `lead` prefix (gutter bars, list markers, quote padding).
    pub fn push_frags(
        &mut self,
        lead: Vec<Span<'static>>,
        frags: &[Frag],
        kind: LineKind,
        source: Option<Range<usize>>,
    ) {
        let lead_width: usize = lead.iter().map(|s| measure::width(&s.content)).sum();
        let mut spans = lead;
        let mut links: SmallVec<[(Range<u16>, u32); 1]> = SmallVec::new();
        let mut col = lead_width;

        // Coalesce adjacent frags with identical style and link into one span.
        let mut run = String::new();
        let mut run_style = Style::default();
        let mut run_link: Option<u32> = None;
        let mut run_start = col;
        for frag in frags {
            if frag.text.is_empty() {
                continue;
            }
            let same = !run.is_empty() && frag.style == run_style && frag.link == run_link;
            if !same {
                if !run.is_empty() {
                    if let Some(idx) = run_link {
                        links.push((run_start as u16..col as u16, idx));
                    }
                    spans.push(Span::styled(std::mem::take(&mut run), run_style));
                }
                run_style = frag.style;
                run_link = frag.link;
                run_start = col;
            }
            run.push_str(&frag.text);
            col += frag.width;
        }
        if !run.is_empty() {
            if let Some(idx) = run_link {
                links.push((run_start as u16..col as u16, idx));
            }
            spans.push(Span::styled(run, run_style));
        }

        self.push_line_with_links(spans, col, kind, source, links);
    }

    /// Emit a pre-assembled line whose spans are already styled and measured.
    /// `content_width` must be the exact display width of `spans`.
    pub fn push_spans(
        &mut self,
        spans: Vec<Span<'static>>,
        kind: LineKind,
        source: Option<Range<usize>>,
    ) {
        let width: usize = spans.iter().map(|s| measure::width(&s.content)).sum();
        self.push_line_with_links(spans, width, kind, source, SmallVec::new());
    }

    fn push_line_with_links(
        &mut self,
        mut spans: Vec<Span<'static>>,
        content_width: usize,
        kind: LineKind,
        source: Option<Range<usize>>,
        links: SmallVec<[(Range<u16>, u32); 1]>,
    ) {
        debug_assert!(
            content_width <= self.width,
            "line overflows content width: {content_width} > {} ({spans:?})",
            self.width
        );
        // Pad to exactly the content width so the page background is solid.
        if content_width < self.width {
            spans.push(Span::styled(
                " ".repeat(self.width - content_width),
                self.page_fill,
            ));
        }

        #[cfg(debug_assertions)]
        {
            let total: usize = spans.iter().map(|s| measure::width(&s.content)).sum();
            debug_assert_eq!(total, self.width, "width invariant violated");
        }

        let plain_start = self.plain.len();
        for span in &spans {
            self.plain.push_str(&span.content);
        }
        // Trim the pad from the plain mirror so search never matches padding.
        let trimmed = self.plain.trim_end_matches(' ').len();
        self.plain.truncate(trimmed.max(plain_start));
        let plain_range = plain_start..self.plain.len();
        self.plain.push('\n');

        self.lines.push(Line::from(spans));
        self.meta.push(LineMeta {
            kind,
            source,
            plain: plain_range,
            links,
        });
        self.last_blank = kind == LineKind::Blank;
    }

    /// Finish, dropping any trailing blank line.
    #[must_use]
    pub fn finish(mut self, width: u16) -> RenderedDoc {
        while self.meta.last().is_some_and(|m| m.kind == LineKind::Blank) {
            self.lines.pop();
            self.meta.pop();
        }
        RenderedDoc {
            lines: self.lines,
            meta: self.meta,
            outline: self.outline,
            links: self.links,
            plain: self.plain,
            width,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::frag::FragKind;

    fn sink() -> LineSink {
        LineSink::new(20, Style::new())
    }

    fn frag(text: &str) -> Frag {
        Frag {
            text: text.to_owned(),
            style: Style::new(),
            link: None,
            width: measure::width(text),
            kind: FragKind::Word,
        }
    }

    fn line_width(line: &Line<'_>) -> usize {
        line.spans.iter().map(|s| measure::width(&s.content)).sum()
    }

    #[test]
    fn every_line_is_padded_to_exact_width() {
        let mut s = sink();
        s.push_frags(Vec::new(), &[frag("short")], LineKind::Body, None);
        s.blank();
        s.push_frags(Vec::new(), &[frag("also short")], LineKind::Body, None);
        let doc = s.finish(20);
        for line in &doc.lines {
            assert_eq!(line_width(line), 20);
        }
    }

    #[test]
    fn consecutive_blanks_collapse() {
        let mut s = sink();
        s.push_frags(Vec::new(), &[frag("a")], LineKind::Body, None);
        s.blank();
        s.blank();
        s.blank();
        s.push_frags(Vec::new(), &[frag("b")], LineKind::Body, None);
        let doc = s.finish(20);
        assert_eq!(doc.lines.len(), 3);
    }

    #[test]
    fn leading_and_trailing_blanks_are_dropped() {
        let mut s = sink();
        s.blank();
        s.push_frags(Vec::new(), &[frag("only")], LineKind::Body, None);
        s.blank();
        let doc = s.finish(20);
        assert_eq!(doc.lines.len(), 1);
    }

    #[test]
    fn plain_mirror_excludes_padding() {
        let mut s = sink();
        s.push_frags(Vec::new(), &[frag("findme")], LineKind::Body, None);
        let doc = s.finish(20);
        assert_eq!(doc.plain, "findme\n");
    }

    #[test]
    fn link_columns_are_recorded() {
        let mut s = sink();
        let idx = s.intern_link("https://x");
        let mut f = frag("link");
        f.link = Some(idx);
        s.push_frags(vec![Span::raw(">> ")], &[f], LineKind::Body, None);
        let doc = s.finish(20);
        assert_eq!(doc.links, ["https://x"]);
        assert_eq!(doc.meta[0].links.as_slice(), &[(3u16..7u16, 0u32)]);
    }

    #[test]
    fn links_are_deduplicated() {
        let mut s = sink();
        assert_eq!(s.intern_link("https://a"), 0);
        assert_eq!(s.intern_link("https://b"), 1);
        assert_eq!(s.intern_link("https://a"), 0);
    }

    #[test]
    fn anchors_point_at_the_next_line() {
        let mut s = sink();
        s.push_frags(Vec::new(), &[frag("before")], LineKind::Body, None);
        s.push_anchor(2, "here".into(), "Here".into());
        s.push_frags(Vec::new(), &[frag("Here")], LineKind::Heading(2), None);
        let doc = s.finish(20);
        assert_eq!(doc.outline.len(), 1);
        assert_eq!(doc.outline[0].line, 1);
    }

    #[test]
    fn adjacent_same_style_frags_coalesce() {
        let mut s = sink();
        s.push_frags(
            Vec::new(),
            &[frag("one"), frag(" "), frag("two")],
            LineKind::Body,
            None,
        );
        let doc = s.finish(20);
        // one content span + one pad span
        assert_eq!(doc.lines[0].spans.len(), 2);
        assert_eq!(doc.lines[0].spans[0].content, "one two");
    }
}
