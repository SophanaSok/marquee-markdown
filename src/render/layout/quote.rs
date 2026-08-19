//! Blockquote emitter: an accent gutter bar, muted text, and GFM alert
//! callouts with an icon-and-title head line.

use std::ops::Range;

use ratatui::text::Span;

use super::{Context, PrefixPart};
use crate::render::block::{AlertKind, Block};
use crate::render::doc::LineKind;

pub(super) fn emit(
    ctx: &mut Context<'_>,
    alert: Option<AlertKind>,
    children: &[Block],
    span: &Range<usize>,
) {
    let bar_style = match alert {
        Some(kind) => ctx.theme.alert(kind),
        None => ctx.theme.quote_bar(),
    };
    let bar = Span::styled("▎ ", bar_style);
    ctx.prefix.push(PrefixPart::new(bar.clone(), bar));

    if let Some(kind) = alert {
        // Callout head: icon + title in the alert color, truncated to fit the
        // column so a narrow terminal cannot overflow the line.
        let avail = ctx.available_width();
        let text = crate::render::measure::truncate(
            &format!("{} {}", ctx.theme.alert_icon(kind), kind.title()),
            avail,
            "\u{2026}",
        );
        let head = Span::styled(text, ctx.theme.alert(kind));
        let lead = ctx.lead();
        ctx.sink
            .push_spans(lead, vec![head], LineKind::Quote, Some(span.clone()));
    }

    // Children render with the quote's muted text as their base style via a
    // scoped theme override; simplest faithful approach is to emit paragraphs
    // through the shared styled path.
    for (i, block) in children.iter().enumerate() {
        if i > 0 || alert.is_some() {
            // Spacing line still carries the bar — all lead, no content.
            let lead = ctx.lead_rest();
            ctx.sink
                .push_spans(lead, Vec::new(), LineKind::Quote, Some(span.clone()));
        }
        match &block.kind {
            crate::render::block::BlockKind::Paragraph(content) => {
                super::para::emit_styled(
                    ctx,
                    content,
                    ctx.theme.quote_text(),
                    LineKind::Quote,
                    &block.span,
                );
            }
            _ => ctx.blocks(std::slice::from_ref(block)),
        }
    }

    ctx.prefix.pop();
}
