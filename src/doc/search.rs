//! Finding text in the rendered document.
//!
//! Search runs over [`RenderedDoc::plain`], the flat mirror of the laid-out
//! lines, so a hit is already a place on screen and highlighting it costs
//! nothing at draw time. Nothing here re-lays out the document: doing so
//! would invalidate every line index the application holds, over a search.
//!
//! A phrase broken across a soft wrap matches: the lines of one wrapped
//! paragraph are joined — decoration stripped, one space per break — and
//! scanned as a whole, with a straddling hit split back into per-line
//! segments for the highlight. Lines that are not prose (code, tables,
//! headings) are still matched strictly per line, and nothing ever matches
//! across a paragraph boundary. Two consequences worth knowing: markers and
//! gutter bars (`•`, `▎`, list numerals) are decoration, not text, and do
//! not match; and a single overlong word hard-split at the column edge still
//! does not match across its split, because there is no space there to stand
//! in for.

use std::ops::Range;

use ratatui::style::Style;
use smallvec::SmallVec;

use crate::render::doc::LineKind;
use crate::render::overlay::{Overlay, Patch};
use crate::render::{RenderedDoc, measure};

/// The part of a hit that falls on one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Line index in [`RenderedDoc::lines`].
    pub line: usize,
    /// Column range within that line, in cells.
    pub cols: Range<u16>,
}

/// One hit. Usually one segment; a hit straddling a soft wrap has one per
/// line it touches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Where the hit shows, in line order. Never empty.
    pub segments: SmallVec<[Segment; 1]>,
}

impl Match {
    /// The line the hit starts on — where stepping to it scrolls.
    #[must_use]
    pub fn first_line(&self) -> usize {
        self.segments.first().map_or(0, |segment| segment.line)
    }

    /// The line the hit ends on.
    #[must_use]
    pub fn last_line(&self) -> usize {
        self.segments.last().map_or(0, |segment| segment.line)
    }
}

/// One segment in the flattened per-line index, carrying which match owns it
/// so the selected hit can be styled differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineHit {
    /// Line index in [`RenderedDoc::lines`].
    pub line: usize,
    /// Column range within that line, in cells.
    pub cols: Range<u16>,
    /// Index into [`Search::matches`].
    pub of_match: usize,
}

/// The state of an in-document search.
#[derive(Debug, Clone, Default)]
pub struct Search {
    /// The committed query — what `esc` in a prompt reverts to.
    query: String,
    matches: Vec<Match>,
    /// Every segment of every match, sorted by line then column, for the
    /// overlay's per-line lookup.
    line_hits: Vec<LineHit>,
    current: Option<usize>,
    /// The `(query, revision)` the matches were computed for. Comparing both
    /// is what lets this run every frame — including with a prompt's live
    /// input — and only do work when something actually changed.
    applied: Option<(String, u64)>,
}

impl Search {
    /// The committed query; empty when no search is active.
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

    /// Bring the matches in line with `query` at layout `revision`.
    ///
    /// Idempotent, and a strict no-op when nothing changed — it runs every
    /// frame with `from_line` at the top of the view, and re-picking the
    /// selection on a quiet frame would drag it toward the viewport after
    /// every `n`. A changed query starts the selection from the reader; a
    /// changed layout keeps it on the hit nearest where it was.
    pub fn ensure(&mut self, doc: &RenderedDoc, revision: u64, query: &str, from_line: usize) {
        if self
            .applied
            .as_ref()
            .is_some_and(|(q, r)| q == query && *r == revision)
        {
            return;
        }
        let same_query = self.applied.as_ref().is_some_and(|(q, _)| q == query);
        let anchor = if same_query {
            self.current_match().map_or(from_line, Match::first_line)
        } else {
            from_line
        };

        self.matches = find(doc, query);
        self.line_hits = flatten(&self.matches);
        self.current = self.first_at_or_after(anchor);
        self.applied = Some((query.to_owned(), revision));
    }

    /// Make `query` the committed query.
    pub fn commit(&mut self, query: &str) {
        self.query = query.to_owned();
    }

    /// Search and commit in one step: what accepting a prompt does.
    pub fn search(&mut self, doc: &RenderedDoc, revision: u64, query: &str, from_line: usize) {
        self.ensure(doc, revision, query, from_line);
        self.commit(query);
    }

    /// Forget the search entirely.
    pub fn clear(&mut self) {
        self.query.clear();
        self.matches.clear();
        self.line_hits.clear();
        self.current = None;
        self.applied = None;
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
        Some(self.matches[next].first_line())
    }

