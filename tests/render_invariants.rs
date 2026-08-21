//! End-to-end invariants of the rendering engine.
//!
//! These run over the kitchen-sink fixture, which exercises every construct the
//! renderer handles, at many widths. The width invariant in particular is the
//! foundation the centered reading column and the sealed code cards rest on, so
//! it is checked exhaustively rather than by example.

use marquee_markdown::render::{
    self, Document, HtmlMode, LayoutOptions, LineKind, ParseOptions, RenderedDoc,
};
use marquee_markdown::theme::system::{self, TerminalColors};
use marquee_markdown::theme::{Rgb, Theme, ThemeVariant};

const FIXTURE: &str = include_str!("fixtures/kitchen-sink.md");

/// Every theme the invariants have to hold for, by the name to blame.
///
/// The shipped two plus one built from a terminal's own colors. A `system`
/// palette is chosen by somebody else's colorscheme rather than by us, so
/// including it is what stops the invariants from quietly meaning "holds for
/// the two palettes we happened to write".
fn themes() -> Vec<(String, Theme)> {
    let mut out: Vec<(String, Theme)> = ThemeVariant::all()
        .iter()
        .map(|variant| (variant.name().to_owned(), Theme::new(*variant)))
        .collect();
    out.push((
        "system".to_owned(),
        system::theme(&terminal()).expect("theme"),
    ));
    out
}

/// What a terminal with a real colorscheme answers.
fn terminal() -> TerminalColors {
    let mut colors = TerminalColors {
        fg: Some(Rgb(0xd8, 0xd8, 0xd8)),
        bg: Some(Rgb(0x18, 0x18, 0x18)),
        ..TerminalColors::UNKNOWN
    };
    for (slot, value) in [
        (1usize, Rgb(0xab, 0x46, 0x42)),
        (2, Rgb(0xa1, 0xb5, 0x6c)),
        (3, Rgb(0xf7, 0xca, 0x88)),
        (4, Rgb(0x7c, 0xaf, 0xc2)),
        (5, Rgb(0xba, 0x8b, 0xaf)),
        (8, Rgb(0x58, 0x58, 0x58)),
    ] {
        colors.ansi[slot] = Some(value);
    }
    colors
}

fn opts(width: u16) -> LayoutOptions {
    LayoutOptions {
        width,
        code_line_numbers: false,
        preserve_new_lines: false,
    }
}

