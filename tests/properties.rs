//! Property tests for the invariants the whole design rests on.
//!
//! The fixture-based tests check that the renderer handles the constructs
//! someone thought to write down. These check that it cannot be broken by
//! input nobody thought of — which is the only useful standard for a width
//! invariant, because the ways text is wider than it looks are exactly the
//! ways nobody anticipates: an emoji family joined by zero-width joiners, a
//! variation selector, a combining mark, a Nerd Font glyph in the private use
//! area that reports one cell and often draws two.
//!
//! Everything here is deterministic given its seed; a failure leaves a
//! regression file under `tests/properties.proptest-regressions`.

use marquee_markdown::doc::View;
use marquee_markdown::doc::search::{self, Search};
use marquee_markdown::doc::view::Extent;
use marquee_markdown::render::{self, Document, LayoutOptions, RenderedDoc, measure, tui};
use marquee_markdown::theme::{Theme, ThemeVariant};
use proptest::prelude::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

/// Text chosen to disagree with itself about how wide it is.
const NASTY: &[&str] = &[
    "plain",
    "日本語",    // double-width
    "한국어",    // double-width, different script
    "🎨",        // emoji
    "👩‍👩‍👧‍👦",        // ZWJ family: many code points, one cluster
    "🇯🇵",        // regional indicator pair
    "e\u{301}",  // combining acute
    "é",         // precomposed
    "a\u{fe0f}", // variation selector
    "\u{f05a}",  // Nerd Font private use area
    "\u{200b}",  // zero-width space
    "\u{0301}",  // a lone combining mark
    "ﷺ",         // one code point, wide ligature
    "\t",        // tab, expanded during fragmentation
    "very-long-unbreakable-token-that-exceeds-any-sensible-column-width",
    "מילה", // right-to-left
    "  ",
    "-",
    "#",
];

/// Wrap `text` in `depth` levels of quoting and list nesting.
///
/// Nesting is what makes the width invariant hard: every level eats columns
/// from the lead, so at a narrow terminal the room left for content shrinks
/// toward nothing — and a double-width grapheme that cannot fit in one cell
/// is the case that once looped forever.
fn nest(text: &str, depth: usize) -> String {
    let mut out = text.to_owned();
    for level in 0..depth {
        out = if level % 2 == 0 {
            format!("> {out}")
        } else {
            format!("- {out}")
        };
    }
    out
}

prop_compose! {
    /// A run of adversarial text.
    fn nasty_text(max_pieces: usize)(
        pieces in prop::collection::vec(prop::sample::select(NASTY), 1..=max_pieces)
    ) -> String {
        pieces.join(" ")
    }
}

/// One markdown block built around adversarial text, optionally nested.
fn block() -> impl Strategy<Value = String> {
    (0usize..10, nasty_text(6), 0usize..4).prop_map(|(kind, text, depth)| {
        let text = nest(&text, depth);
        match kind {
            0 => format!("# {text}"),
            1 => format!("###### {text}"),
            2 => format!("- {text}\n- {text}"),
            3 => format!("1. {text}\n2. {text}"),
            4 => format!("> {text}"),
            5 => format!("> [!WARNING]\n> {text}"),
            6 => format!("```rust\nfn x() {{ // {text}\n}}\n```"),
            7 => format!("| a | b |\n| - | - |\n| {text} | {text} |"),
            8 => format!("[{text}](https://example.com/{text}) and ![{text}](img.png)"),
            // Inline code: the chip pads glue to their content, which is the
            // one construct that can carry an anchor across a line break.
            9 => format!("a `{text}` and `{text}`"),
            _ => text,
        }
    })
}

prop_compose! {
    /// A whole document of them.
    fn document()(blocks in prop::collection::vec(block(), 1..8)) -> String {
        blocks.join("\n\n")
    }
}

fn options(width: u16, numbers: bool, preserve: bool) -> LayoutOptions {
    LayoutOptions {
        width,
        code_line_numbers: numbers,
        preserve_new_lines: preserve,
    }
}

