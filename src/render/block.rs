//! The intermediate block tree between pulldown-cmark events and layout.
//!
//! Layout cannot run straight off the event stream: tables must be measured
//! before they are emitted, nested lists and quotes need indent context, GFM
//! alerts need lookahead into a blockquote's first paragraph, and heading slugs
//! need document-wide deduplication. Parsing once into this tree also means a
//! resize re-runs layout only — the parse is cached for the document's life.

use std::ops::Range;

pub use crate::theme::AlertKind;

/// The deepest nesting a document is allowed to build.
///
/// The trees in this module and in [`html`](super::html) are walked by
/// recursion in several places — layout above all, plus the heading counter
/// behind [`Document::heading_count`](super::Document::heading_count), the
/// plain-text flattener, and the derived `Drop` that frees them — so tree
/// depth is call-stack depth, and a *document* chooses it. Layout is by far
/// the deepest of them: on an 8 MiB stack it runs out at around 3,000 levels
/// of `> - `, where dropping the tree survives to roughly 40,000.
///
/// Running out is not a clean failure. A stack overflow **aborts**, and an
/// abort does not unwind, so neither the RAII terminal guard nor the panic
/// hook that exists to restore the screen ever runs: the reader dies with the
/// alternate screen still up, the cursor still hidden, and bracketed paste
/// and mouse reporting still on — a terminal that needs `reset`. Nor is the
/// document necessarily the reader's own, since `https://` and `github://`
/// are sources like any other.
///
/// 256 sits far below the first failure and far above anything a terminal can
/// show. Every level of quote or list costs two cells of lead, so 256 levels
/// is 512 cells of decoration before a character of text — wider than any
/// real column, and `LineSink::fit_lead` already gives the decoration away to
/// keep the text on the page. Past a few dozen levels at 80 columns the text
/// is pinned to a one-cell column already.
///
/// Past the cap a container is not represented: its children are spliced into
/// its parent, so the content still renders, at the capped indent rather than
/// a deeper one. That is the same trade `Role::Other` makes in
/// [`html`](super::html) — keep the children, drop the tag.
pub(super) const MAX_NESTING: usize = 256;

/// One block-level element, with the byte range of the markdown source it came
/// from. Source ranges are what let the viewer keep the reading position stable
/// across re-layout: remember the top line's range, re-lay, seek back to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    /// Byte range in the original markdown source.
    pub span: Range<usize>,
    /// Which edge the content is set against.
    ///
    /// Only raw HTML's `align` attribute ever sets this — markdown has no
    /// syntax that could ask for it — and only the heading and paragraph
    /// emitters honour it. A field rather than a wrapping `BlockKind` variant
    /// on purpose: the tree is walked by several hand-written recursive
    /// helpers that end in `_ => 0`, and a new container variant any one of
    /// them forgot would make `Document::heading_count` disagree with the
    /// outline silently. That is the pane-geometry bug this project has
    /// already paid for once. A field cannot be forgotten, because every
    /// construction site is a compile error until it is filled in.
    pub align: Alignment,
}

impl Block {
    /// The ordinary case: set against the left edge, like all of markdown.
    #[must_use]
    pub fn at(kind: BlockKind, span: Range<usize>) -> Self {
        Self {
            kind,
            span,
            align: Alignment::Left,
        }
    }
}

/// Block-level structure. Container variants own their children so layout can
/// recurse with accumulated indent.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    Heading {
        /// 1-6.
        level: u8,
        /// Slug for anchors, deduplicated document-wide (`intro`, `intro-1`).
        id: String,
        content: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    CodeBlock {
        /// Fence info string's first word, if any (`rust`, `jsonc`).
        language: Option<String>,
        /// Raw text, exactly as written; trailing newline trimmed.
        text: String,
    },
    BlockQuote {
        /// `Some` when the quote's first paragraph starts with a GFM alert
        /// marker such as `[!NOTE]`; the marker itself is stripped.
        alert: Option<AlertKind>,
        children: Vec<Block>,
    },
    List {
        /// `Some(start)` for ordered lists, `None` for bullet lists.
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    Table {
        alignments: Vec<Alignment>,
        /// Header row; one `Vec<Inline>` per cell.
        header: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    /// Thematic break (`---`, `***`).
    Rule,
    /// Raw HTML block, kept verbatim and rendered as muted literal text.
    /// Only produced when the HTML could not be interpreted, or when the
    /// reader asked for it with `html = "literal"`.
    Html(String),
    /// A footnote definition (`[^1]: …`); rendered at its source position.
    FootnoteDefinition {
        label: String,
        children: Vec<Block>,
    },
}

/// One list item. Task state comes from GFM task list markers.
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    /// `Some(done)` when the item is a task (`- [x]` / `- [ ]`).
    pub task: Option<bool>,
    pub children: Vec<Block>,
}

/// Table column alignment from the delimiter row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}

/// Inline (span-level) content. Emphasis variants nest.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    Code(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Strikethrough(Vec<Inline>),
    Link {
        /// Destination as written; resolution against a base happens at open
        /// time, not parse time.
        dest: String,
        content: Vec<Inline>,
    },
    Image {
        dest: String,
        /// Alt text, rendered as the placeholder.
        alt: Vec<Inline>,
    },
    /// `[^label]` reference marker.
    FootnoteReference(String),
    /// Newline that renders as a space (`CommonMark` soft break).
    SoftBreak,
    /// Forced line break (trailing spaces or backslash).
    HardBreak,
}

impl Inline {
    /// Flatten to plain text, recursively — used for slugs, TOC labels, and
    /// table column measurement pre-passes.
    #[must_use]
    pub fn plain_text(content: &[Inline]) -> String {
        let mut out = String::new();
        Self::collect_plain(content, &mut out);
        out
    }

    fn collect_plain(content: &[Inline], out: &mut String) {
        for inline in content {
            match inline {
                Inline::Text(t) | Inline::Code(t) => out.push_str(t),
                Inline::Emphasis(c) | Inline::Strong(c) | Inline::Strikethrough(c) => {
                    Self::collect_plain(c, out);
                }
                Inline::Link { content, .. } => Self::collect_plain(content, out),
                Inline::Image { alt, .. } => Self::collect_plain(alt, out),
                Inline::FootnoteReference(label) => {
                    out.push('[');
                    out.push_str(label);
                    out.push(']');
                }
                Inline::SoftBreak | Inline::HardBreak => out.push(' '),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_flattens_nesting() {
        let content = vec![
            Inline::Text("a ".into()),
            Inline::Strong(vec![
                Inline::Text("b ".into()),
                Inline::Emphasis(vec![Inline::Text("c".into())]),
            ]),
            Inline::SoftBreak,
            Inline::Code("d".into()),
        ];
        assert_eq!(Inline::plain_text(&content), "a b c d");
    }

    #[test]
    fn plain_text_uses_link_text_not_dest() {
        let content = vec![Inline::Link {
            dest: "https://example.com".into(),
            content: vec![Inline::Text("label".into())],
        }];
        assert_eq!(Inline::plain_text(&content), "label");
    }
}
