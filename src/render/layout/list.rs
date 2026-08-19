//! List emitter: bullets by depth, aligned ordered numerals, task checkboxes,
//! hanging indent for wrapped content.

use std::ops::Range;

use ratatui::text::Span;

use super::{Context, PrefixPart};
use crate::render::block::ListItem;
use crate::render::measure;

/// Bullet glyphs by nesting depth (cycled past the end).
const BULLETS: [&str; 3] = ["•", "◦", "▪"];

pub(super) fn emit(
    ctx: &mut Context<'_>,
    start: Option<u64>,
    items: &[ListItem],
    _span: &Range<usize>,
) {
    // Depth = how many list prefixes are already on the stack.
    let depth = ctx.prefix.iter().filter(|p| p.is_list).count();

    // Ordered lists: right-align numerals to the widest one.
    let num_width = start.map(|s| {
        let last = s + items.len().saturating_sub(1) as u64;
        format!("{last}.").len()
    });

    for (i, item) in items.iter().enumerate() {
        let marker_style = ctx.theme.list_marker();
        let marker = match (start, item.task) {
            (_, Some(done)) => {
                let glyph = if done { "☑" } else { "☐" };
                format!("{glyph} ")
            }
            (Some(s), None) => {
                let w = num_width.unwrap_or(2);
                format!("{:>w$} ", format!("{}.", s + i as u64))
            }
            (None, None) => format!("{} ", BULLETS[depth % BULLETS.len()]),
        };
        let pad = Span::styled(" ".repeat(measure::width(&marker)), ctx.theme.page());
        let mut part = PrefixPart::new(Span::styled(marker, marker_style), pad);
        part.is_list = true;
        ctx.prefix.push(part);
        ctx.blocks_tight(&item.children);
        ctx.prefix.pop();
    }
}