fn render_at(text: &str, width: u16, numbers: bool, preserve: bool) -> RenderedDoc {
    render::render(
        text,
        &Theme::new(ThemeVariant::Slate),
        options(width, numbers, preserve),
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// The invariant everything else is built on: every emitted line is
    /// exactly the content width. `LineSink` asserts this in debug builds, so
    /// a violation panics here rather than silently tearing the page.
    #[test]
    fn every_line_is_exactly_the_content_width(
        text in document(),
        width in 10u16..=120,
        numbers in any::<bool>(),
        preserve in any::<bool>(),
    ) {
        let doc = render_at(&text, width, numbers, preserve);
        prop_assert_eq!(doc.width, width);
        for (index, line) in doc.lines.iter().enumerate() {
            let rendered: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            prop_assert_eq!(
                measure::width(&rendered),
                usize::from(width),
                "line {} is not {} cells: {:?}",
                index,
                width,
                rendered
            );
        }
    }

    /// The same, squeezed: the narrowest column the layout accepts, with
    /// nesting deep enough that the room left for content approaches zero.
    /// This is where a grapheme wider than the space for it has to be
    /// handled rather than looped on.
    #[test]
    fn a_squeezed_column_still_emits_exact_lines(
        text in nasty_text(4),
        depth in 0usize..8,
        width in 10u16..=16,
    ) {
        let doc = render_at(&nest(&text, depth), width, true, false);
        for line in &doc.lines {
            let rendered: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            prop_assert_eq!(measure::width(&rendered), usize::from(width), "{:?}", rendered);
        }
    }

    /// Wrapping disabled (`-w 0`) lays out at a column wider than any
    /// terminal; the invariant holds there too.
    #[test]
    fn unwrapped_output_is_exact_as_well(text in document()) {
        let width = u16::MAX / 4;
        let doc = render_at(&text, width, false, false);
        for line in doc.lines.iter().take(40) {
            let rendered: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            prop_assert_eq!(measure::width(&rendered), usize::from(width));
        }
    }

    /// Re-laying the same document out at another width changes the lines and
    /// nothing about the document: the outline keeps its headings, in order.
    #[test]
    fn the_outline_survives_any_width(
        text in document(),
        first in 10u16..=120,
        second in 10u16..=120,
    ) {
        let parsed = Document::parse(&text);
        let theme = Theme::new(ThemeVariant::Slate);
        let a = parsed.layout(&theme, options(first, false, false));
        let b = parsed.layout(&theme, options(second, false, false));

        let ids = |doc: &RenderedDoc| {
            doc.outline.iter().map(|anchor| anchor.id.clone()).collect::<Vec<_>>()
        };
        prop_assert_eq!(ids(&a), ids(&b));
        // And anchors point at lines that exist, in ascending order.
        for anchor in &a.outline {
            prop_assert!(anchor.line < a.lines.len());
        }
        prop_assert!(a.outline.windows(2).all(|p| p[0].line <= p[1].line));
    }

    /// Every line's metadata describes a real slice of the plain mirror, and
    /// the recorded lead never claims more than the line holds.
    #[test]
    fn line_metadata_stays_inside_the_document(
        text in document(),
        width in 10u16..=120,
    ) {
        let doc = render_at(&text, width, false, false);
        prop_assert_eq!(doc.lines.len(), doc.meta.len());
        for meta in &doc.meta {
            prop_assert!(doc.plain.get(meta.plain.clone()).is_some());
            let line_text = &doc.plain[meta.plain.clone()];
            // Decoration always leaves at least one cell for text — a lead
            // that filled the column was the deep-nesting overflow.
            prop_assert!(
                usize::from(meta.lead_cols) < usize::from(width),
                "lead {} fills the {} cell column: {:?}",
                meta.lead_cols,
                width,
                line_text
            );
            for (cols, target) in &meta.links {
                prop_assert!(cols.start <= cols.end);
                prop_assert!(usize::from(cols.end) <= usize::from(width));
                prop_assert!((*target as usize) < doc.links.len());
            }
        }
    }

    /// Clipping a line to a window always produces exactly that many cells,
    /// wherever the window falls — including inside a double-width cluster.
    #[test]
    fn clipping_always_produces_the_requested_width(
        text in document(),
        width in 10u16..=80,
        left in 0u16..90,
        window in 1u16..90,
    ) {
        let doc = render_at(&text, width, false, false);
        for line in doc.lines.iter().take(24) {
            let clipped = tui::clip(line, left, window, Style::default());
            let rendered: String = clipped.spans.iter().map(|s| s.content.as_ref()).collect();
            prop_assert_eq!(measure::width(&rendered), usize::from(window));
        }
    }

    /// Drawing never writes outside the buffer, whatever the pane geometry
    /// and scroll position say — a resize can leave them a frame behind.
    #[test]
    fn drawing_stays_inside_the_buffer(
        text in document(),
        doc_width in 10u16..=120,
        area_w in 1u16..60,
        area_h in 1u16..30,
        top in 0usize..500,
    ) {
        let doc = render_at(&text, doc_width, false, false);
        let area = Rect::new(0, 0, area_w, area_h);
        let mut buf = Buffer::empty(area);
        tui::render(
            &mut buf,
            Rect::new(0, 0, area_w.saturating_add(20), area_h.saturating_add(20)),
            &doc,
            top,
            0,
            Style::default(),
            &marquee_markdown::render::overlay::Plain,
        );
    }

    /// Searching arbitrary text for an arbitrary query never panics, and
    /// every hit points at a place that exists on the page.
    #[test]
    fn every_search_hit_is_a_real_place_on_the_page(
        text in document(),
        query in nasty_text(3),
        width in 10u16..=100,
    ) {
        let doc = render_at(&text, width, false, false);
        let hits = search::find(&doc, &query);
        for hit in &hits {
            prop_assert!(!hit.segments.is_empty(), "a hit with nothing visible");
            for segment in &hit.segments {
                prop_assert!(segment.line < doc.lines.len(), "line out of range");
                prop_assert!(segment.cols.start < segment.cols.end, "empty segment");
                prop_assert!(
                    usize::from(segment.cols.end) <= usize::from(width),
                    "segment past the column: {:?}",
                    segment
                );
            }
            // Segments run down the page, one line each, in order.
            prop_assert!(
                hit.segments.windows(2).all(|p| p[0].line < p[1].line),
                "segments out of order"
            );
            prop_assert_eq!(hit.first_line(), hit.segments[0].line);
        }
        // Hits come back in document order.
        prop_assert!(hits.windows(2).all(|p| p[0].first_line() <= p[1].first_line()));
    }

    /// The per-line index the highlight uses agrees with the matches it was
    /// built from, for every line of the document.
    #[test]
    fn the_highlight_index_agrees_with_the_matches(
        text in document(),
        query in nasty_text(2),
        width in 20u16..=100,
    ) {
        let doc = render_at(&text, width, false, false);
        let mut search = Search::default();
        search.search(&doc, 1, &query, 0);

        let mut counted = 0;
        for line in 0..doc.lines.len() {
            for hit in search.on_line(line) {
                prop_assert_eq!(hit.line, line, "indexed under the wrong line");
                prop_assert!(hit.of_match < search.matches().len());
                counted += 1;
            }
        }
        let expected: usize = search.matches().iter().map(|m| m.segments.len()).sum();
        prop_assert_eq!(counted, expected, "segments lost between the two views");
    }

    /// Any sequence of scrolling leaves the view inside the document.
    #[test]
    fn scrolling_never_leaves_the_document(
        lines in 0usize..2000,
        height in 1u16..80,
        doc_width in 1u16..200,
        area_width in 1u16..200,
        steps in prop::collection::vec(0u8..8, 1..40),
    ) {
        let extent = Extent { lines, height, doc_width, area_width };
        let mut view = View::default();
        for step in steps {
            match step {
                0 => view.scroll(1, extent),
                1 => view.scroll(-1, extent),
                2 => view.page(1, extent),
                3 => view.page(-1, extent),
                4 => view.half_page(1, extent),
                5 => view.to_bottom(extent),
                6 => view.go_to(lines.saturating_mul(2), extent),
                _ => view.reveal(lines / 2, extent),
            }
            prop_assert!(view.top <= extent.max_top(), "top {} past the end", view.top);
            prop_assert!(view.left <= extent.max_left());
        }
        view.to_top();
        prop_assert_eq!(view.top, 0);
    }
}