#[test]
fn every_line_is_exactly_the_content_width_at_every_width() {
    for (name, theme) in themes() {
        for width in [10u16, 20, 32, 40, 55, 72, 80, 100, 120, 200] {
            let doc = render::render(FIXTURE, &theme, opts(width));
            for (i, line) in doc.lines.iter().enumerate() {
                assert_eq!(
                    line.width(),
                    usize::from(width),
                    "{name} width {width}: line {i} is {} cells\n{:?}",
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
fn short_table_cells_never_wrap_whatever_they_contain() {
    // The column solver and the row emitter have to agree about how wide a cell
    // is. They measured it by different routes once, and an inline code span —
    // drawn as a padded chip, two cells wider than its text — came out of the
    // solver too narrow for its own content: the row grew to three lines, a
    // blank, the text, another blank.
    //
    // Every kind here fits its column comfortably, so a row that is taller than
    // one line means something measured the cell as narrower than it draws.
    let theme = Theme::new(ThemeVariant::Slate);
    for cell in [
        "plain",
        "`code`",
        "*em*",
        "**strong**",
        "~~struck~~",
        "[link](x)",
    ] {
        let source = format!("| Kind | Value |\n| --- | --- |\n| {cell} | yes |\n");
        for width in [60u16, 80, 120] {
            let out = render_text(&source, &theme, width);
            let rows = out
                .lines()
                .filter(|l| l.contains('\u{2502}') && !l.contains('\u{253c}'))
                .count();
            assert_eq!(
                rows, 2,
                "cell {cell:?} at width {width} produced {rows} content lines, want 2:\n{out}"
            );
        }
    }
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

/// Lay the fixture out under one HTML mode.
fn render_html(source: &str, theme: &Theme, width: u16, html: HtmlMode) -> RenderedDoc {
    let mut parse = ParseOptions::default();
    parse.html = html;
    render::render_with(source, theme, parse, opts(width))
}

fn text_of(doc: &RenderedDoc) -> String {
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

#[test]
fn every_html_mode_keeps_the_width_invariant() {
    for (name, theme) in themes() {
        for html in HtmlMode::ALL {
            for width in [10u16, 20, 32, 40, 55, 72, 80, 100, 120, 200] {
                let doc = render_html(FIXTURE, &theme, width, html);
                for (i, line) in doc.lines.iter().enumerate() {
                    assert_eq!(
                        line.width(),
                        usize::from(width),
                        "line {i} at width {width} under {html:?} in {name}"
                    );
                }
            }
        }
    }
}

#[test]
fn interpreted_html_puts_no_markup_on_the_page() {
    // The reported bug, as an assertion. Code blocks legitimately contain
    // angle brackets, and the literal fallback is markup on purpose, so both
    // are exempt — everything else must read as prose.
    let theme = Theme::new(ThemeVariant::Slate);
    let doc = render_html(FIXTURE, &theme, 80, HtmlMode::Render);
    for (i, line) in doc.lines.iter().enumerate() {
        if matches!(
            doc.meta[i].kind,
            LineKind::Code { .. } | LineKind::CodeBorder { .. } | LineKind::Html
        ) {
            continue;
        }
        let text: String = line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        for markup in [
            "<p ",
            "<p>",
            "</p>",
            "<div",
            "</div>",
            "align=\"center\"",
            "<br>",
        ] {
            assert!(
                !text.contains(markup),
                "line {i} shows {markup:?}: {text:?}"
            );
        }
    }
}

#[test]
fn an_html_heading_reaches_the_outline_and_the_count() {
    // Pane geometry is decided from `heading_count` at parse time, before any
    // layout exists, so the two must agree or the contents pane decides wrong.
    let theme = Theme::new(ThemeVariant::Slate);
    let mut parse = ParseOptions::default();
    parse.html = HtmlMode::Render;
    let document = Document::parse_with(FIXTURE, parse);
    let doc = document.layout(&theme, opts(80));
    assert_eq!(
        document.heading_count(),
        doc.outline.len(),
        "the parse-time count and the outline disagree"
    );
    assert!(
        doc.outline.iter().any(|a| a.text == "An HTML heading"),
        "the HTML heading is missing from the outline: {:?}",
        doc.outline.iter().map(|a| &a.text).collect::<Vec<_>>()
    );
}

#[test]
fn a_centered_block_is_actually_centered() {
    let theme = Theme::new(ThemeVariant::Slate);
    let doc = render_html(FIXTURE, &theme, 80, HtmlMode::Render);
    let line = doc
        .lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .find(|text| text.contains("An HTML heading"))
        .expect("the centered heading is on the page");
    let left = line.len() - line.trim_start().len();
    let right = line.len() - line.trim_end().len();
    assert!(
        left.abs_diff(right) <= 1 && left > 0,
        "not centered: {left} left, {right} right in {line:?}"
    );
}

#[test]
fn unrecognized_html_still_renders_as_literal_markup() {
    // `<details>` and `<table>` have no emitter, and a table read as one
    // run-on sentence is worse than one read as tags.
    let theme = Theme::new(ThemeVariant::Slate);
    let doc = render_html(FIXTURE, &theme, 80, HtmlMode::Render);
    assert!(
        text_of(&doc).contains("<details>"),
        "the declined block lost its markup"
    );
}

#[test]
fn hiding_html_leaves_no_run_of_blank_lines() {
    // Dropping a block must not leave the blank that separated it behind.
    let theme = Theme::new(ThemeVariant::Slate);
    let doc = render_html(FIXTURE, &theme, 80, HtmlMode::Hide);
    let mut blanks = 0;
    for (i, meta) in doc.meta.iter().enumerate() {
        if meta.kind == LineKind::Blank {
            blanks += 1;
            assert!(blanks < 2, "two blank lines in a row at line {i}");
        } else {
            blanks = 0;
        }
    }
    for (i, meta) in doc.meta.iter().enumerate() {
        // Code blocks legitimately contain angle brackets.
        if matches!(
            meta.kind,
            LineKind::Code { .. } | LineKind::CodeBorder { .. }
        ) {
            continue;
        }
        let text: String = doc.lines[i]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(
            !text.contains("<p") && !text.contains("<div") && !text.contains("</"),
            "hide mode left markup on line {i}: {text:?}"
        );
    }
}
