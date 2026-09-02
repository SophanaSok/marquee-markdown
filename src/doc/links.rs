//! Stepping through the links in a document.
//!
//! The renderer already records where every link sits — which line, which
//! columns, and which entry in the document's link table — so this is a
//! selection over data that exists rather than a second pass over the text.

use std::ops::Range;

use ratatui::style::Style;

use crate::render::RenderedDoc;
use crate::render::overlay::{Overlay, Patch};

/// One link, as a place on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Line index in [`RenderedDoc::lines`].
    pub line: usize,
    /// Column range within that line, in cells.
    pub cols: Range<u16>,
    /// Index into [`RenderedDoc::links`].
    pub target: u32,
}

/// The links in the open document, and which one the reader has stepped to.
#[derive(Debug, Clone, Default)]
pub struct Links {
    entries: Vec<Link>,
    selected: Option<usize>,
    /// Layout revision the entries were collected at.
    revision: u64,
    /// Whether a collection has happened at all. A flag rather than testing
    /// `entries` for emptiness: a document with no links is also empty, and
    /// mistaking it for "never collected" would re-scan every line of it on
    /// every frame.
    collected: bool,
}

impl Links {
    /// Collect the links again if the document has been laid out since.
    ///
    /// Re-laying out moves every link to a different line, but not to a
    /// different place in the document, so the selection survives by index.
    pub fn refresh(&mut self, doc: &RenderedDoc, revision: u64) {
        if self.collected && revision == self.revision {
            return;
        }
        self.entries = collect(doc);
        self.revision = revision;
        self.collected = true;
        self.selected = self.selected.filter(|&index| index < self.entries.len());
    }

    /// Every link, in document order.
    #[must_use]
    pub fn entries(&self) -> &[Link] {
        &self.entries
    }

    /// The link the reader has stepped to.
    #[must_use]
    pub fn selected(&self) -> Option<&Link> {
        self.selected.and_then(|index| self.entries.get(index))
    }

    /// Where the selected link points.
    #[must_use]
    pub fn selected_url<'doc>(&self, doc: &'doc RenderedDoc) -> Option<&'doc str> {
        let link = self.selected()?;
        doc.links.get(link.target as usize).map(String::as_str)
    }

    /// Which link is selected, as a 0-based index.
    #[must_use]
    pub fn position(&self) -> Option<usize> {
        self.selected
    }

    /// Forget the selection.
    pub fn clear(&mut self) {
        self.selected = None;
    }

    /// Step to the next or previous link, and report the line to bring into
    /// view.
    ///
    /// With nothing selected, stepping forward starts at the first link at or
    /// after `from_line` — the reader's own position, rather than the top of a
    /// document they may be a long way down.
    pub fn step(&mut self, direction: isize, from_line: usize) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }
        let count = self.entries.len();
        let next = match self.selected {
            Some(current) if direction >= 0 => (current + 1) % count,
            Some(current) => (current + count - 1) % count,
            None if direction >= 0 => self
                .entries
                .partition_point(|link| link.line < from_line)
                .checked_rem(count)
                .unwrap_or(0),
            None => self
                .entries
                .partition_point(|link| link.line < from_line)
                .checked_sub(1)
                .unwrap_or(count - 1),
        };
        self.selected = Some(next);
        Some(self.entries[next].line)
    }

    /// The links on one line, as a contiguous slice.
    #[must_use]
    pub fn on_line(&self, line: usize) -> &[Link] {
        let start = self.entries.partition_point(|link| link.line < line);
        let end = self.entries.partition_point(|link| link.line <= line);
        &self.entries[start..end]
    }

    /// An overlay picking out the selected link.
    #[must_use]
    pub fn overlay(&self, style: Style) -> LinkOverlay<'_> {
        LinkOverlay { links: self, style }
    }
}

/// Collect every link in a document, in order.
fn collect(doc: &RenderedDoc) -> Vec<Link> {
    doc.meta
        .iter()
        .enumerate()
        .flat_map(|(line, meta)| {
            meta.links.iter().map(move |(cols, target)| Link {
                line,
                cols: cols.clone(),
                target: *target,
            })
        })
        .collect()
}

/// Highlights the selected link on its way to the buffer.
#[derive(Debug, Clone, Copy)]
pub struct LinkOverlay<'a> {
    links: &'a Links,
    style: Style,
}

