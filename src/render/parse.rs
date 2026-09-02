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

use super::block::{AlertKind, Alignment, Block, BlockKind, Inline, ListItem, MAX_NESTING};
use super::html::{self, HtmlMode};

/// Options for one parse.
///
/// Separate from `LayoutOptions` because these change the block tree, and the
/// tree is built once and laid out many times. Anything here invalidates a
/// cached parse; anything there does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ParseOptions {
    /// What to do with raw HTML.
    pub html: HtmlMode,
}

/// Parse a markdown document into the block tree, with default options.
#[must_use]
pub fn parse(source: &str) -> Vec<Block> {
    parse_with(source, ParseOptions::default())
}

/// Parse a markdown document into the block tree.
#[must_use]
pub fn parse_with(source: &str, options: ParseOptions) -> Vec<Block> {
    let parser = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_SMART_PUNCTUATION
        | Options::ENABLE_MATH;

    let mut builder = TreeBuilder {
        options,
        ..TreeBuilder::default()
    };
    for (event, span) in CapNesting::new(Parser::new_ext(source, parser).into_offset_iter()) {
        builder.event(event, span);
    }
    builder.finish()
}

/// Drops container `Start`/`End` pairs nested deeper than [`MAX_NESTING`].
///
/// Both halves of a pair are dropped, so the stream stays balanced and
/// [`TreeBuilder`] needs to know nothing about the cap. The content inside
/// still arrives and still lands in whatever container is open at the cap,
/// which is what makes this a flattening rather than a truncation — a
/// pathological document renders as its text at the capped indent, not as an
/// error and not as nothing.
struct CapNesting<I> {
    inner: I,
    /// Containers open and represented in the tree.
    depth: usize,
    /// Containers open whose `Start` was dropped.
    suppressed: usize,
}

impl<I> CapNesting<I> {
    fn new(inner: I) -> Self {
        Self {
            inner,
            depth: 0,
            suppressed: 0,
        }
    }
}

/// The tags that open a [`Frame`] which can contain another one.
///
/// `Tag::HtmlBlock` opens a frame too, but cannot nest: pulldown-cmark
/// delivers an HTML block as a flat run of `Event::Html` lines rather than as
/// a tree, so it adds one level and no more.
fn nests(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::FootnoteDefinition(_)
    )
}

/// The closing half of [`nests`].
fn nests_end(tag: TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition
    )
}

impl<'a, I> Iterator for CapNesting<I>
where
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    type Item = (Event<'a>, Range<usize>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (event, span) = self.inner.next()?;
            match &event {
                Event::Start(tag) if nests(tag) => {
                    if self.depth >= MAX_NESTING {
                        self.suppressed += 1;
                        continue;
                    }
                    self.depth += 1;
                }
                Event::End(tag) if nests_end(*tag) => {
                    // A suppressed container always closes before any
                    // container still represented: the stream is properly
                    // nested, so whatever opened last closes first, and
                    // everything opened after the cap was suppressed too.
                    if self.suppressed > 0 {
                        self.suppressed -= 1;
                        continue;
                    }
                    debug_assert!(self.depth > 0, "container End with nothing open");
                    self.depth = self.depth.saturating_sub(1);
                }
                _ => {}
            }
            return Some((event, span));
        }
    }
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
    /// What the reader asked for.
    options: ParseOptions,
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
    /// A raw HTML block, accumulating its source.
    ///
    /// pulldown-cmark delivers one `Event::Html` per *line* of an HTML block,
    /// so without somewhere to put them each line became its own block — and
    /// the layout put a blank line between every one. The buffer is what makes
    /// an HTML block a single thing that can be looked at as a whole.
    Html(String),
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

