//! Finding text in the rendered document.
//!
//! Search runs over [`RenderedDoc::plain`], the flat mirror of the laid-out
//! lines, so a hit is already a line and a column range and highlighting it
//! costs nothing at draw time. Nothing here re-lays out the document: doing so
//! would invalidate every line index the application holds, over a search.
//!
//! The consequence of searching the *rendered* text is that a phrase broken
//! across a soft wrap will not match, since on screen it genuinely is two
//! lines. That is the same thing the reader sees, which is the useful
//! behavior for a highlight.

use std::ops::Range;

use ratatui::style::Style;

use crate::render::overlay::{Overlay, Patch};
use crate::render::{RenderedDoc, measure};

/// One hit, as a place on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Line index in [`RenderedDoc::lines`].
    pub line: usize,
    /// Column range within that line, in cells.
    pub cols: Range<u16>,
}

/// The state of an in-document search.
#[derive(Debug, Clone, Default)]
pub struct Search {
    query: String,
    matches: Vec<Match>,
    current: Option<usize>,
    /// Layout revision the matches were found against. Line indices only mean
    /// anything for one layout, so this is what makes staleness detectable
    /// rather than a subtle wrong-highlight bug.
    revision: u64,
}

impl Search {
    /// The query being searched for; empty when no search is active.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Whether a search is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.query.is_empty()
    }

    /// Every hit, in document order.
    #[must_use]
    pub fn matches(&self) -> &[Match] {
        &self.matches
    }

    /// Which hit is selected, as a 0-based index.
    #[must_use]
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    /// The selected hit.
    #[must_use]
    pub fn current_match(&self) -> Option<&Match> {
        self.current.and_then(|index| self.matches.get(index))
    }

    /// Start searching for `query`, selecting the first hit at or after
    /// `from_line`, and return it.
    pub fn search(&mut self, doc: &RenderedDoc, revision: u64, query: &str, from_line: usize) {
        self.query = query.to_owned();
        self.matches = find(doc, query);
        self.revision = revision;
        self.current = self.first_at_or_after(from_line);
    }

    /// Forget the search entirely.
    pub fn clear(&mut self) {
        self.query.clear();
        self.matches.clear();
        self.current = None;
    }

    /// Re-find the hits if the document has been laid out again since.
    ///
    /// Line indices belong to one layout, so a resize or a theme switch
    /// invalidates every match. Re-finding is a single scan of the plain
    /// mirror and keeps the selection on the hit nearest where the reader was.
    pub fn refresh(&mut self, doc: &RenderedDoc, revision: u64, from_line: usize) {
        if revision == self.revision || !self.is_active() {
            self.revision = revision;
            return;
        }
        let previous = self
            .current_match()
            .map(|hit| hit.line)
            .unwrap_or(from_line);
        self.matches = find(doc, &self.query);
        self.revision = revision;
        self.current = self.first_at_or_after(previous);
    }

    /// Select the next hit, wrapping around. Returns the line to scroll to.
    pub fn select_next(&mut self) -> Option<usize> {
        self.step(1)
    }

    /// Select the previous hit, wrapping around.
    pub fn select_previous(&mut self) -> Option<usize> {
        self.step(-1)
    }

    fn step(&mut self, direction: isize) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let count = self.matches.len();
        let current = self.current.unwrap_or(0);
        let next = if direction >= 0 {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        self.current = Some(next);
        Some(self.matches[next].line)
    }

    /// The first hit at or after `line`, wrapping to the first hit overall.
    fn first_at_or_after(&self, line: usize) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let index = self.matches.partition_point(|hit| hit.line < line);
        Some(if index == self.matches.len() {
            0
        } else {
            index
        })
    }

    /// The hits on one line, as a contiguous slice.
    #[must_use]
    pub fn on_line(&self, line: usize) -> &[Match] {
        let start = self.matches.partition_point(|hit| hit.line < line);
        let end = self.matches.partition_point(|hit| hit.line <= line);
        &self.matches[start..end]
    }

    /// An overlay that highlights the hits, with the selected one picked out.
    #[must_use]
    pub fn overlay(&self, normal: Style, selected: Style) -> SearchOverlay<'_> {
        SearchOverlay {
            search: self,
            normal,
            selected,
        }
    }
}

/// Highlights search hits on their way to the buffer.
#[derive(Debug, Clone, Copy)]
pub struct SearchOverlay<'a> {
    search: &'a Search,
    normal: Style,
    selected: Style,
}

