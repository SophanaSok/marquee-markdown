//! Span-aware line breaking over fragment vectors.
//!
//! Greedy fill: fragments accumulate until the next would overflow, then the
//! line breaks at the last legal boundary. Styles travel with fragments, so
//! wrapping can never separate text from its styling, and widths were computed
//! at fragment construction, so wrapping never re-measures.

use unicode_segmentation::UnicodeSegmentation;

use super::frag::{Frag, FragKind};
use super::measure;

/// One wrapped line: the fragments that fit, in order, spaces trimmed at ends.
pub type WrappedLine = Vec<Frag>;

/// How to treat content that exceeds the available width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapMode {
    /// Prose: break at spaces, hard-split only atoms wider than a whole line.
    Word,
    /// Code: break exactly at the column, preserving every character.
    HardAtColumn,
}

/// Wrap fragments to `width` cells. Always returns at least one (possibly
/// empty) line so callers can rely on block presence.
#[must_use]
pub fn wrap(frags: Vec<Frag>, width: usize, mode: WrapMode) -> Vec<WrappedLine> {
    let width = width.max(1);
    match mode {
        WrapMode::Word => wrap_words(frags, width),
        WrapMode::HardAtColumn => wrap_hard(frags, width),
    }
}

fn wrap_words(frags: Vec<Frag>, width: usize) -> Vec<WrappedLine> {
    let mut lines: Vec<WrappedLine> = Vec::new();
    let mut current: WrappedLine = Vec::new();
    let mut used = 0usize;

    for frag in frags {
        match frag.kind {
            FragKind::Break => {
                flush(&mut lines, &mut current, &mut used);
                continue;
            }
            FragKind::Space if current.is_empty() => continue, // leading space
            _ => {}
        }

        if used + frag.width > width && !current.is_empty() {
            if frag.kind == FragKind::Space {
                // The break consumes the space.
                flush(&mut lines, &mut current, &mut used);
                continue;
            }
            if frag.kind == FragKind::Glue {
                // Glue may not start a line: pull its anchor cluster back.
                let take_from = glue_anchor(&current);
                if take_from > 0 {
                    let carried: Vec<Frag> = current.drain(take_from..).collect();
                    used = current.iter().map(|f| f.width).sum();
                    flush(&mut lines, &mut current, &mut used);
                    for c in carried {
                        used += c.width;
                        current.push(c);
                    }
                } else {
                    flush(&mut lines, &mut current, &mut used);
                }
            } else {
                flush(&mut lines, &mut current, &mut used);
            }
        }

        if frag.width > width {
            // Atom wider than a whole line: hard-split at grapheme boundaries.
            hard_split_into(&frag, width, &mut lines, &mut current, &mut used);
            continue;
        }
        if frag.kind == FragKind::Space && current.is_empty() {
            continue;
        }
        used += frag.width;
        current.push(frag);
    }
    flush_final(&mut lines, current);
    lines
}

fn wrap_hard(frags: Vec<Frag>, width: usize) -> Vec<WrappedLine> {
    let mut lines: Vec<WrappedLine> = Vec::new();
    let mut current: WrappedLine = Vec::new();
    let mut used = 0usize;

    for frag in frags {
        if frag.kind == FragKind::Break {
            flush(&mut lines, &mut current, &mut used);
            continue;
        }
        let mut frag = frag;
        loop {
            let room = width - used;
            if frag.width <= room {
                used += frag.width;
                current.push(frag);
                break;
            }
            let (head, head_w, tail) = measure::split_at_col(&frag.text, room);
            if head.is_empty() {
                if current.is_empty() && used == 0 {
                    // The next grapheme is wider than the entire line. Emit a
                    // one-cell marker in its place so the loop always makes
                    // progress and the width invariant still holds.
                    frag = replace_leading_grapheme(&frag, &mut current);
                    flush(&mut lines, &mut current, &mut used);
                    if frag.text.is_empty() {
                        break;
                    }
                    continue;
                }
                // A wide grapheme straddles the boundary; break first.
                flush(&mut lines, &mut current, &mut used);
                continue;
            }
            let mut head_frag = frag.clone();
            head_frag.text = head.to_owned();
            head_frag.width = head_w;
            current.push(head_frag);
            used += head_w;
            flush(&mut lines, &mut current, &mut used);
            frag.width -= head_w;
            frag.text = tail.to_owned();
        }
    }
    flush_final(&mut lines, current);
    lines
}

