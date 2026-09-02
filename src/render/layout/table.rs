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
use crate::render::frag::{self, Breaks, Frag, FragKind, IgnoreLinks, LinkSink};
use crate::render::measure;
use crate::render::wrap::{self, WrapMode};
use crate::theme::Theme;

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
    // A framed table is a fixed-width object, so it is set against an edge as
    // a whole rather than line by line: the alignment leaves the context and
    // becomes one pad in front of every line. Cards are ordinary wrapped text
    // and would otherwise centre each value line behind its own label.
    let outer = std::mem::replace(&mut ctx.align, Alignment::Left);
    let avail = ctx.available_width();
    // Frame chrome: k+1 borders plus 2 padding cells per column.
    let chrome = 3 * columns + 1;
    // Frame the table only when every column's widest word can fit. Below
    // that, a framed table shreds prose into unreadable 2-character columns,
    // so stack the rows as `label: value` cards instead.
    let Some(widths) = (avail > chrome)
        .then(|| solve_widths(ctx, columns, header, rows, avail - chrome))
        .flatten()
    else {
        emit_cards(ctx, header, rows, span);
        ctx.align = outer;
        return;
    };

    let room = avail.saturating_sub(widths.iter().sum::<usize>() + chrome);
    let frame = Frame {
        indent: match outer {
            Alignment::Left => 0,
            Alignment::Center => room / 2,
            Alignment::Right => room,
        },
        widths: &widths,
        span,
    };

    let border = ctx.theme.table_border();
    let header_style = ctx.theme.table_header();
    let body_style = ctx.theme.body();
    let mid_rule = ("├", "┼", "┤");

    frame_row(ctx, &frame, ("┌", "┬", "┐"), border);
    if !header.is_empty() {
        cells_rows(
            ctx,
            &frame,
            alignments,
            header,
            (header_style, header_style),
        );
        // A header with nothing under it needs no rule under it either;
        // drawing one lays a double line along the bottom edge.
        if !rows.is_empty() {
            frame_row(ctx, &frame, mid_rule, border);
        }
    }
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            frame_row(ctx, &frame, mid_rule, border);
        }
        cells_rows(ctx, &frame, alignments, row, (body_style, ctx.theme.page()));
    }
    frame_row(ctx, &frame, ("└", "┴", "┘"), border);
    ctx.align = outer;
}

/// The fixed geometry of one table, shared by every line it emits.
struct Frame<'a> {
    /// Cells of page between the content edge and the table's left border,
    /// from the block's own alignment.
    indent: usize,
    widths: &'a [usize],
    span: &'a Range<usize>,
}

/// The lead for one table line: the container prefix, then the pad that sets
/// the table against its edge.
///
/// Part of the lead rather than the content so `LineMeta` counts it as
/// decoration, which is the same reason `Context::align_pad` gives.
fn lead_with(ctx: &mut Context<'_>, frame: &Frame<'_>) -> Vec<Span<'static>> {
    let mut lead = ctx.lead();
    if frame.indent > 0 {
        lead.push(Span::styled(" ".repeat(frame.indent), ctx.theme.page()));
    }
    lead
}

/// Longest word we insist on fitting before giving up on a framed table; one
/// pathological token should not force every table into card layout.
const MAX_MIN_COLUMN: usize = 24;

/// A cell's natural and minimum width, measured from the fragments that will
/// actually be drawn.
///
/// Measuring the cell's plain text instead looks equivalent and is not: an
/// inline code span is drawn as a padded chip, two columns wider than the text
/// it contains. The solver would hand the emitter a column too narrow for its
/// own content, and the cell would wrap into a three-line row — a blank line,
/// the text, another blank — for no reason a reader could see.
fn measure_cell(content: &[Inline], theme: &Theme) -> (usize, usize) {
    let frags = frag::fragment(
        content,
        Style::new(),
        theme,
        &mut IgnoreLinks,
        Breaks::Collapse,
    );
    // Natural is the widest *line*, not the total: a cell holding a `<br>`
    // wraps there whatever column it gets, so asking for the sum of its lines
    // would claim room no line of it uses.
    let (mut natural, mut line) = (0usize, 0usize);
    for f in &frags {
        if f.kind == FragKind::Break {
            natural = natural.max(line);
            line = 0;
        } else {
            line += f.width;
        }
    }
    natural = natural.max(line);

    // The widest run the wrapper cannot break: a word plus the glue stuck to it.
    let (mut widest, mut run) = (0usize, 0usize);
    for f in &frags {
        match f.kind {
            FragKind::Glue => run += f.width,
            FragKind::Word => {
                widest = widest.max(run);
                run = f.width;
            }
            FragKind::Space | FragKind::Break => {
                widest = widest.max(run);
                run = 0;
            }
        }
    }
    (natural, widest.max(run))
}

