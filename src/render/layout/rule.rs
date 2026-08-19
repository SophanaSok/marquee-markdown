//! Thematic break: a hairline across the content column.

use std::ops::Range;

use ratatui::text::Span;

use super::Context;
use crate::render::doc::LineKind;

pub(super) fn emit(ctx: &mut Context<'_>, span: &Range<usize>) {
    let rule = Span::styled("─".repeat(ctx.available_width()), ctx.theme.rule());
    let lead = ctx.lead();
    ctx.sink
        .push_spans(lead, vec![rule], LineKind::Rule, Some(span.clone()));
}
