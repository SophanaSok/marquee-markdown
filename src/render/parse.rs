//! Markdown source → [`Block`] tree.
//!
//! Folds pulldown-cmark's event stream into the intermediate tree, carrying
//! source byte ranges, deduplicating heading slugs document-wide, and detecting
//! GFM alert quotes. Runs once per document; resize re-runs layout only.

use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

use super::block::{AlertKind, Alignment, Block, BlockKind, Inline, ListItem};

/// Parse a markdown document into the block tree.
#[must_use]
pub fn parse(source: &str) -> Vec<Block> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_SMART_PUNCTUATION;

    let mut builder = TreeBuilder::default();
    for (event, span) in Parser::new_ext(source, options).into_offset_iter() {
        builder.event(event, span);
    }
    builder.finish()
}

/// Reconstructs nesting from the flat event stream.
///
/// Block containers (quotes, list items, footnote definitions) push a frame
/// onto `stack`; leaf blocks accumulate inlines into `inline_stack`. The two
/// stacks are disjoint by construction: pulldown-cmark never interleaves
/// block-level and inline-level starts out of order.
#[derive(Default)]
struct TreeBuilder {
    /// Finished blocks at the current nesting level, one frame per container.
    stack: Vec<Frame>,
    /// Top-level output.
    root: Vec<Block>,
    /// Open inline containers within the current leaf block.
    inline_stack: Vec<InlineFrame>,
    /// Slug -> times seen, for document-wide heading id dedup.
    slugs: HashMap<String, usize>,
    /// Table under construction.
    table: Option<TableBuilder>,
}

struct Frame {
    kind: FrameKind,
    children: Vec<Block>,
    span: Range<usize>,
}

