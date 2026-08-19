//! End-to-end invariants of the rendering engine.
//!
//! These run over the kitchen-sink fixture, which exercises every construct the
//! renderer handles, at many widths. The width invariant in particular is the
//! foundation the centered reading column and the sealed code cards rest on, so
//! it is checked exhaustively rather than by example.

use marquee_markdown::render::{self, LayoutOptions};
use marquee_markdown::theme::{Theme, ThemeVariant};

const FIXTURE: &str = include_str!("fixtures/kitchen-sink.md");

fn opts(width: u16) -> LayoutOptions {
    LayoutOptions {
        width,
        code_line_numbers: false,
    }
}

#[test]
fn every_line_is_exactly_the_content_width_at_every_width() {
    for variant in ThemeVariant::all() {
        let theme = Theme::new(variant);
        for width in [10u16, 20, 32, 40, 55, 72, 80, 100, 120, 200] {
            let doc = render::render(FIXTURE, &theme, opts(width));
            for (i, line) in doc.lines.iter().enumerate() {
                assert_eq!(
                    line.width(),
                    usize::from(width),
                    "{variant} width {width}: line {i} is {} cells\n{:?}",
                    line.width(),
                    line
                );
            }
        }
    }
}

#[test]
fn code_lines_never_escape_their_container() {
    // The glow bug this project exists to fix: a long line inside a fence must
    // stay bounded by the card, at every width.
    let theme = Theme::new(ThemeVariant::Slate);
    for width in [24u16, 40, 60, 80] {
        let doc = render::render(FIXTURE, &theme, opts(width));
        for (i, meta) in doc.meta.iter().enumerate() {
            if matches!(meta.kind, marquee_markdown::render::LineKind::Code { .. }) {
                let text: String = doc.lines[i]
                    .spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect();
                assert!(
                    text.starts_with('│') && text.trim_end().ends_with('│'),
                    "width {width}: code line {i} broke its container: {text:?}"
                );
            }
        }
    }
}

#[test]
fn outline_is_sorted_and_covers_every_heading() {
    let theme = Theme::new(ThemeVariant::Paper);
    let doc = render::render(FIXTURE, &theme, opts(80));
    // Compare against what the parser found, not a re-implementation of it:
    // the fixture's unstripped YAML frontmatter legitimately produces a setext
    // heading, and frontmatter stripping is a separate concern.
    let parsed_headings = count_headings(&marquee_markdown::render::parse::parse(FIXTURE));
    assert_eq!(doc.outline.len(), parsed_headings);
    assert!(
        doc.outline.windows(2).all(|w| w[0].line < w[1].line),
        "outline not strictly ordered by line"
    );
    for anchor in &doc.outline {
        assert!(
            anchor.line < doc.lines.len(),
            "anchor points past the buffer"
        );
    }
}

#[test]
fn headings_render_without_hash_marks() {
    let theme = Theme::new(ThemeVariant::Paper);
    let doc = render::render(FIXTURE, &theme, opts(80));
    for anchor in &doc.outline {
        let text: String = doc.lines[anchor.line]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(!text.trim_start().starts_with('#'), "hash leaked: {text:?}");
        assert!(
            text.contains(&anchor.text),
            "heading text missing: {text:?}"
        );
    }
}

#[test]
fn anchor_ids_are_unique() {
    let theme = Theme::new(ThemeVariant::Paper);
    let doc = render::render(FIXTURE, &theme, opts(80));
    let mut seen = std::collections::HashSet::new();
    for anchor in &doc.outline {
        assert!(
            seen.insert(&anchor.id),
            "duplicate anchor id: {}",
            anchor.id
        );
    }
}

#[test]
fn plain_mirror_lines_up_with_the_line_buffer() {
    let theme = Theme::new(ThemeVariant::Slate);
    let doc = render::render(FIXTURE, &theme, opts(72));
    for (i, meta) in doc.meta.iter().enumerate() {
        let slice = &doc.plain[meta.plain.clone()];
        let rendered: String = doc.lines[i]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(
            slice,
            rendered.trim_end(),
            "line {i} plain mirror disagrees with the rendered line"
        );
    }
}

#[test]
fn active_anchor_never_points_past_the_outline() {
    let theme = Theme::new(ThemeVariant::Paper);
    let doc = render::render(FIXTURE, &theme, opts(80));
    for top in 0..doc.lines.len() {
        if let Some(idx) = doc.active_anchor(top) {
            assert!(idx < doc.outline.len());
            assert!(doc.outline[idx].line <= top);
        }
    }
}