    /// The first hit starting at or after `line`, wrapping to the first hit.
    fn first_at_or_after(&self, line: usize) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let index = self.matches.partition_point(|hit| hit.first_line() < line);
        Some(if index == self.matches.len() {
            0
        } else {
            index
        })
    }

    /// The hit segments on one line, as a contiguous slice.
    #[must_use]
    pub fn on_line(&self, line: usize) -> &[LineHit] {
        let start = self.line_hits.partition_point(|hit| hit.line < line);
        let end = self.line_hits.partition_point(|hit| hit.line <= line);
        &self.line_hits[start..end]
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

/// Every segment of every match, flattened for per-line lookup.
///
/// Sorted by construction: matches come back in document order and a match's
/// segments are in line order, so the flattened list is ordered by
/// (line, column) already.
fn flatten(matches: &[Match]) -> Vec<LineHit> {
    let hits: Vec<LineHit> = matches
        .iter()
        .enumerate()
        .flat_map(|(index, hit)| {
            hit.segments.iter().map(move |segment| LineHit {
                line: segment.line,
                cols: segment.cols.clone(),
                of_match: index,
            })
        })
        .collect();
    debug_assert!(
        hits.windows(2)
            .all(|pair| (pair[0].line, pair[0].cols.start) <= (pair[1].line, pair[1].cols.start)),
        "line hits out of order"
    );
    hits
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
        for hit in self.search.on_line(line) {
            out.push(Patch {
                cols: hit.cols.clone(),
                style: if Some(hit.of_match) == self.search.current {
                    self.selected
                } else {
                    self.normal
                },
            });
        }
    }
}

/// A line's contribution to the text being scanned.
struct Piece<'doc> {
    line: usize,
    /// Content with the leading decoration stripped.
    content: &'doc str,
    /// Byte offset of `content` within the joined text of its unit.
    joined_start: usize,
    /// Display column where `content` begins on its line.
    base_col: u16,
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

    let mut index = 0;
    while index < doc.meta.len() {
        let unit = next_unit(doc, index);
        scan_unit(doc, &unit, query, sensitive, &mut matches);
        index += unit.len().max(1);
    }
    matches
}

/// The run of lines starting at `from` that scan together: one non-joinable
/// line alone, or a maximal group of wrapped-prose lines sharing a source.
fn next_unit(doc: &RenderedDoc, from: usize) -> Vec<Piece<'_>> {
    let Some(first) = piece(doc, from) else {
        return Vec::new();
    };
    let first_meta = &doc.meta[from];
    if !joinable(first_meta.kind) || first_meta.source.is_none() || first.content.is_empty() {
        return vec![first];
    }

    let mut unit = vec![first];
    let mut joined_len = unit[0].content.len();
    for line in from + 1..doc.meta.len() {
        let meta = &doc.meta[line];
        // Same paragraph: same source range, still prose, still has content.
        // An empty joinable line (a callout's bare gutter bar) breaks the
        // group, so a quote's spacing lines never bridge its paragraphs.
        if meta.kind != first_meta.kind || meta.source != first_meta.source {
            break;
        }
        let Some(mut next) = piece(doc, line) else {
            break;
        };
        if next.content.is_empty() {
            break;
        }
        joined_len += 1; // the joiner space standing in for the wrap
        next.joined_start = joined_len;
        joined_len += next.content.len();
        unit.push(next);
    }
    unit
}

/// One line's piece: its plain text with the lead stripped, and where the
/// content starts on screen.
fn piece(doc: &RenderedDoc, line: usize) -> Option<Piece<'_>> {
    let meta = doc.meta.get(line)?;
    let text = doc.plain.get(meta.plain.clone())?;
    if joinable(meta.kind) {
        // `head_width`, not `lead_cols`: a wide grapheme at the boundary can
        // leave the split one short, and the column math must agree with the
        // bytes actually stripped.
        let (_, head_width, content) = measure::split_at_col(text, usize::from(meta.lead_cols));
        Some(Piece {
            line,
            content,
            joined_start: 0,
            base_col: u16::try_from(head_width).unwrap_or(u16::MAX),
        })
    } else {
        // Not prose: matched exactly as displayed, decoration and all.
        Some(Piece {
            line,
            content: text,
            joined_start: 0,
            base_col: 0,
        })
    }
}

/// Whether lines of this kind are wrapped prose that reads across breaks.
///
/// The exclusions are all deliberate: a heading's underline shares its span
/// and kind, so joining would append the hairline to the title; HTML wraps
/// hard mid-word, so a space joiner would corrupt it; tables and code are
/// grids, not sentences.
fn joinable(kind: LineKind) -> bool {
    matches!(kind, LineKind::Body | LineKind::Quote | LineKind::List)
}