/// Index from which trailing frags must carry over so glue keeps its anchor:
/// the last run of `word glue…` in the line, or 0 when the whole line is one
/// unbreakable run.
fn glue_anchor(current: &[Frag]) -> usize {
    let mut idx = current.len();
    while idx > 0 {
        let prev = &current[idx - 1];
        if prev.kind == FragKind::Space {
            return idx;
        }
        idx -= 1;
    }
    0
}

fn flush(lines: &mut Vec<WrappedLine>, current: &mut WrappedLine, used: &mut usize) {
    trim_trailing_spaces(current);
    lines.push(std::mem::take(current));
    *used = 0;
}

fn flush_final(lines: &mut Vec<WrappedLine>, mut current: WrappedLine) {
    trim_trailing_spaces(&mut current);
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
}

fn trim_trailing_spaces(line: &mut WrappedLine) {
    while line.last().is_some_and(|f| f.kind == FragKind::Space) {
        line.pop();
    }
}

/// Split an over-wide atom across as many lines as needed.
fn hard_split_into(
    frag: &Frag,
    width: usize,
    lines: &mut Vec<WrappedLine>,
    current: &mut WrappedLine,
    used: &mut usize,
) {
    let mut owned = frag.text.clone();
    loop {
        let room = width - *used;
        let (head, head_w, tail) = measure::split_at_col(&owned, room);
        if head.is_empty() {
            if current.is_empty() && *used == 0 {
                // Grapheme wider than the whole line: substitute a one-cell
                // marker so we always advance. Without this the loop spins
                // forever emitting empty lines.
                let mut probe = frag.clone();
                probe.text = owned.clone();
                let remainder = replace_leading_grapheme(&probe, current);
                flush(lines, current, used);
                if remainder.text.is_empty() {
                    return;
                }
                owned = remainder.text;
                continue;
            }
            flush(lines, current, used);
            continue;
        }
        let head = head.to_owned();
        let tail = tail.to_owned();
        let mut piece = frag.clone();
        piece.text = head;
        piece.width = head_w;
        *used += head_w;
        current.push(piece);
        if tail.is_empty() {
            return;
        }
        flush(lines, current, used);
        owned = tail;
    }
}

/// Push a one-cell truncation marker standing in for `frag`'s first grapheme,
/// returning `frag` advanced past it. Used only at degenerate widths where a
/// grapheme cannot fit any line at all.
fn replace_leading_grapheme(frag: &Frag, current: &mut WrappedLine) -> Frag {
    let mut graphemes = frag.text.graphemes(true);
    let _skipped = graphemes.next();
    let rest: String = graphemes.collect();
    let mut marker = frag.clone();
    marker.text = "\u{2026}".to_owned();
    marker.width = 1;
    current.push(marker);
    let mut remainder = frag.clone();
    remainder.width = measure::width(&rest);
    remainder.text = rest;
    remainder
}