enum FrameKind {
    Quote(Option<AlertKind>),
    List {
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    Item {
        task: Option<bool>,
    },
    Footnote(String),
    /// A leaf block currently collecting inlines.
    Leaf(LeafKind),
}

enum LeafKind {
    Paragraph,
    Heading(u8),
    Code(Option<String>),
}

struct InlineFrame {
    kind: InlineKind,
    content: Vec<Inline>,
}

enum InlineKind {
    Root,
    Emphasis,
    Strong,
    Strikethrough,
    Link(String),
    Image(String),
}

struct TableBuilder {
    alignments: Vec<Alignment>,
    header: Vec<Vec<Inline>>,
    rows: Vec<Vec<Vec<Inline>>>,
    current_row: Vec<Vec<Inline>>,
    in_head: bool,
    span: Range<usize>,
}

impl TreeBuilder {
    fn event(&mut self, event: Event<'_>, span: Range<usize>) {
        // Tight lists (and other tight containers) deliver inline content with
        // no wrapping Paragraph tag; open a synthetic one so text is never lost.
        if self.inline_stack.is_empty() && opens_inline_content(&event) {
            self.open_leaf(LeafKind::Paragraph, span.clone());
        }
        match event {
            Event::Start(tag) => self.start(tag, span),
            Event::End(tag) => self.end(tag, span),
            Event::Text(t) => self.push_inline_text(&t),
            Event::Code(t) => self.push_inline(Inline::Code(t.into_string())),
            Event::SoftBreak => self.push_inline(Inline::SoftBreak),
            Event::HardBreak => self.push_inline(Inline::HardBreak),
            Event::Rule => self.push_block(Block {
                kind: BlockKind::Rule,
                span,
            }),
            Event::TaskListMarker(done) => {
                if let Some(Frame {
                    kind: FrameKind::Item { task },
                    ..
                }) = self.stack.last_mut()
                {
                    *task = Some(done);
                }
            }
            Event::FootnoteReference(label) => {
                self.push_inline(Inline::FootnoteReference(label.into_string()));
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                // Block-level HTML arrives outside any leaf; inline HTML is
                // kept as literal text within the current inline run.
                if self.in_leaf() {
                    self.push_inline(Inline::Text(html.into_string()));
                } else {
                    self.push_block(Block {
                        kind: BlockKind::Html(html.trim_end().to_owned()),
                        span,
                    });
                }
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => {
                self.push_inline(Inline::Code(t.into_string()));
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>, span: Range<usize>) {
        match tag {
            Tag::Paragraph => self.open_leaf(LeafKind::Paragraph, span),
            Tag::Heading { level, .. } => {
                self.open_leaf(LeafKind::Heading(heading_level(level)), span);
            }
            Tag::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        let lang = info.split_whitespace().next().unwrap_or("");
                        (!lang.is_empty()).then(|| lang.to_owned())
                    }
                    CodeBlockKind::Indented => None,
                };
                self.open_leaf(LeafKind::Code(language), span);
            }
            Tag::BlockQuote(kind) => {
                self.close_dangling_leaf();
                let alert = kind.map(|k| match k {
                    BlockQuoteKind::Note => AlertKind::Note,
                    BlockQuoteKind::Tip => AlertKind::Tip,
                    BlockQuoteKind::Important => AlertKind::Important,
                    BlockQuoteKind::Warning => AlertKind::Warning,
                    BlockQuoteKind::Caution => AlertKind::Caution,
                });
                self.stack.push(Frame {
                    kind: FrameKind::Quote(alert),
                    children: Vec::new(),
                    span,
                });
            }
            Tag::List(start) => {
                self.close_dangling_leaf();
                self.stack.push(Frame {
                    kind: FrameKind::List {
                        start,
                        items: Vec::new(),
                    },
                    children: Vec::new(),
                    span,
                });
            }
            Tag::Item => {
                self.close_dangling_leaf();
                self.stack.push(Frame {
                    kind: FrameKind::Item { task: None },
                    children: Vec::new(),
                    span,
                });
            }
            Tag::FootnoteDefinition(label) => {
                self.close_dangling_leaf();
                self.stack.push(Frame {
                    kind: FrameKind::Footnote(label.into_string()),
                    children: Vec::new(),
                    span,
                });
            }
            Tag::Table(aligns) => {
                self.close_dangling_leaf();
                self.table = Some(TableBuilder {
                    alignments: aligns
                        .iter()
                        .map(|a| match a {
                            pulldown_cmark::Alignment::Center => Alignment::Center,
                            pulldown_cmark::Alignment::Right => Alignment::Right,
                            _ => Alignment::Left,
                        })
                        .collect(),
                    header: Vec::new(),
                    rows: Vec::new(),
                    current_row: Vec::new(),
                    in_head: false,
                    span,
                });
            }
            Tag::TableHead => {
                if let Some(t) = &mut self.table {
                    t.in_head = true;
                }
            }
            Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.current_row = Vec::new();
                }
            }
            Tag::TableCell => self.open_inline_root(),
            Tag::Emphasis => self.open_inline(InlineKind::Emphasis),
            Tag::Strong => self.open_inline(InlineKind::Strong),
            Tag::Strikethrough => self.open_inline(InlineKind::Strikethrough),
            Tag::Link { dest_url, .. } => {
                self.open_inline(InlineKind::Link(dest_url.into_string()))
            }
            Tag::Image { dest_url, .. } => {
                self.open_inline(InlineKind::Image(dest_url.into_string()))
            }
            // Definition lists, metadata, superscript etc. — treat contents as
            // ordinary inlines/blocks; no dedicated styling yet.
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd, span: Range<usize>) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock => self.close_leaf(),
            TagEnd::BlockQuote(_) => {
                self.close_dangling_leaf();
                if let Some(frame) = self.stack.pop() {
                    let FrameKind::Quote(alert) = frame.kind else {
                        return;
                    };
                    self.push_block(Block {
                        kind: BlockKind::BlockQuote {
                            alert,
                            children: frame.children,
                        },
                        span: frame.span,
                    });
                }
            }
            TagEnd::List(_) => {
                if let Some(frame) = self.stack.pop() {
                    let FrameKind::List { start, items } = frame.kind else {
                        return;
                    };
                    self.push_block(Block {
                        kind: BlockKind::List { start, items },
                        span: frame.span,
                    });
                }
            }
            TagEnd::Item => {
                self.close_dangling_leaf();
                if let Some(frame) = self.stack.pop() {
                    let FrameKind::Item { task } = frame.kind else {
                        return;
                    };
                    let item = ListItem {
                        task,
                        children: frame.children,
                    };
                    if let Some(Frame {
                        kind: FrameKind::List { items, .. },
                        ..
                    }) = self.stack.last_mut()
                    {
                        items.push(item);
                    }
                }
            }
            TagEnd::FootnoteDefinition => {
                self.close_dangling_leaf();
                if let Some(frame) = self.stack.pop() {
                    let FrameKind::Footnote(label) = frame.kind else {
                        return;
                    };
                    self.push_block(Block {
                        kind: BlockKind::FootnoteDefinition {
                            label,
                            children: frame.children,
                        },
                        span: frame.span,
                    });
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.push_block(Block {
                        kind: BlockKind::Table {
                            alignments: t.alignments,
                            header: t.header,
                            rows: t.rows,
                        },
                        span: t.span.start..span.end.max(t.span.end),
                    });
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = &mut self.table {
                    t.header = std::mem::take(&mut t.current_row);
                    t.in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.current_row);
                    t.rows.push(row);
                }
            }
            TagEnd::TableCell => {
                let content = self.close_inline_root();
                if let Some(t) = &mut self.table {
                    t.current_row.push(content);
                }
            }
            TagEnd::Emphasis => self.close_inline(Inline::Emphasis),
            TagEnd::Strong => self.close_inline(Inline::Strong),
            TagEnd::Strikethrough => self.close_inline(Inline::Strikethrough),
            TagEnd::Link => {
                if let Some(frame) = self.inline_stack.pop() {
                    let InlineKind::Link(dest) = frame.kind else {
                        return;
                    };
                    self.push_inline(Inline::Link {
                        dest,
                        content: frame.content,
                    });
                }
            }
            TagEnd::Image => {
                if let Some(frame) = self.inline_stack.pop() {
                    let InlineKind::Image(dest) = frame.kind else {
                        return;
                    };
                    self.push_inline(Inline::Image {
                        dest,
                        alt: frame.content,
                    });
                }
            }
            _ => {}
        }
    }

    // --- leaf and inline plumbing -----------------------------------------

    fn in_leaf(&self) -> bool {
        matches!(
            self.stack.last(),
            Some(Frame {
                kind: FrameKind::Leaf(_),
                ..
            })
        ) || !self.inline_stack.is_empty()
    }

    /// Close a synthetic paragraph left open by tight-container content.
    fn close_dangling_leaf(&mut self) {
        if matches!(
            self.stack.last(),
            Some(Frame {
                kind: FrameKind::Leaf(_),
                ..
            })
        ) {
            self.close_leaf();
        }
    }

    fn open_leaf(&mut self, kind: LeafKind, span: Range<usize>) {
        self.stack.push(Frame {
            kind: FrameKind::Leaf(kind),
            children: Vec::new(),
            span,
        });
        self.open_inline_root();
    }

    fn close_leaf(&mut self) {
        let content = self.close_inline_root();
        let Some(frame) = self.stack.pop() else {
            return;
        };
        let FrameKind::Leaf(leaf) = frame.kind else {
            return;
        };
        let kind = match leaf {
            LeafKind::Paragraph => BlockKind::Paragraph(content),
            LeafKind::Heading(level) => {
                let id = self.slug(&Inline::plain_text(&content));
                BlockKind::Heading { level, id, content }
            }
            LeafKind::Code(language) => {
                let text = Inline::plain_text(&content);
                BlockKind::CodeBlock {
                    language,
                    text: text.trim_end_matches('\n').to_owned(),
                }
            }
        };
        self.push_block(Block {
            kind,
            span: frame.span,
        });
    }

    fn open_inline_root(&mut self) {
        self.inline_stack.push(InlineFrame {
            kind: InlineKind::Root,
            content: Vec::new(),
        });
    }

    fn close_inline_root(&mut self) -> Vec<Inline> {
        // Collapse any unclosed inline frames into the root — malformed input
        // must never lose text.
        while self.inline_stack.len() > 1 {
            let frame = self.inline_stack.pop().expect("len > 1");
            self.inline_stack
                .last_mut()
                .expect("root remains")
                .content
                .extend(frame.content);
        }
        self.inline_stack
            .pop()
            .map(|f| f.content)
            .unwrap_or_default()
    }

    fn open_inline(&mut self, kind: InlineKind) {
        self.inline_stack.push(InlineFrame {
            kind,
            content: Vec::new(),
        });
    }

    fn close_inline(&mut self, wrap: impl FnOnce(Vec<Inline>) -> Inline) {
        if let Some(frame) = self.inline_stack.pop() {
            self.push_inline(wrap(frame.content));
        }
    }

    fn push_inline(&mut self, inline: Inline) {
        // Content pushed with no frame open is content thrown away, and it
        // goes silently: that is how `- **Bold.** Rest.` lost its lead-in for
        // three releases. Assert it here rather than trusting every caller,
        // the way `LineSink` asserts the width invariant.
        debug_assert!(
            !self.inline_stack.is_empty(),
            "inline content with nowhere to go: {inline:?}"
        );
        if let Some(frame) = self.inline_stack.last_mut() {
            frame.content.push(inline);
        }
    }

    fn push_inline_text(&mut self, text: &str) {
        // Merge adjacent text runs so wrapping sees whole words.
        if let Some(frame) = self.inline_stack.last_mut() {
            if let Some(Inline::Text(prev)) = frame.content.last_mut() {
                prev.push_str(text);
                return;
            }
            frame.content.push(Inline::Text(text.to_owned()));
        }
    }

    fn push_block(&mut self, block: Block) {
        match self.stack.last_mut() {
            Some(frame) => frame.children.push(block),
            None => self.root.push(block),
        }
    }

    fn slug(&mut self, text: &str) -> String {
        let base: String = text
            .to_lowercase()
            .chars()
            .filter_map(|c| {
                if c.is_alphanumeric() {
                    Some(c)
                } else if c.is_whitespace() || c == '-' || c == '_' {
                    Some('-')
                } else {
                    None
                }
            })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let base = if base.is_empty() {
            "section".to_owned()
        } else {
            base
        };
        let count = self.slugs.entry(base.clone()).or_insert(0);
        let slug = if *count == 0 {
            base.clone()
        } else {
            format!("{base}-{count}")
        };
        *count += 1;
        slug
    }

    fn finish(mut self) -> Vec<Block> {
        // Drain anything malformed input left open.
        while let Some(frame) = self.stack.pop() {
            let blocks = frame.children;
            match self.stack.last_mut() {
                Some(parent) => parent.children.extend(blocks),
                None => self.root.extend(blocks),
            }
        }
        self.root
    }
}

