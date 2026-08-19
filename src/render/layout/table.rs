//! Table emitter: box-drawn frame, shaded header band, content-derived column
//! widths, honored alignment, wrapping cells.
//!
//! Column solver: per column take `min` (widest unbreakable word) and `nat`
//! (widest single-line cell). If natural widths fit, use them and left-align
//! the table. If only minimums fit, water-fill the slack proportionally to
//! each column's deficit. If even minimums overflow, shrink proportionally —
//! the table never exceeds the content column; cells wrap harder instead.

use std::ops::Range;

use ratatui::style::Style;
use ratatui::text::Span;

use super::Context;
use crate::render::block::{Alignment, Inline};
use crate::render::doc::LineKind;
use crate::render::frag::{self, Breaks, Frag, FragKind, IgnoreLinks};
use crate::render::measure;
use crate::render::wrap::{self, WrapMode};

pub(super) fn emit(
    ctx: &mut Context<'_>,
    alignments: &[Alignment],
    header: &[Vec<Inline>],
    rows: &[Vec<Vec<Inline>>],
    span: &Range<usize>,
) {
    let columns = header
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0));
    if columns == 0 {
        return;
    }
    let avail = ctx.available_width();
    // Frame chrome: k+1 borders plus 2 padding cells per column.
    let chrome = 3 * columns + 1;
    // Frame the table only when every column's widest word can fit. Below
    // that, a framed table shreds prose into unreadable 2-character columns,
    // so stack the rows as `label: value` cards instead.
    let Some(widths) = (avail > chrome)
        .then(|| solve_widths(columns, header, rows, avail - chrome))
        .flatten()
    else {
        return emit_cards(ctx, header, rows, span);
    };

    let border = ctx.theme.table_border();
    let header_style = ctx.theme.table_header();
    let body_style = ctx.theme.body();

    frame_row(ctx, &widths, "┌", "┬", "┐", border, span);
    if !header.is_empty() {
        cells_rows(
            ctx,
            &widths,
            alignments,
            header,
            header_style,
            header_style,
            span,
        );
        frame_row(ctx, &widths, "├", "┼", "┤", border, span);
    }
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            frame_row(ctx, &widths, "├", "┼", "┤", border, span);
        }
        cells_rows(
            ctx,
            &widths,
            alignments,
            row,
            body_style,
            ctx.theme.page(),
            span,
        );
    }
    frame_row(ctx, &widths, "└", "┴", "┘", border, span);
}

/// Longest word we insist on fitting before giving up on a framed table; one
/// pathological token should not force every table into card layout.
const MAX_MIN_COLUMN: usize = 24;

/// Solve per-column widths against a content budget.
///
/// Returns `None` when even the per-column minimums (each column's widest
/// unbreakable word) do not fit — the caller then falls back to cards.
fn solve_widths(
    columns: usize,
    header: &[Vec<Inline>],
    rows: &[Vec<Vec<Inline>>],
    budget: usize,
) -> Option<Vec<usize>> {
    let mut min = vec![1usize; columns];
    let mut nat = vec![1usize; columns];
    let mut measure_cell = |col: usize, content: &[Inline]| {
        let text = Inline::plain_text(content);
        nat[col] = nat[col].max(measure::width(&text));
        let widest_word = text
            .split_whitespace()
            .map(measure::width)
            .max()
            .unwrap_or(0);
        min[col] = min[col].max(widest_word.min(MAX_MIN_COLUMN));
    };
    for (col, cell) in header.iter().enumerate() {
        measure_cell(col, cell);
    }
    for row in rows {
        for (col, cell) in row.iter().enumerate() {
            measure_cell(col, cell);
        }
    }

    let nat_sum: usize = nat.iter().sum();
    if nat_sum <= budget {
        return Some(nat);
    }
    let min_sum: usize = min.iter().sum();
    if min_sum > budget {
        return None;
    }
    {
        // Water-fill the slack proportionally to each column's deficit.
        let slack = budget - min_sum;
        let deficit_sum: usize = nat
            .iter()
            .zip(&min)
            .map(|(n, m)| n.saturating_sub(*m))
            .sum::<usize>()
            .max(1);
        let mut widths: Vec<usize> = min
            .iter()
            .zip(&nat)
            .map(|(m, n)| m + slack * n.saturating_sub(*m) / deficit_sum)
            .collect();
        // Hand rounding remainders to the largest-deficit columns first.
        let mut used: usize = widths.iter().sum();
        let mut order: Vec<usize> = (0..columns).collect();
        order.sort_by_key(|&c| std::cmp::Reverse(nat[c].saturating_sub(widths[c])));
        let mut i = 0;
        while used < budget && !order.is_empty() {
            let c = order[i % order.len()];
            if widths[c] < nat[c] {
                widths[c] += 1;
                used += 1;
            } else if order.iter().all(|&c| widths[c] >= nat[c]) {
                break;
            }
            i += 1;
        }
        Some(widths)
    }
}