/// Total display width of a wrapped line.
#[must_use]
pub fn line_width(line: &WrappedLine) -> usize {
    line.iter().map(|f| f.width).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    fn frag(text: &str, kind: FragKind) -> Frag {
        Frag {
            text: text.to_owned(),
            style: Style::new(),
            link: None,
            width: measure::width(text),
            kind,
        }
    }

    fn words(text: &str) -> Vec<Frag> {
        let mut out = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            let is_space = rest.starts_with(' ');
            let end = rest
                .find(|c: char| (c == ' ') != is_space)
                .unwrap_or(rest.len());
            let (piece, tail) = rest.split_at(end);
            out.push(frag(
                piece,
                if is_space {
                    FragKind::Space
                } else {
                    FragKind::Word
                },
            ));
            rest = tail;
        }
        out
    }

    fn text_of(line: &WrappedLine) -> String {
        line.iter().map(|f| f.text.as_str()).collect()
    }

    #[test]
    fn short_text_stays_on_one_line() {
        let lines = wrap(words("hello world"), 20, WrapMode::Word);
        assert_eq!(lines.len(), 1);
        assert_eq!(text_of(&lines[0]), "hello world");
    }

    #[test]
    fn wraps_at_word_boundary() {
        let lines = wrap(words("alpha beta gamma"), 11, WrapMode::Word);
        let texts: Vec<_> = lines.iter().map(text_of).collect();
        assert_eq!(texts, ["alpha beta", "gamma"]);
    }

    #[test]
    fn no_line_exceeds_width() {
        for w in 1..30 {
            let lines = wrap(
                words("the quick brown fox jumps over the lazy dog 日本語テキスト"),
                w,
                WrapMode::Word,
            );
            for line in &lines {
                assert!(line_width(line) <= w, "width {w}: {:?}", text_of(line));
            }
        }
    }

    #[test]
    fn overlong_atom_is_hard_split() {
        let lines = wrap(words("supercalifragilistic"), 8, WrapMode::Word);
        assert!(lines.len() >= 3);
        let rejoined: String = lines.iter().map(text_of).collect();
        assert_eq!(rejoined, "supercalifragilistic");
    }

    #[test]
    fn hard_break_forces_new_line() {
        let mut frags = words("one");
        frags.push(frag("", FragKind::Break));
        frags.extend(words("two"));
        let lines = wrap(frags, 40, WrapMode::Word);
        let texts: Vec<_> = lines.iter().map(text_of).collect();
        assert_eq!(texts, ["one", "two"]);
    }

    #[test]
    fn glue_carries_its_anchor_to_the_next_line() {
        // "xx `code`" where the closing pad is glue: if the chip must wrap, the
        // glued run moves as one unit.
        let mut frags = words("aaaa bbbb");
        frags.push(frag(" ", FragKind::Space));
        frags.push(frag("chip", FragKind::Word));
        frags.push(frag("!", FragKind::Glue));
        let lines = wrap(frags, 12, WrapMode::Word);
        let texts: Vec<_> = lines.iter().map(text_of).collect();
        assert_eq!(texts, ["aaaa bbbb", "chip!"], "glue split from its word");
    }

    #[test]
    fn code_mode_breaks_exactly_at_column_and_loses_nothing() {
        let src = "let this_is_a_long_line = vec![1, 2, 3];";
        let lines = wrap(vec![frag(src, FragKind::Word)], 10, WrapMode::HardAtColumn);
        for line in &lines {
            assert!(line_width(line) <= 10);
        }
        let rejoined: String = lines.iter().map(text_of).collect();
        assert_eq!(rejoined, src);
    }

    #[test]
    fn code_mode_never_bisects_wide_graphemes() {
        let lines = wrap(
            vec![frag("日本語のテキスト", FragKind::Word)],
            5,
            WrapMode::HardAtColumn,
        );
        for line in &lines {
            assert!(line_width(line) <= 5);
        }
        let rejoined: String = lines.iter().map(text_of).collect();
        assert_eq!(rejoined, "日本語のテキスト");
    }

    #[test]
    fn wide_graphemes_terminate_at_width_one() {
        // Regression: a double-width grapheme can never fit a 1-cell line.
        // The wrapper must substitute a marker rather than spin forever.
        for mode in [WrapMode::Word, WrapMode::HardAtColumn] {
            let lines = wrap(vec![frag("日本語", FragKind::Word)], 1, mode);
            assert!(!lines.is_empty());
            for line in &lines {
                assert!(line_width(line) <= 1, "{mode:?} produced an over-wide line");
            }
        }
    }

    #[test]
    fn mixed_width_text_terminates_at_every_narrow_width() {
        for w in 1..6 {
            for mode in [WrapMode::Word, WrapMode::HardAtColumn] {
                let lines = wrap(words("a 日本 bb 語"), w, mode);
                for line in &lines {
                    assert!(line_width(line) <= w, "width {w} {mode:?}");
                }
            }
        }
    }

    #[test]
    fn empty_input_yields_one_empty_line() {
        let lines = wrap(Vec::new(), 10, WrapMode::Word);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].is_empty());
    }

    #[test]
    fn trailing_spaces_are_trimmed() {
        let lines = wrap(words("word   "), 20, WrapMode::Word);
        assert_eq!(text_of(&lines[0]), "word");
    }
}