/// Scan one unit, mapping hits in its joined text back to per-line segments.
fn scan_unit(
    doc: &RenderedDoc,
    unit: &[Piece<'_>],
    query: &str,
    sensitive: bool,
    matches: &mut Vec<Match>,
) {
    let _ = doc;
    if unit.is_empty() {
        return;
    }
    let joined: String = unit
        .iter()
        .map(|piece| piece.content)
        .collect::<Vec<_>>()
        .join(" ");

    let mut offset = 0;
    while offset < joined.len() {
        let Some(length) = matches_at(&joined[offset..], query, sensitive) else {
            offset += next_char(&joined[offset..]);
            continue;
        };
        if let Some(hit) = segments_of(unit, offset, offset + length) {
            matches.push(hit);
        }
        // Hits do not overlap: resume after this one.
        offset += length.max(next_char(&joined[offset..]));
    }
}

/// The per-line segments of a hit at `[start, end)` in the unit's joined
/// text. `None` when nothing of it is visible (the hit was only the joiner).
fn segments_of(unit: &[Piece<'_>], start: usize, end: usize) -> Option<Match> {
    let mut segments: SmallVec<[Segment; 1]> = SmallVec::new();
    for piece in unit {
        let piece_end = piece.joined_start + piece.content.len();
        let from = start.max(piece.joined_start);
        let to = end.min(piece_end);
        if from >= to {
            continue; // this line, or the joiner next to it, is not touched
        }
        let within_start = from - piece.joined_start;
        let within_end = to - piece.joined_start;
        let start_col = piece.base_col + column(&piece.content[..within_start]);
        let end_col = piece.base_col + column(&piece.content[..within_end]);
        if start_col < end_col {
            segments.push(Segment {
                line: piece.line,
                cols: start_col..end_col,
            });
        }
    }
    if segments.is_empty() {
        None
    } else {
        Some(Match { segments })
    }
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

    fn doc_at(text: &str, width: u16, preserve: bool) -> RenderedDoc {
        render::render(
            text,
            &Theme::new(ThemeVariant::Slate),
            LayoutOptions {
                width,
                code_line_numbers: false,
                preserve_new_lines: preserve,
            },
        )
    }

    fn doc(text: &str) -> RenderedDoc {
        doc_at(text, 40, false)
    }

    /// The text a segment highlights, read back off the rendered line.
    fn segment_text(doc: &RenderedDoc, segment: &Segment) -> String {
        let line: String = doc.lines[segment.line]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        let (_, _, from) = measure::split_at_col(&line, usize::from(segment.cols.start));
        let (text, _, _) =
            measure::split_at_col(from, usize::from(segment.cols.end - segment.cols.start));
        text.to_owned()
    }

    /// Every hit as its highlighted text, segments joined with `|`.
    fn texts(doc: &RenderedDoc, matches: &[Match]) -> Vec<String> {
        matches
            .iter()
            .map(|hit| {
                hit.segments
                    .iter()
                    .map(|segment| segment_text(doc, segment))
                    .collect::<Vec<_>>()
                    .join("|")
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
        assert_eq!(hits[0].segments[0].cols, 0..2);
        assert_eq!(hits[1].segments[0].cols, 2..4);
    }

    #[test]
    fn a_lowercase_query_ignores_case_and_a_mixed_one_does_not() {
        let doc = doc("Rust rust RUST\n");
        assert_eq!(find(&doc, "rust").len(), 3);
        assert_eq!(texts(&doc, &find(&doc, "Rust")), vec!["Rust"]);
    }

    #[test]
    fn a_match_crosses_a_soft_wrap_in_a_paragraph() {
        // Wide enough that "sturgeon general" wraps between the words.
        let doc = doc_at(
            "Some words to push the phrase sturgeon general across a line break.\n",
            34,
            false,
        );
        let hits = find(&doc, "phrase sturgeon");
        assert_eq!(hits.len(), 1, "{:?}", texts(&doc, &hits));
        assert_eq!(hits[0].segments.len(), 2, "not split across lines");
        assert_eq!(texts(&doc, &hits), vec!["phrase|sturgeon"]);
        assert_eq!(hits[0].first_line() + 1, hits[0].last_line());
    }

    #[test]
    fn a_match_crosses_a_wrap_inside_a_list_item() {
        let doc = doc_at(
            "- a list item long enough that crossing phrase wraps onto the next line\n",
            30,
            false,
        );
        let hits = find(&doc, "that crossing");
        assert_eq!(hits.len(), 1, "{:?}", texts(&doc, &hits));
        // Both segments start past the marker / hanging indent.
        for segment in &hits[0].segments {
            assert!(segment.cols.start >= 2, "{segment:?}");
        }
        assert_eq!(texts(&doc, &hits), vec!["that|crossing"]);
    }

    #[test]
    fn a_match_crosses_a_wrap_inside_a_quote() {
        let doc = doc_at(
            "> quoted prose long enough that borderline example wraps here\n",
            30,
            false,
        );
        let hits = find(&doc, "enough that");
        assert_eq!(hits.len(), 1, "{:?}", texts(&doc, &hits));
        for segment in &hits[0].segments {
            assert!(segment.cols.start >= 2, "inside the gutter: {segment:?}");
        }
    }

    #[test]
    fn markers_and_bars_are_not_searchable() {
        // Decoration, not text — the same reason headings never match `#`.
        let doc = doc("> a quote\n\n- item one\n\n1. numbered\n");
        assert!(find(&doc, "\u{258e}").is_empty(), "the quote bar matched");
        assert!(find(&doc, "\u{2022}").is_empty(), "the bullet matched");
        assert!(find(&doc, "1.").is_empty(), "the list numeral matched");
    }

    #[test]
    fn no_match_across_paragraphs() {
        let doc = doc("First paragraph ends alpha\n\nbeta starts the second\n");
        assert!(find(&doc, "alpha beta").is_empty());
    }

    #[test]
    fn no_match_across_list_items() {
        let doc = doc("- first item alpha\n- beta second item\n");
        assert!(find(&doc, "alpha beta").is_empty());
    }

    #[test]
    fn no_match_across_table_cells_or_code_lines() {
        let table = doc("| a | b |\n| - | - |\n| alpha | beta |\n");
        assert!(
            find(&table, "alpha beta").is_empty(),
            "matched across cells"
        );
        let code = doc("```\nalpha\nbeta\n```\n");
        assert!(
            find(&code, "alpha beta").is_empty(),
            "matched across code lines"
        );
    }

    #[test]
    fn no_match_between_an_alert_head_and_its_body() {
        let doc = doc("> [!NOTE]\n> The body text of the note.\n");
        assert!(find(&doc, "Note The body").is_empty());
        // The body itself still matches.
        assert_eq!(find(&doc, "body text").len(), 1);
    }

    #[test]
    fn a_heading_is_not_joined_to_its_underline() {
        let doc = doc("# Title\n\nbody\n");
        assert!(find(&doc, "Title \u{2500}").is_empty());
        assert_eq!(find(&doc, "Title").len(), 1);
    }

    #[test]
    fn smart_case_applies_across_the_boundary() {
        // The uppercase character lands in the second line's part of the hit.
        let doc = doc_at(
            "Some words to push the phrase Sturgeon general across a line break.\n",
            34,
            false,
        );
        assert_eq!(find(&doc, "phrase sturgeon").len(), 1, "lowercase misses");
        assert_eq!(find(&doc, "phrase Sturgeon").len(), 1);
        assert!(find(&doc, "Phrase sturgeon").is_empty(), "case ignored");
    }

    #[test]
    fn columns_are_cells_not_bytes() {
        let doc = doc("\u{65e5}\u{672c}\u{8a9e} target\n");
        let hits = find(&doc, "target");
        assert_eq!(hits.len(), 1);
        // Three double-width characters and a space.
        assert_eq!(hits[0].segments[0].cols, 7..13);
        assert_eq!(texts(&doc, &hits), vec!["target"]);
    }

    #[test]
    fn cjk_before_a_cross_wrap_hit_measures_in_cells() {
        // The wide text sits on the second line, before the hit's tail.
        let doc = doc_at(
            "Padding words here so that alpha \u{65e5}\u{672c}beta lands across the wrap\n",
            32,
            false,
        );
        let hits = find(&doc, "alpha \u{65e5}\u{672c}beta");
        assert_eq!(hits.len(), 1, "{:?}", texts(&doc, &hits));
        assert_eq!(
            texts(&doc, &hits),
            vec!["alpha|\u{65e5}\u{672c}beta"],
            "columns drifted around the wide characters"
        );
    }

    #[test]
    fn a_hit_ending_at_the_join_has_no_empty_segment() {
        let doc = doc_at(
            "Some words to push the phrase sturgeon general across a line break.\n",
            34,
            false,
        );
        // Query ends exactly at the line break: one segment, on one line.
        let hits = find(&doc, "phrase ");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].segments.len(), 1, "{:?}", hits[0].segments);
        for hit in &find(&doc, "phrase sturgeon") {
            for segment in &hit.segments {
                assert!(segment.cols.start < segment.cols.end, "{segment:?}");
            }
        }
    }

    #[test]
    fn preserved_line_breaks_still_join_for_search() {
        // Under -n the author's newline renders as a break, and semantically
        // it is a space.
        let doc = doc_at("one sentence here\nanother sentence\n", 40, true);
        let hits = find(&doc, "here another");
        assert_eq!(hits.len(), 1, "{:?}", texts(&doc, &hits));
        assert_eq!(hits[0].segments.len(), 2);
    }

    #[test]
    fn an_empty_query_finds_nothing_rather_than_everything() {
        assert!(find(&doc("anything\n"), "").is_empty());
    }

    #[test]
    fn padding_is_not_searchable() {
        let doc = doc("hi\n");
        assert!(find(&doc, "  ").is_empty());
    }

    #[test]
    fn hits_come_back_in_document_order() {
        let text: String = (1..=30).map(|n| format!("line {n} needle\n\n")).collect();
        let doc = doc(&text);
        let hits = find(&doc, "needle");
        assert!(hits.len() >= 30);
        assert!(
            hits.windows(2)
                .all(|pair| pair[0].first_line() <= pair[1].first_line())
        );
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
        assert!(hit.first_line() >= 10, "jumped backwards");
    }

    #[test]
    fn a_search_with_no_hits_leaves_nothing_selected() {
        let (_, mut search) = searching("nothing here\n", "absent");
        assert!(search.is_active());
        assert_eq!(search.current(), None);
        assert!(search.matches().is_empty());
        assert_eq!(search.select_next(), None);
    }

    #[test]
    fn ensure_is_a_strict_noop_when_nothing_changed() {
        // It runs every frame with the viewport top; re-picking the selection
        // would drag it back toward the view after every `n`.
        let text: String = (1..=10).map(|n| format!("needle {n}\n\n")).collect();
        let (doc, mut search) = searching(&text, "needle");
        search.select_next();
        search.select_next();
        let picked = search.current();
        search.ensure(&doc, 1, "needle", 0);
        search.ensure(&doc, 1, "needle", 99);
        assert_eq!(search.current(), picked, "the selection drifted");
    }

    #[test]
    fn ensure_with_a_new_query_starts_from_the_reader() {
        let text: String = (1..=20).map(|n| format!("needle {n}\n\n")).collect();
        let (doc, mut search) = searching(&text, "needle");
        search.ensure(&doc, 1, "needle 1", 30);
        let hit = search.current_match().expect("a hit");
        assert!(hit.first_line() >= 30, "did not start from the reader");
        // The committed query is untouched until commit — this was a preview.
        assert_eq!(search.query(), "needle");
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
        let before = search.current_match().expect("a hit").first_line();

        let narrow = doc_at(&text, 20, false);
        search.ensure(&narrow, 2, "needle", 0);
        let after = search.current_match().expect("a hit after re-layout");
        assert!(after.first_line() >= before, "selection moved backwards");
        assert!(
            search
                .matches()
                .iter()
                .all(|hit| hit.last_line() < narrow.lines.len())
        );
    }

    #[test]
    fn hits_on_a_line_come_back_as_a_slice() {
        let (_, search) = searching("aa\n\nbb aa\n", "aa");
        let line = search.matches()[1].first_line();
        assert_eq!(search.on_line(line).len(), 1);
        assert!(search.on_line(line + 1_000).is_empty());
    }

    #[test]
    fn a_cross_wrap_hit_appears_on_both_of_its_lines() {
        let doc = doc_at(
            "Some words to push the phrase sturgeon general across a line break.\n",
            34,
            false,
        );
        let mut search = Search::default();
        search.search(&doc, 1, "phrase sturgeon", 0);
        let hit = search.current_match().expect("a hit");
        let (first, last) = (hit.first_line(), hit.last_line());
        assert_ne!(first, last);
        assert_eq!(search.on_line(first).len(), 1);
        assert_eq!(search.on_line(last).len(), 1);
        // Both carry the same owning match, so both highlight as selected.
        assert_eq!(
            search.on_line(first)[0].of_match,
            search.on_line(last)[0].of_match
        );
    }

    #[test]
    fn clearing_forgets_everything() {
        let (_, mut search) = searching("needle\n", "needle");
        search.clear();
        assert!(!search.is_active());
        assert!(search.matches().is_empty());
        assert_eq!(search.current(), None);
        assert!(search.on_line(0).is_empty());
    }
}