#[test]
fn every_span_paints_a_background() {
    // A span without a background punches a hole in the painted page.
    let theme = Theme::new(ThemeVariant::Slate);
    let doc = render::render(FIXTURE, &theme, opts(80));
    for (i, line) in doc.lines.iter().enumerate() {
        for span in &line.spans {
            assert!(
                span.style.bg.is_some(),
                "line {i} has an unpainted span: {span:?}"
            );
        }
    }
}

#[test]
fn empty_and_whitespace_documents_do_not_panic() {
    let theme = Theme::new(ThemeVariant::Paper);
    for src in ["", "\n", "   \n\n  \n", "#", "```\n"] {
        let doc = render::render(src, &theme, opts(40));
        for line in &doc.lines {
            assert_eq!(line.width(), 40);
        }
    }
}

#[test]
fn narrow_widths_degrade_without_panicking() {
    let theme = Theme::new(ThemeVariant::Slate);
    for width in 10u16..24 {
        let doc = render::render(FIXTURE, &theme, opts(width));
        for line in &doc.lines {
            assert_eq!(line.width(), usize::from(width));
        }
    }
}

#[test]
fn tables_frame_when_words_fit_and_stack_as_cards_when_they_do_not() {
    let theme = Theme::new(ThemeVariant::Slate);
    // Four prose columns: comfortable at 80 cells, hopeless at 30.
    let source = concat!(
        "| Left | Center | Right | Notes |\n",
        "|------|--------|-------|-------|\n",
        "| a | b | c | a much longer cell here |\n",
    );

    let wide = render_text(source, &theme, 80);
    assert!(
        wide.contains('\u{250c}'),
        "wide table should be framed:\n{wide}"
    );
    assert!(
        wide.contains('\u{2502}'),
        "wide table missing verticals:\n{wide}"
    );

    let narrow = render_text(source, &theme, 30);
    assert!(
        !narrow.contains('\u{250c}'),
        "narrow table should fall back to cards:\n{narrow}"
    );
    assert!(narrow.contains("Left:"), "card labels missing:\n{narrow}");
    // The data must survive the fallback, not be dropped.
    assert!(narrow.contains("longer"), "card content missing:\n{narrow}");
}

#[test]
fn a_table_that_fits_is_never_stretched_to_the_full_column() {
    // A narrow table centred in a wide column should keep its natural size,
    // not smear across the page.
    let theme = Theme::new(ThemeVariant::Paper);
    let out = render_text("| Single |\n|--------|\n| column |\n", &theme, 80);
    let top = out.lines().find(|l| l.contains('\u{250c}')).expect("frame");
    assert!(
        top.trim_end().chars().count() < 20,
        "table stretched to fill the column: {top:?}"
    );
}

#[test]
fn code_block_language_label_appears_in_the_top_border() {
    let theme = Theme::new(ThemeVariant::Paper);
    let out = render_text("```rust\nfn x() {}\n```\n", &theme, 40);
    assert!(
        out.contains("\u{256d}\u{2500} rust"),
        "missing label:\n{out}"
    );
    assert!(out.contains('\u{2570}'), "missing bottom border:\n{out}");
}

#[test]
fn alert_callouts_render_a_title() {
    let theme = Theme::new(ThemeVariant::Slate);
    let out = render_text("> [!WARNING]\n> mind the gap\n", &theme, 40);
    assert!(out.contains("Warning"), "alert title missing:\n{out}");
    assert!(out.contains('\u{258e}'), "quote bar missing:\n{out}");
}

/// Flatten a rendered document to plain text for structural assertions.
fn render_text(source: &str, theme: &Theme, width: u16) -> String {
    let doc = render::render(source, theme, opts(width));
    doc.lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Count headings anywhere in the block tree, including inside containers.
fn count_headings(blocks: &[marquee_markdown::render::block::Block]) -> usize {
    use marquee_markdown::render::block::BlockKind;
    blocks
        .iter()
        .map(|b| {
            let own = usize::from(matches!(b.kind, BlockKind::Heading { .. }));
            let nested = match &b.kind {
                BlockKind::BlockQuote { children, .. }
                | BlockKind::FootnoteDefinition { children, .. } => count_headings(children),
                BlockKind::List { items, .. } => {
                    items.iter().map(|i| count_headings(&i.children)).sum()
                }
                _ => 0,
            };
            own + nested
        })
        .sum()
}