impl Overlay for SearchOverlay<'_> {
    fn patches(&self, line: usize, out: &mut Vec<Patch>) {
        let selected = self.search.current_match();
        for hit in self.search.on_line(line) {
            out.push(Patch {
                cols: hit.cols.clone(),
                style: if Some(hit) == selected {
                    self.selected
                } else {
                    self.normal
                },
            });
        }
    }
}

/// Every occurrence of `query` in the rendered text.
///
/// Case-insensitive unless the query contains an uppercase letter, the
/// convention a reader coming from `vim` or `less` will expect.
#[must_use]
pub fn find(doc: &RenderedDoc, query: &str) -> Vec<Match> {
    if query.is_empty() {
        return Vec::new();
    }
    let sensitive = query.chars().any(char::is_uppercase);
    let mut matches = Vec::new();

    for (line, meta) in doc.meta.iter().enumerate() {
        let Some(text) = doc.plain.get(meta.plain.clone()) else {
            continue;
        };
        let mut offset = 0;
        while offset < text.len() {
            let Some(length) = matches_at(&text[offset..], query, sensitive) else {
                offset += next_char(&text[offset..]);
                continue;
            };
            let start = column(&text[..offset]);
            let end = start.saturating_add(column(&text[offset..offset + length]));
            matches.push(Match {
                line,
                cols: start..end,
            });
            // Hits do not overlap: resume after this one.
            offset += length.max(next_char(&text[offset..]));
        }
    }
    matches
}

/// The byte length of the match starting at the front of `text`, if there is
/// one.
fn matches_at(text: &str, query: &str, sensitive: bool) -> Option<usize> {
    let mut haystack = text.chars();
    let mut needle = query.chars();
    let mut used = 0;
    loop {
        match (haystack.next(), needle.next()) {
            (_, None) => return Some(used),
            (None, Some(_)) => return None,
            (Some(here), Some(wanted)) => {
                let same = if sensitive {
                    here == wanted
                } else {
                    here == wanted || here.to_lowercase().eq(wanted.to_lowercase())
                };
                if !same {
                    return None;
                }
                used += here.len_utf8();
            }
        }
    }
}

/// Byte length of the first character, or 1 for an empty string so a scan
/// always makes progress.
fn next_char(text: &str) -> usize {
    text.chars().next().map_or(1, char::len_utf8)
}

