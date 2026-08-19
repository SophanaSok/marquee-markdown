//! Thematic break: a hairline across the content column.

use std::ops::Range;

use ratatui::text::Span;

use super::Context;
use crate::render::doc::LineKind;

pub(super) fn emit(ctx: &mut Context<'_>, span: &Range<usize>) {
    let lead = ctx.lead();
    let mut spans = lead;
    spans.push(Span::styled(
        "─".repeat(ctx.available_width()),
        ctx.theme.rule(),
    ));
    ctx.sink
        .push_spans(spans, LineKind::Rule, Some(span.clone()));
}
