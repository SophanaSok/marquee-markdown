//! Heading emitter: styled text, an anchor, and hairline rules under H1/H2.
//!
//! Hierarchy comes from weight, color, and vertical rhythm — never from
//! literal `#` marks. The extra blank line above headings is the layout's
//! main breathing room.

use std::ops::Range;

use ratatui::text::Span;

use super::Context;
use crate::render::block::Inline;
use crate::render::doc::LineKind;
use crate::render::frag::{self, IgnoreLinks};
use crate::render::wrap::{self, WrapMode};

pub(super) fn emit(
    ctx: &mut Context<'_>,
    level: u8,
    id: &str,
    content: &[Inline],
    span: &Range<usize>,
) {
    // Extra space above headings (the sink collapses doubles at block starts).
    ctx.sink.blank();

    ctx.sink
        .push_anchor(level, id.to_owned(), Inline::plain_text(content));

    let style = ctx.theme.heading(level);
    let frags = frag::fragment(content, style, ctx.theme, &mut IgnoreLinks);
    let avail = ctx.available_width();
    for line in wrap::wrap(frags, avail, WrapMode::Word) {
        let lead = ctx.lead();
        ctx.sink
            .push_frags(lead, &line, LineKind::Heading(level), Some(span.clone()));
    }

    // Hairline rule beneath the two top levels.
    if level <= 2 {
        let lead = ctx.lead_rest();
        let rule = Span::styled("─".repeat(ctx.available_width()), ctx.theme.rule());
        let mut spans = lead;
        spans.push(rule);
        ctx.sink
            .push_spans(spans, LineKind::Heading(level), Some(span.clone()));
    }
}