/// A horizontal frame line: `├───┼─────┤` etc.
fn frame_row(
    ctx: &mut Context<'_>,
    widths: &[usize],
    left: &str,
    mid: &str,
    right: &str,
    border: Style,
    span: &Range<usize>,
) {
    let mut text = String::from(left);
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            text.push_str(mid);
        }
        text.push_str(&"─".repeat(w + 2));
    }
    text.push_str(right);
    let mut spans = ctx.lead();
    spans.push(Span::styled(text, border));
    ctx.sink
        .push_spans(spans, LineKind::Table, Some(span.clone()));
}

/// Emit one logical row, wrapping cells and padding short ones, as one or more
/// physical lines.
fn cells_rows(
    ctx: &mut Context<'_>,
    widths: &[usize],
    alignments: &[Alignment],
    row: &[Vec<Inline>],
    text_style: Style,
    pad_style: Style,
    span: &Range<usize>,
) {
    let border = ctx.theme.table_border();
    // Wrap every cell to its column width.
    let wrapped: Vec<Vec<Vec<Frag>>> = (0..widths.len())
        .map(|col| {
            let content = row.get(col).map(Vec::as_slice).unwrap_or(&[]);
            let frags = frag::fragment(
                content,
                text_style,
                ctx.theme,
                &mut IgnoreLinks,
                Breaks::Collapse,
            );
            // Cell text inherits the row background (header band vs page).
            let frags: Vec<Frag> = frags
                .into_iter()
                .map(|mut f| {
                    if f.kind != FragKind::Glue {
                        f.style = f.style.patch(bg_of(pad_style));
                    }
                    f
                })
                .collect();
            wrap::wrap(frags, widths[col], WrapMode::Word)
        })
        .collect();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);

    for line_idx in 0..height {
        let mut spans = ctx.lead();
        spans.push(Span::styled("│", border));
        for (col, cell_lines) in wrapped.iter().enumerate() {
            spans.push(Span::styled(" ", pad_style));
            let empty = Vec::new();
            let segment = cell_lines.get(line_idx).unwrap_or(&empty);
            let content_w = wrap::line_width(segment);
            let pad = widths[col].saturating_sub(content_w);
            let (before, after) = match alignments.get(col).copied().unwrap_or_default() {
                Alignment::Left => (0, pad),
                Alignment::Right => (pad, 0),
                Alignment::Center => (pad / 2, pad - pad / 2),
            };
            if before > 0 {
                spans.push(Span::styled(" ".repeat(before), pad_style));
            }
            for f in segment {
                spans.push(Span::styled(f.text.clone(), f.style));
            }
            if after > 0 {
                spans.push(Span::styled(" ".repeat(after), pad_style));
            }
            spans.push(Span::styled(" ", pad_style));
            spans.push(Span::styled("│", border));
        }
        ctx.sink
            .push_spans(spans, LineKind::Table, Some(span.clone()));
    }
}

fn bg_of(style: Style) -> Style {
    Style::new().bg(style.bg.unwrap_or(ratatui::style::Color::Reset))
}

/// Narrow-width fallback: render each row as a stack of `label: value` lines.
///
/// Used when the framed table cannot fit even one cell per column. Nothing is
/// lost — the same content is simply laid out vertically.
fn emit_cards(
    ctx: &mut Context<'_>,
    header: &[Vec<Inline>],
    rows: &[Vec<Vec<Inline>>],
    span: &Range<usize>,
) {
    let labels: Vec<String> = header.iter().map(|c| Inline::plain_text(c)).collect();

    for (row_idx, row) in rows.iter().enumerate() {
        if row_idx > 0 {
            let mut spans = ctx.lead();
            spans.push(Span::styled(
                "\u{2500}".repeat(ctx.available_width()),
                ctx.theme.rule(),
            ));
            ctx.sink
                .push_spans(spans, LineKind::Table, Some(span.clone()));
        }
        for (col, cell) in row.iter().enumerate() {
            let width = ctx.available_width();
            let label = labels.get(col).map_or_else(String::new, |l| {
                measure::truncate(l, width.saturating_sub(2).max(1) / 2, "\u{2026}")
            });
            if !label.is_empty() {
                let text = format!("{label}: ");
                let pad = Span::styled(" ".repeat(measure::width(&text)), ctx.theme.page());
                let part = super::PrefixPart::new(
                    Span::styled(
                        text,
                        ctx.theme.table_header().patch(bg_of(ctx.theme.page())),
                    ),
                    pad,
                );
                ctx.prefix.push(part);
            }
            super::para::emit_styled(ctx, cell, ctx.theme.body(), LineKind::Table, span);
            if !label.is_empty() {
                ctx.prefix.pop();
            }
        }
    }
}
