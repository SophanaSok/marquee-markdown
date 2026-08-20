//! Paragraph emitter: fragment, wrap, emit.

use std::ops::Range;

use super::Context;
use crate::render::block::Inline;
use crate::render::doc::LineKind;
use crate::render::frag::{self, Breaks, LinkSink};
use crate::render::wrap::{self, WrapMode};

pub(super) fn emit(ctx: &mut Context<'_>, content: &[Inline], span: &Range<usize>) {
    emit_styled(ctx, content, ctx.theme.body(), LineKind::Body, span);
}

/// Shared body for paragraphs and quote text, which differ only in base style.
pub(super) fn emit_styled(
    ctx: &mut Context<'_>,
    content: &[Inline],
    base: ratatui::style::Style,
    kind: LineKind,
    span: &Range<usize>,
) {
    struct Sink<'a>(&'a mut crate::render::sink::LineSink);
    impl LinkSink for Sink<'_> {
        fn intern(&mut self, dest: &str) -> u32 {
            self.0.intern_link(dest)
        }
    }
    let frags = {
        let mut links = Sink(ctx.sink);
        let breaks = if ctx.options.preserve_new_lines {
            Breaks::Preserve
        } else {
            Breaks::Collapse
        };
        frag::fragment(content, base, ctx.theme, &mut links, breaks)
    };
    let avail = ctx.available_width();
    for line in wrap::wrap(frags, avail, WrapMode::Word) {
        let mut lead = ctx.lead();
        lead.extend(ctx.align_pad(&line));
        ctx.sink.push_frags(lead, &line, kind, Some(span.clone()));
    }
}