/// Solve per-column widths against a content budget.
///
/// Returns `None` when even the per-column minimums (each column's widest
/// unbreakable word) do not fit — the caller then falls back to cards.
fn solve_widths(
    ctx: &Context<'_>,
    columns: usize,
    header: &[Vec<Inline>],
    rows: &[Vec<Vec<Inline>>],
    budget: usize,
) -> Option<Vec<usize>> {
    let mut min = vec![1usize; columns];
    let mut nat = vec![1usize; columns];
    let mut measure_cell = |col: usize, content: &[Inline]| {
        let (cell_nat, cell_min) = measure_cell(content, ctx.theme);
        nat[col] = nat[col].max(cell_nat);
        min[col] = min[col].max(cell_min.min(MAX_MIN_COLUMN));
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
    frame: &Frame<'_>,
    (left, mid, right): (&str, &str, &str),
    border: Style,
) {
    let mut text = String::from(left);
    for (i, w) in frame.widths.iter().enumerate() {
        if i > 0 {
            text.push_str(mid);
        }
        text.push_str(&"─".repeat(w + 2));
    }
    text.push_str(right);
    let rule = Span::styled(text, border);
    let lead = lead_with(ctx, frame);
    ctx.sink
        .push_spans(lead, vec![rule], LineKind::Table, Some(frame.span.clone()));
}

/// Emit one logical row, wrapping cells and padding short ones, as one or more
/// physical lines.
fn cells_rows(
    ctx: &mut Context<'_>,
    frame: &Frame<'_>,
    alignments: &[Alignment],
    row: &[Vec<Inline>],
    (text_style, pad_style): (Style, Style),
) {
    let border = ctx.theme.table_border();
    // Wrap every cell to its column width. Links are interned as they are
    // found, so a cell of badges is walkable with `]` exactly as the same
    // links are in the narrow-width card layout.
    let wrapped: Vec<Vec<Vec<Frag>>> = (0..frame.widths.len())
        .map(|col| {
            let content = row.get(col).map(Vec::as_slice).unwrap_or(&[]);
            let frags = {
                let mut links = Links(ctx.sink);
                frag::fragment(content, text_style, ctx.theme, &mut links, Breaks::Collapse)
            };
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
            wrap::wrap(frags, frame.widths[col], WrapMode::Word)
        })
        .collect();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);

    for line_idx in 0..height {
        // Assembled as fragments rather than spans so the sink records which
        // columns a link covers; borders and padding are fragments carrying no
        // link, which is what keeps them out of the walk.
        let mut frags = vec![chrome_frag("│", border)];
        for (col, cell_lines) in wrapped.iter().enumerate() {
            frags.push(chrome_frag(" ", pad_style));
            let empty = Vec::new();
            let segment = cell_lines.get(line_idx).unwrap_or(&empty);
            let content_w = wrap::line_width(segment);
            let pad = frame.widths[col].saturating_sub(content_w);
            let (before, after) = match alignments.get(col).copied().unwrap_or_default() {
                Alignment::Left => (0, pad),
                Alignment::Right => (pad, 0),
                Alignment::Center => (pad / 2, pad - pad / 2),
            };
            if before > 0 {
                frags.push(chrome_frag(&" ".repeat(before), pad_style));
            }
            frags.extend(segment.iter().cloned());
            if after > 0 {
                frags.push(chrome_frag(&" ".repeat(after), pad_style));
            }
            frags.push(chrome_frag(" ", pad_style));
            frags.push(chrome_frag("│", border));
        }
        let lead = lead_with(ctx, frame);
        ctx.sink
            .push_frags(lead, &frags, LineKind::Table, Some(frame.span.clone()));
    }
}

/// A border or padding fragment: drawn, never part of a link, and never
/// dropped at a line end (the wrapper is done with the line by now).
fn chrome_frag(text: &str, style: Style) -> Frag {
    Frag {
        text: text.to_owned(),
        style,
        link: None,
        width: measure::width(text),
        kind: FragKind::Word,
    }
}

/// Interns the links found while fragmenting a cell.
struct Links<'a>(&'a mut crate::render::sink::LineSink);

impl LinkSink for Links<'_> {
    fn intern(&mut self, dest: &str) -> u32 {
        self.0.intern_link(dest)
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
    // A header with no rows under it has nothing to label, so it is the one
    // row there is. Labelling it with itself would print `Name: Name`.
    let only = [header.to_vec()];
    let (labels, rows) = if rows.is_empty() {
        (Vec::new(), &only[..])
    } else {
        (labels, rows)
    };

    for (row_idx, row) in rows.iter().enumerate() {
        if row_idx > 0 {
            let divider = Span::styled("\u{2500}".repeat(ctx.available_width()), ctx.theme.rule());
            let lead = ctx.lead();
            ctx.sink
                .push_spans(lead, vec![divider], LineKind::Table, Some(span.clone()));
        }
        for (col, cell) in row.iter().enumerate() {
            // An empty cell with no label to introduce it says nothing, and a
            // line is what saying nothing would cost.
            if cell.is_empty() && labels.get(col).is_none_or(String::is_empty) {
                continue;
            }
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
