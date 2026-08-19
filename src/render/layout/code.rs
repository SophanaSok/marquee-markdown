//! Code block emitter: the rounded container.
//!
//! ```text
//! ╭─ rust ─────────────────────╮
//! │ fn main() {                │
//! │     println!("hi");        │
//! ╰────────────────────────────╯
//! ```
//!
//! This module is the only emitter of code lines, and every interior line is
//! built as `│␣` + content padded to the interior width + `␣│` — exactly the
//! available width. Long lines wrap *inside* the container; nothing downstream
//! can widen a line, so escape is structurally impossible.

use std::ops::Range;

use ratatui::text::Span;

use super::Context;
use crate::render::doc::LineKind;
use crate::render::frag::{Frag, FragKind};
use crate::render::highlight;
use crate::render::measure;
use crate::render::wrap::{self, WrapMode};

pub(super) fn emit(ctx: &mut Context<'_>, language: Option<&str>, text: &str, span: &Range<usize>) {
    let avail = ctx.available_width();
    // Degenerate widths: fall back to plain hard-wrapped text.
    if avail < 8 {
        return emit_bare(ctx, text, span);
    }
    let interior = avail - 4; // "│ " … " │"
    let border = ctx.theme.code_border();
    let fill = ctx.theme.code_fill();
    let block_index = ctx.code_blocks_emitted;
    ctx.code_blocks_emitted += 1;

    // Line-number gutter inside the container.
    let source_lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.lines().collect()
    };
    let gutter = if ctx.options.code_line_numbers {
        source_lines.len().to_string().len() + 1
    } else {
        0
    };
    let code_width = interior.saturating_sub(gutter).max(1);

    // Top border with the language label.
    let top = match language {
        Some(lang) if !lang.is_empty() => {
            let label = measure::truncate(lang, interior.saturating_sub(4), "…");
            let label_w = measure::width(&label);
            let dashes = avail.saturating_sub(label_w + 5);
            vec![
                Span::styled("╭─ ", border),
                Span::styled(label, ctx.theme.code_label()),
                Span::styled(format!(" {}╮", "─".repeat(dashes)), border),
            ]
        }
        _ => vec![Span::styled(format!("╭{}╮", "─".repeat(avail - 2)), border)],
    };
    push_row(ctx, top, LineKind::CodeBorder { block: block_index }, span);

    // Body: highlighted spans per source line, wrapped hard at the column.
    let highlighted = highlight::highlight(text, language, ctx.theme);
    for (i, line_spans) in highlighted.into_iter().enumerate() {
        let frags: Vec<Frag> = line_spans
            .into_iter()
            .map(|(style, piece)| Frag {
                width: measure::width(&piece),
                text: piece,
                style,
                link: None,
                kind: FragKind::Word,
            })
            .collect();
        let wrapped = wrap::wrap(frags, code_width, WrapMode::HardAtColumn);
        for (j, seg) in wrapped.into_iter().enumerate() {
            let mut spans = vec![Span::styled("│ ", border)];
            if gutter > 0 {
                let num = if j == 0 {
                    format!("{:>w$} ", i + 1, w = gutter - 1)
                } else {
                    " ".repeat(gutter)
                };
                spans.push(Span::styled(num, ctx.theme.code_label().patch(fill)));
            }
            let mut used = 0;
            for frag in &seg {
                used += frag.width;
                spans.push(Span::styled(
                    frag.text.clone(),
                    frag.style.patch(fill_bg(fill)),
                ));
            }
            if used < code_width {
                spans.push(Span::styled(" ".repeat(code_width - used), fill));
            }
            spans.push(Span::styled(" │", border));
            push_row(ctx, spans, LineKind::Code { block: block_index }, span);
        }
    }

    // Bottom border.
    let bottom = vec![Span::styled(format!("╰{}╯", "─".repeat(avail - 2)), border)];
    push_row(
        ctx,
        bottom,
        LineKind::CodeBorder { block: block_index },
        span,
    );
}

fn push_row(ctx: &mut Context<'_>, row: Vec<Span<'static>>, kind: LineKind, span: &Range<usize>) {
    let mut spans = ctx.lead();
    spans.extend(row);
    ctx.sink.push_spans(spans, kind, Some(span.clone()));
}

/// Width too small for a container: plain hard-wrapped code text.
fn emit_bare(ctx: &mut Context<'_>, text: &str, span: &Range<usize>) {
    let style = ctx.theme.code_fill();
    let avail = ctx.available_width();
    for source_line in text.lines() {
        let frag = Frag {
            text: source_line.to_owned(),
            style,
            link: None,
            width: measure::width(source_line),
            kind: FragKind::Word,
        };
        for seg in wrap::wrap(vec![frag], avail, WrapMode::HardAtColumn) {
            let lead = ctx.lead();
            ctx.sink
                .push_frags(lead, &seg, LineKind::Code { block: 0 }, Some(span.clone()));
        }
    }
}

/// Extract just the background of the fill style for patching onto highlight
/// spans, so syntax foregrounds survive but the surface bg always wins.
fn fill_bg(fill: ratatui::style::Style) -> ratatui::style::Style {
    ratatui::style::Style::new().bg(fill.bg.unwrap_or(ratatui::style::Color::Reset))
}
