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
use crate::render::frag::{self, Breaks, IgnoreLinks};
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
    let frags = // A heading is one line whatever the author typed.
    frag::fragment(content, style, ctx.theme, &mut IgnoreLinks, Breaks::Collapse);
    let avail = ctx.available_width();
    for line in wrap::wrap(frags, avail, WrapMode::Word) {
        let lead = ctx.lead();
        ctx.sink
            .push_frags(lead, &line, LineKind::Heading(level), Some(span.clone()));
    }

    // Hairline rule beneath the two top levels.
    if level <= 2 {
        let rule = Span::styled("─".repeat(ctx.available_width()), ctx.theme.rule());
        let lead = ctx.lead_rest();
        ctx.sink.push_spans(
            lead,
            vec![rule],
            LineKind::Heading(level),
            Some(span.clone()),
        );
    }
}