pub(super) enum InlineKind {
    Root,
    Emphasis,
    Strong,
    Strikethrough,
    Link(String),
    Image(String),
    /// `<code>` written as inline HTML. Flattened on close, because
    /// [`Inline::Code`] holds text rather than children.
    Code,
    /// An inline tag whose closing tag has to pop something, but which
    /// contributes no styling of its own (`<span>`, `<sub>`).
    Transparent,
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
            Event::Rule => self.push_block(Block::at(BlockKind::Rule, span)),
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
            Event::Html(html) => match self.stack.last_mut() {
                // One `Event::Html` per *source line*, so the open frame is
                // where the lines are joined back into the block the author
                // actually wrote. Without it each line became its own block,
                // and the layout put a blank line between every one.
                Some(Frame {
                    kind: FrameKind::Html(buffer),
                    ..
                }) => buffer.push_str(&html),
                // Defensive: pulldown always frames these. A version that
                // stopped must not silently swallow a document's HTML.
                _ => self.finish_html(&html, span),
            },
            Event::InlineHtml(html) => self.inline_html(&html, span),
            // TeX is not typeset — there is no glyph budget for it in a
            // terminal cell grid. Code styling is the honest fallback: it
            // marks the span as notation rather than prose, and the source
            // is what the author would have to read anyway. Without
            // `ENABLE_MATH` the delimiters and the formula both reach the
            // page as literal text, which is strictly worse.
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
            Tag::HtmlBlock => {
                self.close_dangling_leaf();
                self.stack.push(Frame {
                    kind: FrameKind::Html(String::new()),
                    children: Vec::new(),
                    span,
                });
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
                    self.push_block(Block::at(
                        BlockKind::BlockQuote {
                            alert,
                            children: frame.children,
                        },
                        frame.span,
                    ));
                }
            }
            TagEnd::List(_) => {
                if let Some(frame) = self.stack.pop() {
                    let FrameKind::List { start, items } = frame.kind else {
                        return;
                    };
                    self.push_block(Block::at(BlockKind::List { start, items }, frame.span));
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
                    self.push_block(Block::at(
                        BlockKind::FootnoteDefinition {
                            label,
                            children: frame.children,
                        },
                        frame.span,
                    ));
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.push_block(Block::at(
                        BlockKind::Table {
                            alignments: t.alignments,
                            header: t.header,
                            rows: t.rows,
                        },
                        t.span.start..span.end.max(t.span.end),
                    ));
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
            TagEnd::HtmlBlock => {
                if let Some(Frame {
                    kind: FrameKind::Html(buffer),
                    span,
                    ..
                }) = self.stack.pop()
                {
                    self.finish_html(&buffer, span);
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

    // --- raw HTML ---------------------------------------------------------

    /// Dispose of one complete raw HTML block.
    fn finish_html(&mut self, raw: &str, span: Range<usize>) {
        match self.options.html {
            HtmlMode::Hide => {}
            HtmlMode::Literal => self.push_literal_html(raw, span),
            HtmlMode::Render => {
                // `slug` borrows all of `self`, and the interpreter needs it
                // so an HTML heading joins the same document-wide dedup as a
                // markdown one. Hand it just the counter.
                let slugs = &mut self.slugs;
                let interpreted = html::interpret(raw, &span, &mut |text| slug_in(slugs, text));
                match interpreted {
                    Some(blocks) => {
                        for block in blocks {
                            self.push_block(block);
                        }
                    }
                    None => self.push_literal_html(raw, span),
                }
            }
        }
    }

    fn push_literal_html(&mut self, raw: &str, span: Range<usize>) {
        let text = raw.trim_end().to_owned();
        if !text.is_empty() {
            self.push_block(Block::at(BlockKind::Html(text), span));
        }
    }

    /// Dispose of one inline HTML tag.
    ///
    /// These arrive one tag at a time with the text between them delivered as
    /// ordinary `Event::Text`, so the state has to live across events. It
    /// already does: `inline_stack` is exactly the mechanism `Tag::Emphasis`
    /// uses, and an opening tag pushes onto it the same way.
    fn inline_html(&mut self, raw: &str, span: Range<usize>) {
        if !self.in_leaf() {
            // A tag on its own outside any leaf is a block, not an inline.
            self.finish_html(raw, span);
            return;
        }
        match self.options.html {
            HtmlMode::Hide => {}
            HtmlMode::Literal => self.push_inline(Inline::Text(raw.to_owned())),
            HtmlMode::Render => self.render_inline_html(raw),
        }
    }

    fn render_inline_html(&mut self, raw: &str) {
        let trimmed = raw.trim();
        // A closing tag pops the frame its opening tag pushed. Anything that
        // does not match is dropped rather than shown: in `render` mode the
        // promise is that no markup reaches the page.
        if let Some(name) = trimmed
            .strip_prefix("</")
            .and_then(|rest| rest.strip_suffix('>'))
        {
            self.close_inline_html(&name.trim().to_lowercase());
            return;
        }
        match html::inline_open(raw) {
            Some(html::InlineTag::Open(kind)) => self.open_inline(kind),
            Some(html::InlineTag::Void(inlines)) => {
                for inline in inlines {
                    self.push_inline(inline);
                }
            }
            // Unparseable, or something with no inline meaning: drop it.
            None => {}
        }
    }

    /// Close the innermost open inline frame belonging to `name`.
    ///
    /// Everything opened inside it closes with it, so mis-nested markup costs
    /// styling rather than text — the same policy `close_inline_root` already
    /// applies to input pulldown itself leaves open.
    fn close_inline_html(&mut self, name: &str) {
        let Some(target) = html::inline_kind(name) else {
            return;
        };
        let Some(depth) = self.inline_stack.iter().rposition(|frame| {
            std::mem::discriminant(&frame.kind) == std::mem::discriminant(&target)
        }) else {
            return;
        };
        if depth == 0 {
            return; // never pop the root
        }
        while self.inline_stack.len() > depth {
            let frame = self.inline_stack.pop().expect("len > depth");
            let wrapped = match frame.kind {
                InlineKind::Emphasis => Inline::Emphasis(frame.content),
                InlineKind::Strong => Inline::Strong(frame.content),
                InlineKind::Strikethrough => Inline::Strikethrough(frame.content),
                InlineKind::Link(dest) => Inline::Link {
                    dest,
                    content: frame.content,
                },
                InlineKind::Image(dest) => Inline::Image {
                    dest,
                    alt: frame.content,
                },
                InlineKind::Code => Inline::Code(Inline::plain_text(&frame.content)),
                InlineKind::Transparent | InlineKind::Root => {
                    // Contributes no styling: splice the children up.
                    for inline in frame.content {
                        self.push_inline(inline);
                    }
                    continue;
                }
            };
            self.push_inline(wrapped);
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
        self.push_block(Block::at(kind, frame.span));
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
        slug_in(&mut self.slugs, text)
    }
}

/// Allocate a heading slug, deduplicated against everything seen so far.
///
/// A free function taking only the counter, because the HTML interpreter needs
/// it while `TreeBuilder` is already borrowed elsewhere — and because an HTML
/// heading must share one counter with the markdown ones or `#setup` would
/// name two different places.
fn slug_in(slugs: &mut HashMap<String, usize>, text: &str) -> String {
    {
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
        let count = slugs.entry(base.clone()).or_insert(0);
        let slug = if *count == 0 {
            base.clone()
        } else {
            format!("{base}-{count}")
        };
        *count += 1;
        slug
    }
}

impl TreeBuilder {
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

    /// Container levels in the tree, counted the way [`MAX_NESTING`] counts
    /// them: a list and each of its items are a level apiece, because each
    /// opens a frame that another can nest inside.
    fn depth(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|block| match &block.kind {
                BlockKind::BlockQuote { children, .. }
                | BlockKind::FootnoteDefinition { children, .. } => 1 + depth(children),
                BlockKind::List { items, .. } => {
                    1 + items
                        .iter()
                        .map(|item| 1 + depth(&item.children))
                        .max()
                        .unwrap_or(0)
                }
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }

    /// Every scrap of text in the tree, however deep.
    fn plain(blocks: &[Block]) -> String {
        fn walk(blocks: &[Block], out: &mut String) {
            for block in blocks {
                match &block.kind {
                    BlockKind::Paragraph(content) | BlockKind::Heading { content, .. } => {
                        out.push_str(&Inline::plain_text(content));
                    }
                    BlockKind::BlockQuote { children, .. }
                    | BlockKind::FootnoteDefinition { children, .. } => walk(children, out),
                    BlockKind::List { items, .. } => {
                        for item in items {
                            walk(&item.children, out);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut out = String::new();
        walk(blocks, &mut out);
        out
    }

    #[test]
    fn nesting_past_the_cap_is_flattened_rather_than_followed() {
        // 12,000 levels used to abort. Layout recurses once per level, and a
        // stack overflow does not unwind — so the reader died with the screen
        // still on the alternate buffer and the cursor still hidden, and the
        // document need not be the reader's own to do it.
        for source in [
            "> ".repeat(12_000),
            "> - ".repeat(12_000),
            "- ".repeat(12_000),
        ] {
            let blocks = parse(&format!("{source}deep\n"));
            assert_eq!(
                depth(&blocks),
                MAX_NESTING,
                "capped at MAX_NESTING and no deeper"
            );
            assert!(
                plain(&blocks).contains("deep"),
                "flattened, not truncated: the text inside still has to arrive"
            );
        }
    }

    #[test]
    fn nesting_within_the_cap_is_left_exactly_as_written() {
        // The cap has to be invisible to any document anyone would write.
        for levels in [1usize, 2, 8, 64] {
            let blocks = parse(&format!("{}deep\n", "> ".repeat(levels)));
            assert_eq!(depth(&blocks), levels);
        }
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
            ("- <b>HTML lead.</b> Rest.\n", "HTML lead. Rest."),
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
    fn math_becomes_code_rather_than_literal_delimiters() {
        let blocks = parse("Inline $E = mc^2$ here.\n");
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("expected paragraph");
        };
        // The delimiters must be consumed, not printed: the failure this
        // guards is `$E = mc^2$` reaching the page verbatim, which is what
        // happens the moment `ENABLE_MATH` is dropped from the option set.
        assert!(
            content
                .iter()
                .any(|i| matches!(i, Inline::Code(t) if t == "E = mc^2"))
        );
        assert!(
            !Inline::plain_text(content).contains('$'),
            "math delimiters reached the plain mirror"
        );
    }

    #[test]
    fn display_math_becomes_code_too() {
        let blocks = parse("$$\n\\int_0^1 x^2 dx\n$$\n");
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("expected paragraph, got {:?}", blocks[0].kind);
        };
        let plain = Inline::plain_text(content);
        assert!(plain.contains("\\int_0^1 x^2 dx"), "got {plain:?}");
        assert!(!plain.contains('$'), "got {plain:?}");
    }

    fn parse_html(source: &str, html: HtmlMode) -> Vec<Block> {
        parse_with(source, ParseOptions { html })
    }

    #[test]
    fn literal_html_is_preserved() {
        let blocks = parse_html("<div>\nhtml\n</div>\n", HtmlMode::Literal);
        assert!(
            blocks
                .iter()
                .any(|b| matches!(&b.kind, BlockKind::Html(h) if h.contains("div")))
        );
    }

    #[test]
    fn an_html_block_is_one_block_not_one_per_line() {
        // The reported bug: pulldown emits one `Event::Html` per source line,
        // so without a frame to collect them every line became its own block
        // and the layout put a blank line between each.
        let source = "<div>\nline one\nline two\nline three\n</div>\n";
        let blocks = parse_html(source, HtmlMode::Literal);
        let html: Vec<_> = blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::Html(_)))
            .collect();
        assert_eq!(html.len(), 1, "one block per HTML block: {blocks:#?}");
        let BlockKind::Html(text) = &html[0].kind else {
            unreachable!()
        };
        assert_eq!(text.lines().count(), 5, "every source line is kept");
    }

    #[test]
    fn an_html_heading_joins_the_block_tree() {
        let blocks = parse("<h1 align=\"center\">Title</h1>\n");
        let [block] = blocks.as_slice() else {
            panic!("one block, got {blocks:#?}");
        };
        let BlockKind::Heading { level, id, content } = &block.kind else {
            panic!("a heading, got {:?}", block.kind);
        };
        assert_eq!(*level, 1);
        assert_eq!(id, "title");
        assert_eq!(Inline::plain_text(content), "Title");
        assert_eq!(block.align, Alignment::Center);
    }

    #[test]
    fn html_headings_share_the_document_wide_slug_counter() {
        // Two anchors named `setup` would send `#setup` to one of two places.
        let blocks = parse("## Setup\n\n<h2>Setup</h2>\n");
        let ids: Vec<_> = blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Heading { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["setup", "setup-1"]);
    }

    #[test]
    fn a_centered_paragraph_keeps_its_words_and_loses_its_tags() {
        let source = "<p align=\"center\">\n  A tagline<br>\n  on two lines.\n</p>\n";
        let blocks = parse(source);
        let [block] = blocks.as_slice() else {
            panic!("one block, got {blocks:#?}");
        };
        let BlockKind::Paragraph(content) = &block.kind else {
            panic!("a paragraph, got {:?}", block.kind);
        };
        assert_eq!(Inline::plain_text(content), "A tagline on two lines.");
        assert!(content.contains(&Inline::HardBreak), "<br> becomes a break");
        assert_eq!(block.align, Alignment::Center);
    }

    #[test]
    fn a_badge_link_keeps_the_page_it_points_at() {
        // `<a href=page><img src=picture>` — the image's destination must not
        // win, or every badge opens the SVG it drew instead of the page.
        let blocks =
            parse("<p><a href=\"https://ci.example\"><img alt=\"CI\" src=\"b.svg\"></a></p>\n");
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("a paragraph, got {:?}", blocks[0].kind);
        };
        let [Inline::Link { dest, content }] = content.as_slice() else {
            panic!("one link, got {content:#?}");
        };
        assert_eq!(dest, "https://ci.example");
        assert_eq!(Inline::plain_text(content), "CI");
    }

    #[test]
    fn unrecognized_html_falls_back_to_literal_markup() {
        // A list read as one run-on sentence is worse than one read as tags:
        // the markers carry the structure, and there is no emitter behind
        // them to draw it.
        let blocks = parse("<ul>\n<li>one</li>\n<li>two</li>\n</ul>\n");
        assert!(
            blocks
                .iter()
                .any(|b| matches!(&b.kind, BlockKind::Html(h) if h.contains("<ul>"))),
            "{blocks:#?}"
        );
    }

    #[test]
    fn an_html_table_reaches_the_tree_as_a_table() {
        // The same block a pipe table produces, so the one emitter draws both.
        let source = "<table>\n<tr><th>H</th></tr>\n<tr><td>a</td></tr>\n</table>\n";
        let blocks = parse(source);
        let [block] = blocks.as_slice() else {
            panic!("one block, got {blocks:#?}");
        };
        let BlockKind::Table {
            alignments,
            header,
            rows,
        } = &block.kind
        else {
            panic!("a table, got {:?}", block.kind);
        };
        assert_eq!(alignments.len(), 1);
        assert_eq!(Inline::plain_text(&header[0]), "H");
        assert_eq!(rows.len(), 1);
        assert_eq!(Inline::plain_text(&rows[0][0]), "a");
        // The other two modes are untouched by interpretation.
        assert!(matches!(
            parse_html(source, HtmlMode::Literal).as_slice(),
            [Block {
                kind: BlockKind::Html(_),
                ..
            }]
        ));
        assert!(parse_html(source, HtmlMode::Hide).is_empty());
    }

    #[test]
    fn a_script_is_never_interpreted_as_prose() {
        let blocks = parse("<script>alert('x')</script>\n");
        assert!(
            blocks.iter().all(|b| matches!(b.kind, BlockKind::Html(_))),
            "{blocks:#?}"
        );
    }

    #[test]
    fn hiding_html_drops_the_block_entirely() {
        let blocks = parse_html("before\n\n<div>gone</div>\n\nafter\n", HtmlMode::Hide);
        assert_eq!(blocks.len(), 2, "{blocks:#?}");
        assert!(
            blocks
                .iter()
                .all(|b| matches!(b.kind, BlockKind::Paragraph(_)))
        );
    }

    #[test]
    fn inline_html_styles_the_text_between_the_tags() {
        let blocks = parse("Inline <b>HTML</b> too.\n");
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("a paragraph, got {:?}", blocks[0].kind);
        };
        assert_eq!(Inline::plain_text(content), "Inline HTML too.");
        assert!(
            content.iter().any(|i| matches!(i, Inline::Strong(_))),
            "{content:#?}"
        );
    }

    #[test]
    fn an_unclosed_inline_tag_never_loses_its_text() {
        // The existing policy for malformed input: styling may be lost, text
        // may not.
        let blocks = parse("Text with <b>an unclosed tag.\n");
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("a paragraph, got {:?}", blocks[0].kind);
        };
        assert_eq!(Inline::plain_text(content), "Text with an unclosed tag.");
    }

    #[test]
    fn hiding_inline_html_keeps_the_text_between_the_tags() {
        let blocks = parse_html("Inline <b>HTML</b> too.\n", HtmlMode::Hide);
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("a paragraph, got {:?}", blocks[0].kind);
        };
        assert_eq!(Inline::plain_text(content), "Inline HTML too.");
    }

    #[test]
    fn literal_inline_html_shows_the_markup() {
        let blocks = parse_html("Inline <b>HTML</b> too.\n", HtmlMode::Literal);
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("a paragraph, got {:?}", blocks[0].kind);
        };
        assert_eq!(Inline::plain_text(content), "Inline <b>HTML</b> too.");
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
        // Two pipe tables, plus the fixture's HTML table and the half of a
        // blank-line-split one that carries rows.
        assert_eq!(table_count, 4, "tables");
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
