//! The layout driver: block tree + theme + width → rendered lines.
//!
//! Each block type has its own emitter module; all of them go through the
//! shared `Context`, which carries the sink, the theme, and the accumulated
//! indent from enclosing containers (quotes, list items).

mod code;
mod heading;
mod list;
mod para;
mod quote;
mod rule;
mod table;

use ratatui::text::Span;

use super::block::{Block, BlockKind};
use super::doc::{LineKind, RenderedDoc};
use super::sink::LineSink;
use crate::theme::Theme;

/// Options for one layout run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutOptions {
    /// Content column width in cells.
    pub width: u16,
    /// Show line numbers inside code blocks.
    pub code_line_numbers: bool,
    /// Keep the line breaks the author typed inside a paragraph, rather than
    /// re-wrapping it. This is glow's `-n`.
    pub preserve_new_lines: bool,
}

/// Lay out a parsed document at a fixed content width.
#[must_use]
pub fn layout(blocks: &[Block], theme: &Theme, options: LayoutOptions) -> RenderedDoc {
    let width = usize::from(options.width.max(10));
    let mut sink = LineSink::new(width, theme.page());
    let mut ctx = Context {
        sink: &mut sink,
        theme,
        options,
        prefix: Vec::new(),
        code_blocks_emitted: 0,
    };
    ctx.blocks(blocks);
    sink.finish(options.width.max(10))
}

/// Shared emitter state. `prefix` is the per-line lead accumulated from
/// enclosing containers — quote bars, list hanging indents — cloned onto every
/// line a nested block emits.
pub(super) struct Context<'a> {
    pub sink: &'a mut LineSink,
    pub theme: &'a Theme,
    pub options: LayoutOptions,
    pub prefix: Vec<PrefixPart>,
    /// Sequential index assigned to the next code block.
    pub code_blocks_emitted: u32,
}

/// One accumulated lead element.
#[derive(Debug, Clone)]
pub(super) struct PrefixPart {
    /// Rendered on the first line of the immediate block only.
    pub first: Span<'static>,
    /// Rendered on continuation lines (same display width as `first`).
    pub rest: Span<'static>,
    /// Whether this part is a list marker, for nesting-depth counting.
    pub is_list: bool,
}

impl PrefixPart {
    pub fn new(first: Span<'static>, rest: Span<'static>) -> Self {
        Self {
            first,
            rest,
            is_list: false,
        }
    }
}

impl Context<'_> {
    /// Emit a sequence of sibling blocks with spacing between them.
    pub fn blocks(&mut self, blocks: &[Block]) {
        for (i, block) in blocks.iter().enumerate() {
            if i > 0 {
                self.sink.blank();
            }
            self.block(block);
        }
    }

    /// Emit sibling blocks without inter-block spacing (tight list items).
    pub fn blocks_tight(&mut self, blocks: &[Block]) {
        for block in blocks {
            self.block(block);
        }
    }

    fn block(&mut self, block: &Block) {
        match &block.kind {
            BlockKind::Heading { level, id, content } => {
                heading::emit(self, *level, id, content, &block.span);
            }
            BlockKind::Paragraph(content) => para::emit(self, content, &block.span),
            BlockKind::CodeBlock { language, text } => {
                code::emit(self, language.as_deref(), text, &block.span);
            }
            BlockKind::BlockQuote { alert, children } => {
                quote::emit(self, *alert, children, &block.span);
            }
            BlockKind::List { start, items } => list::emit(self, *start, items, &block.span),
            BlockKind::Table {
                alignments,
                header,
                rows,
            } => {
                table::emit(self, alignments, header, rows, &block.span);
            }
            BlockKind::Rule => rule::emit(self, &block.span),
            BlockKind::Html(html) => self.html(html, &block.span),
            BlockKind::FootnoteDefinition { label, children } => {
                self.footnote(label, children, &block.span);
            }
        }
    }

    /// Raw HTML: rendered as muted literal lines rather than dropped.
    fn html(&mut self, html: &str, span: &std::ops::Range<usize>) {
        use super::frag::{Frag, FragKind};
        use super::wrap::{self, WrapMode};
        let style = self.theme.muted();
        let avail = self.available_width();
        for source_line in html.lines() {
            let frag = Frag {
                text: source_line.to_owned(),
                style,
                link: None,
                width: super::measure::width(source_line),
                kind: FragKind::Word,
            };
            for wrapped in wrap::wrap(vec![frag], avail, WrapMode::HardAtColumn) {
                let lead = self.lead();
                self.sink
                    .push_frags(lead, &wrapped, LineKind::Html, Some(span.clone()));
            }
        }
    }

    /// Footnote definition: `[label]` marker, then its blocks indented.
    fn footnote(&mut self, label: &str, children: &[Block], _span: &std::ops::Range<usize>) {
        let marker = Span::styled(
            format!("[{label}] "),
            self.theme
                .muted()
                .add_modifier(ratatui::style::Modifier::BOLD),
        );
        let pad = Span::styled(
            " ".repeat(super::measure::width(&marker.content)),
            self.theme.page(),
        );
        self.prefix.push(PrefixPart::new(marker, pad));
        self.blocks(children);
        self.prefix.pop();
    }

    /// Width remaining after the accumulated prefix.
    pub fn available_width(&self) -> usize {
        let prefix: usize = self
            .prefix
            .iter()
            .map(|p| super::measure::width(&p.rest.content))
            .sum();
        self.sink.width().saturating_sub(prefix).max(1)
    }

    /// The lead spans for the next line; first-line parts degrade to their
    /// continuation form after one use.
    pub fn lead(&mut self) -> Vec<Span<'static>> {
        let lead: Vec<Span<'static>> = self.prefix.iter().map(|p| p.first.clone()).collect();
        for part in &mut self.prefix {
            part.first = part.rest.clone();
        }
        lead
    }

    /// The continuation lead without consuming first-line parts.
    pub fn lead_rest(&self) -> Vec<Span<'static>> {
        self.prefix.iter().map(|p| p.rest.clone()).collect()
    }
}