/// Display width as a column count.
fn column(text: &str) -> u16 {
    u16::try_from(measure::width(text)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{self, LayoutOptions};
    use crate::theme::{Theme, ThemeVariant};

    fn doc(text: &str) -> RenderedDoc {
        render::render(
            text,
            &Theme::new(ThemeVariant::Slate),
            LayoutOptions {
                width: 40,
                code_line_numbers: false,
                preserve_new_lines: false,
            },
        )
    }

    fn texts(doc: &RenderedDoc, matches: &[Match]) -> Vec<String> {
        matches
            .iter()
            .map(|hit| {
                let line: String = doc.lines[hit.line]
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect();
                let (_, _, from) = measure::split_at_col(&line, usize::from(hit.cols.start));
                let (text, _, _) =
                    measure::split_at_col(from, usize::from(hit.cols.end - hit.cols.start));
                text.to_owned()
            })
            .collect()
    }

    #[test]
    fn a_hit_points_at_the_text_that_matched() {
        let doc = doc("The quick brown fox\n");
        let hits = find(&doc, "brown");
        assert_eq!(texts(&doc, &hits), vec!["brown"]);
    }

    #[test]
    fn every_occurrence_on_a_line_is_found() {
        let doc = doc("aa bb aa bb aa\n");
        assert_eq!(find(&doc, "aa").len(), 3);
    }

    #[test]
    fn overlapping_starts_do_not_produce_overlapping_hits() {
        let doc = doc("aaaa\n");
        let hits = find(&doc, "aa");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].cols, 0..2);
        assert_eq!(hits[1].cols, 2..4);
    }

    #[test]
    fn a_lowercase_query_ignores_case_and_a_mixed_one_does_not() {
        let doc = doc("Rust rust RUST\n");
        assert_eq!(find(&doc, "rust").len(), 3);
        assert_eq!(texts(&doc, &find(&doc, "Rust")), vec!["Rust"]);
    }

    #[test]
    fn columns_are_cells_not_bytes() {
        let doc = doc("日本語 target\n");
        let hits = find(&doc, "target");
        assert_eq!(hits.len(), 1);
        // Three double-width characters and a space.
        assert_eq!(hits[0].cols, 7..13);
        assert_eq!(texts(&doc, &hits), vec!["target"]);
    }

    #[test]
    fn a_wide_query_measures_in_cells_too() {
        let doc = doc("x 日本語 y\n");
        let hits = find(&doc, "日本");
        assert_eq!(hits[0].cols, 2..6);
    }

    #[test]
    fn an_empty_query_finds_nothing_rather_than_everything() {
        assert!(find(&doc("anything\n"), "").is_empty());
    }

    #[test]
    fn padding_is_not_searchable() {
        // Lines are padded out to the content width; matching that padding
        // would put a highlight on empty space.
        let doc = doc("hi\n");
        assert!(find(&doc, "  ").is_empty());
    }

    #[test]
    fn hits_come_back_in_document_order() {
        let text: String = (1..=30).map(|n| format!("line {n} needle\n\n")).collect();
        let doc = doc(&text);
        let hits = find(&doc, "needle");
        assert!(hits.len() >= 30);
        assert!(hits.windows(2).all(|pair| pair[0].line <= pair[1].line));
    }

    fn searching(text: &str, query: &str) -> (RenderedDoc, Search) {
        let doc = doc(text);
        let mut search = Search::default();
        search.search(&doc, 1, query, 0);
        (doc, search)
    }

    #[test]
    fn stepping_forward_and_back_wraps_around() {
        let text: String = (1..=4).map(|n| format!("needle {n}\n\n")).collect();
        let (_, mut search) = searching(&text, "needle");
        assert_eq!(search.current(), Some(0));
        search.select_next();
        assert_eq!(search.current(), Some(1));
        search.select_previous();
        search.select_previous();
        assert_eq!(search.current(), Some(3), "did not wrap backwards");
        search.select_next();
        assert_eq!(search.current(), Some(0), "did not wrap forwards");
    }

    #[test]
    fn a_search_starts_from_where_the_reader_is() {
        let text: String = (1..=20).map(|n| format!("needle {n}\n\n")).collect();
        let doc = doc(&text);
        let mut search = Search::default();
        search.search(&doc, 1, "needle", 10);
        let hit = search.current_match().expect("a hit");
        assert!(hit.line >= 10, "jumped backwards to line {}", hit.line);
    }

    #[test]
    fn a_search_with_no_hits_leaves_nothing_selected() {
        let (_, search) = searching("nothing here\n", "absent");
        assert!(search.is_active());
        assert_eq!(search.current(), None);
        assert!(search.matches().is_empty());
        // Stepping is a no-op rather than a panic.
        let mut search = search;
        assert_eq!(search.select_next(), None);
    }

    #[test]
    fn re_laying_out_re_finds_the_hits_and_keeps_the_selection_nearby() {
        let text: String = (1..=30)
            .map(|n| format!("needle {n} with some prose to wrap\n\n"))
            .collect();
        let doc = doc(&text);
        let mut search = Search::default();
        search.search(&doc, 1, "needle", 0);
        search.select_next();
        search.select_next();
        let before = search.current_match().expect("a hit").line;

        let narrow = render::render(
            &text,
            &Theme::new(ThemeVariant::Slate),
            LayoutOptions {
                width: 20,
                code_line_numbers: false,
                preserve_new_lines: false,
            },
        );
        search.refresh(&narrow, 2, 0);
        let after = search.current_match().expect("a hit after re-layout");
        assert!(
            after.line >= before,
            "selection moved backwards: {before} -> {}",
            after.line
        );
        // And the hits point at real text again, not at stale line numbers.
        assert!(
            search
                .matches()
                .iter()
                .all(|hit| hit.line < narrow.lines.len())
        );
    }

    #[test]
    fn refreshing_without_a_new_layout_does_nothing() {
        let text = "needle\n";
        let (doc, mut search) = searching(text, "needle");
        let before = search.matches().to_vec();
        search.refresh(&doc, 1, 0);
        assert_eq!(search.matches(), before.as_slice());
    }

    #[test]
    fn hits_on_a_line_come_back_as_a_slice() {
        let (_, search) = searching("aa\n\nbb aa\n", "aa");
        let line = search.matches()[1].line;
        assert_eq!(search.on_line(line).len(), 1);
        assert!(search.on_line(line + 1_000).is_empty());
    }

    #[test]
    fn clearing_forgets_everything() {
        let (_, mut search) = searching("needle\n", "needle");
        search.clear();
        assert!(!search.is_active());
        assert!(search.matches().is_empty());
        assert_eq!(search.current(), None);
    }
}