/// Whether this event begins inline content, and so needs somewhere to put it.
///
/// A tight list item delivers its content with no wrapping `Paragraph` tag, so
/// the first such event has to open one. The emphasis and link *starts* belong
/// here as much as text does: `- **Bold.** Rest.` opens a `Strong` frame
/// before any text arrives, and with no root frame beneath it the finished
/// `Strong` had nowhere to be pushed and was dropped on the floor — silently,
/// because `push_inline` does nothing when the stack is empty.
fn opens_inline_content(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::Text(_)
            | Event::Code(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::FootnoteReference(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::InlineHtml(_)
            | Event::Start(
                Tag::Emphasis
                    | Tag::Strong
                    | Tag::Strikethrough
                    | Tag::Link { .. }
                    | Tag::Image { .. }
            )
    )
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<BlockKind> {
        parse(source).into_iter().map(|b| b.kind).collect()
    }

    #[test]
    fn parses_heading_with_slug() {
        let blocks = parse("# Hello World\n");
        assert_eq!(blocks.len(), 1);
        let BlockKind::Heading { level, id, content } = &blocks[0].kind else {
            panic!("expected heading, got {:?}", blocks[0].kind);
        };
        assert_eq!(*level, 1);
        assert_eq!(id, "hello-world");
        assert_eq!(Inline::plain_text(content), "Hello World");
    }

    #[test]
    fn duplicate_headings_get_deduplicated_slugs() {
        let blocks = parse("## Setup\n\n## Setup\n\n## Setup\n");
        let ids: Vec<_> = blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Heading { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["setup", "setup-1", "setup-2"]);
    }

    #[test]
    fn fenced_code_keeps_language_and_text() {
        let blocks = parse("```rust\nfn main() {}\n```\n");
        let BlockKind::CodeBlock { language, text } = &blocks[0].kind else {
            panic!("expected code block");
        };
        assert_eq!(language.as_deref(), Some("rust"));
        assert_eq!(text, "fn main() {}");
    }

    #[test]
    fn indented_code_has_no_language() {
        let blocks = parse("    indented line\n");
        let BlockKind::CodeBlock { language, text } = &blocks[0].kind else {
            panic!("expected code block, got {:?}", blocks[0].kind);
        };
        assert!(language.is_none());
        assert_eq!(text, "indented line");
    }

    #[test]
    fn gfm_alert_is_detected_and_typed() {
        let blocks = parse("> [!WARNING]\n> Careful now.\n");
        let BlockKind::BlockQuote { alert, children } = &blocks[0].kind else {
            panic!("expected quote, got {:?}", blocks[0].kind);
        };
        assert_eq!(*alert, Some(AlertKind::Warning));
        assert_eq!(children.len(), 1);
    }

    #[test]
    fn plain_quote_has_no_alert() {
        let blocks = parse("> Just a quote.\n");
        let BlockKind::BlockQuote { alert, .. } = &blocks[0].kind else {
            panic!("expected quote");
        };
        assert!(alert.is_none());
    }

    #[test]
    fn nested_quotes_nest() {
        let blocks = parse("> outer\n> > inner\n");
        let BlockKind::BlockQuote { children, .. } = &blocks[0].kind else {
            panic!("expected quote");
        };
        assert!(
            children
                .iter()
                .any(|b| matches!(b.kind, BlockKind::BlockQuote { .. })),
            "inner quote missing: {children:?}"
        );
    }

    #[test]
    fn task_list_markers_are_captured() {
        let blocks = parse("- [ ] todo\n- [x] done\n");
        let BlockKind::List { items, .. } = &blocks[0].kind else {
            panic!("expected list");
        };
        assert_eq!(items[0].task, Some(false));
        assert_eq!(items[1].task, Some(true));
    }

    #[test]
    fn a_tight_list_item_keeps_content_that_starts_with_formatting() {
        // `- **Bold.** Rest.` rendered as `• Rest.` — the lead-in vanished.
        // A tight item has no wrapping paragraph, and the synthetic one was
        // opened only for text-shaped events, so an emphasis *start* arriving
        // first pushed a frame with no root under it. The finished `Strong`
        // then had nowhere to go and was dropped without a word.
        for (source, want) in [
            ("- **Bold lead.** Rest.\n", "Bold lead. Rest."),
            ("- *Em lead.* Rest.\n", "Em lead. Rest."),
            ("- ~~Struck lead.~~ Rest.\n", "Struck lead. Rest."),
            ("- [Link lead](http://x) rest.\n", "Link lead rest."),
            ("- ![Image lead](i.png) rest.\n", "Image lead rest."),
            // Inline HTML is literal text here; what it means is a separate
            // question. Without this it opened a *block* mid-item instead.
            ("- <b>HTML lead.</b> Rest.\n", "<b>HTML lead.</b> Rest."),
            ("1. **Ordered bold.** Rest.\n", "Ordered bold. Rest."),
            ("- **Only bold.**\n", "Only bold."),
        ] {
            let blocks = parse(source);
            let BlockKind::List { items, .. } = &blocks[0].kind else {
                panic!("a list, got {:?} from {source:?}", blocks[0].kind);
            };
            let BlockKind::Paragraph(content) = &items[0].children[0].kind else {
                panic!(
                    "a paragraph, got {:?} from {source:?}",
                    items[0].children[0].kind
                );
            };
            assert_eq!(Inline::plain_text(content), want, "from {source:?}");
        }
    }

    #[test]
    fn a_formatted_lead_in_keeps_its_styling_not_just_its_text() {
        let blocks = parse("- **Bold lead.** Rest.\n");
        let BlockKind::List { items, .. } = &blocks[0].kind else {
            panic!("a list");
        };
        let BlockKind::Paragraph(content) = &items[0].children[0].kind else {
            panic!("a paragraph");
        };
        assert!(
            matches!(content.first(), Some(Inline::Strong(_))),
            "the lead-in lost its weight: {content:#?}"
        );
    }

    #[test]
    fn tight_list_item_text_is_preserved() {
        let blocks = parse("- alpha\n- beta\n");
        let BlockKind::List { items, .. } = &blocks[0].kind else {
            panic!("expected list");
        };
        let texts: Vec<String> = items
            .iter()
            .map(|i| {
                i.children
                    .iter()
                    .filter_map(|b| match &b.kind {
                        BlockKind::Paragraph(c) => Some(Inline::plain_text(c)),
                        _ => None,
                    })
                    .collect()
            })
            .collect();
        assert_eq!(texts, ["alpha", "beta"]);
    }

    #[test]
    fn ordered_list_keeps_start() {
        let blocks = parse("3. three\n4. four\n");
        let BlockKind::List { start, items } = &blocks[0].kind else {
            panic!("expected list");
        };
        assert_eq!(*start, Some(3));
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn nested_list_structure() {
        let blocks = parse("- a\n  - b\n");
        let BlockKind::List { items, .. } = &blocks[0].kind else {
            panic!("expected list");
        };
        assert!(
            items[0]
                .children
                .iter()
                .any(|b| matches!(b.kind, BlockKind::List { .. })),
            "nested list missing: {:?}",
            items[0].children
        );
    }

    #[test]
    fn table_with_alignments() {
        let blocks = parse("| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n");
        let BlockKind::Table {
            alignments,
            header,
            rows,
        } = &blocks[0].kind
        else {
            panic!("expected table, got {:?}", blocks[0].kind);
        };
        assert_eq!(
            alignments,
            &[Alignment::Left, Alignment::Center, Alignment::Right]
        );
        assert_eq!(header.len(), 3);
        assert_eq!(rows.len(), 1);
        assert_eq!(Inline::plain_text(&rows[0][2]), "3");
    }

    #[test]
    fn rule_parses() {
        assert!(matches!(kinds("---\n")[0], BlockKind::Rule));
    }

    #[test]
    fn footnotes_produce_reference_and_definition() {
        let blocks = parse("text[^1]\n\n[^1]: the note\n");
        assert!(blocks.iter().any(|b| matches!(
            &b.kind,
            BlockKind::FootnoteDefinition { label, .. } if label == "1"
        )));
    }

    #[test]
    fn inline_styles_nest() {
        let blocks = parse("***both*** and ~~gone~~\n");
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("expected paragraph");
        };
        assert_eq!(Inline::plain_text(content), "both and gone");
        fn find_strike(c: &[Inline]) -> bool {
            c.iter().any(|i| match i {
                Inline::Strikethrough(_) => true,
                Inline::Emphasis(c) | Inline::Strong(c) => find_strike(c),
                _ => false,
            })
        }
        assert!(find_strike(content));
    }

    #[test]
    fn link_and_image_keep_destinations() {
        let blocks = parse("[text](https://x.y) ![alt](img.png)\n");
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("expected paragraph");
        };
        assert!(
            content
                .iter()
                .any(|i| matches!(i, Inline::Link { dest, .. } if dest == "https://x.y"))
        );
        assert!(
            content
                .iter()
                .any(|i| matches!(i, Inline::Image { dest, .. } if dest == "img.png"))
        );
    }

    #[test]
    fn html_block_is_preserved() {
        let blocks = parse("<div>\nhtml\n</div>\n");
        assert!(
            blocks
                .iter()
                .any(|b| matches!(&b.kind, BlockKind::Html(h) if h.contains("div")))
        );
    }

    #[test]
    fn kitchen_sink_fixture_parses_without_loss() {
        let src = include_str!("../../tests/fixtures/kitchen-sink.md");
        let blocks = parse(src);
        // Structural smoke checks over the full fixture.
        let heading_count = count_kind(&blocks, &|k| matches!(k, BlockKind::Heading { .. }));
        let table_count = count_kind(&blocks, &|k| matches!(k, BlockKind::Table { .. }));
        let code_count = count_kind(&blocks, &|k| matches!(k, BlockKind::CodeBlock { .. }));
        let quote_count = count_kind(&blocks, &|k| matches!(k, BlockKind::BlockQuote { .. }));
        assert!(heading_count >= 10, "headings: {heading_count}");
        assert_eq!(table_count, 2, "tables");
        assert!(code_count >= 4, "code blocks: {code_count}");
        assert!(quote_count >= 6, "quotes incl. alerts: {quote_count}");
    }

    fn count_kind(blocks: &[Block], pred: &dyn Fn(&BlockKind) -> bool) -> usize {
        blocks
            .iter()
            .map(|b| {
                let own = usize::from(pred(&b.kind));
                let nested = match &b.kind {
                    BlockKind::BlockQuote { children, .. }
                    | BlockKind::FootnoteDefinition { children, .. } => count_kind(children, pred),
                    BlockKind::List { items, .. } => {
                        items.iter().map(|i| count_kind(&i.children, pred)).sum()
                    }
                    _ => 0,
                };
                own + nested
            })
            .sum()
    }
}