impl Overlay for LinkOverlay<'_> {
    fn patches(&self, line: usize, out: &mut Vec<Patch>) {
        if let Some(link) = self.links.selected().filter(|link| link.line == line) {
            out.push(Patch {
                cols: link.cols.clone(),
                style: self.style,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{self, LayoutOptions};
    use crate::theme::{Theme, ThemeVariant};

    fn doc(text: &str, width: u16) -> RenderedDoc {
        render::render(
            text,
            &Theme::new(ThemeVariant::Slate),
            LayoutOptions {
                width,
                code_line_numbers: false,
                preserve_new_lines: false,
            },
        )
    }

    fn document() -> String {
        (1..=20)
            .map(|n| format!("Paragraph {n} with [link {n}](https://example.com/{n}).\n\n"))
            .collect()
    }

    fn links_over(doc: &RenderedDoc) -> Links {
        let mut links = Links::default();
        links.refresh(doc, 1);
        links
    }

    #[test]
    fn every_link_in_the_document_is_collected_in_order() {
        let doc = doc(&document(), 60);
        let links = links_over(&doc);
        assert_eq!(links.entries().len(), 20);
        assert!(
            links
                .entries()
                .windows(2)
                .all(|pair| pair[0].line <= pair[1].line)
        );
    }

    #[test]
    fn a_link_points_at_a_url() {
        let doc = doc("See [the guide](https://example.com/guide).\n", 60);
        let mut links = links_over(&doc);
        links.step(1, 0);
        assert_eq!(links.selected_url(&doc), Some("https://example.com/guide"));
    }

    #[test]
    fn stepping_wraps_in_both_directions() {
        let doc = doc(&document(), 60);
        let mut links = links_over(&doc);
        links.step(1, 0);
        assert_eq!(links.position(), Some(0));
        links.step(-1, 0);
        assert_eq!(links.position(), Some(19), "did not wrap backwards");
        links.step(1, 0);
        assert_eq!(links.position(), Some(0), "did not wrap forwards");
    }

    #[test]
    fn stepping_starts_from_where_the_reader_is() {
        let doc = doc(&document(), 60);
        let mut links = links_over(&doc);
        let midway = links.entries()[10].line;
        let line = links.step(1, midway).expect("a link");
        assert!(line >= midway, "jumped backwards to line {line}");
    }

    #[test]
    fn a_document_with_no_links_steps_nowhere() {
        let doc = doc("Just prose, no links at all.\n", 60);
        let mut links = links_over(&doc);
        assert_eq!(links.step(1, 0), None);
        assert!(links.selected().is_none());
        assert!(links.selected_url(&doc).is_none());
    }

    #[test]
    fn a_document_with_no_links_is_not_rescanned_every_frame() {
        let doc = doc("Just prose, no links at all.\n", 60);
        let mut links = Links::default();
        links.refresh(&doc, 1);
        // A sentinel a re-collection would erase. Refreshing at the same
        // revision must return before touching the entries; this runs once
        // per frame, and a link-free document must not pay a full scan each
        // time.
        links.entries.push(Link {
            line: 0,
            cols: 0..1,
            target: 0,
        });
        links.refresh(&doc, 1);
        assert_eq!(links.entries.len(), 1, "an unchanged layout was re-scanned");
        // A new layout revision collects for real.
        links.refresh(&doc, 2);
        assert!(links.entries.is_empty());
    }

    #[test]
    fn re_laying_out_keeps_the_selection_on_the_same_link() {
        let text = document();
        let wide = doc(&text, 80);
        let mut links = links_over(&wide);
        links.step(1, 0);
        links.step(1, 0);
        let url = links.selected_url(&wide).expect("a url").to_owned();

        let narrow = doc(&text, 30);
        links.refresh(&narrow, 2);
        assert_eq!(links.selected_url(&narrow), Some(url.as_str()));
        // And the line it points at is a line of the new layout.
        assert!(links.selected().unwrap().line < narrow.lines.len());
    }

    #[test]
    fn only_the_selected_link_is_highlighted() {
        let doc = doc("A [one](https://a) and [two](https://b) here.\n", 60);
        let mut links = links_over(&doc);
        links.step(1, 0);
        let overlay = links.overlay(Style::new());
        let mut patches = Vec::new();
        overlay.patches(links.selected().unwrap().line, &mut patches);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].cols, links.selected().unwrap().cols);
    }

    #[test]
    fn links_on_a_line_come_back_as_a_slice() {
        let doc = doc("A [one](https://a) and [two](https://b) here.\n", 60);
        let links = links_over(&doc);
        let line = links.entries()[0].line;
        assert_eq!(links.on_line(line).len(), 2);
        assert!(links.on_line(line + 1_000).is_empty());
    }
}
