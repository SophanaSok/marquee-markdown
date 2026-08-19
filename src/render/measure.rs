//! The single width chokepoint.
//!
//! Every display-width decision in the crate goes through this module. Nothing
//! else may call `unicode_width` directly: the whole layout design rests on the
//! invariant that every emitted line is exactly the content width, and that only
//! holds if all width math agrees with itself.
//!
//! Widths are measured over grapheme clusters, not chars — a combining sequence
//! or emoji ZWJ family is one cluster with one width, and splitting inside one
//! would tear it apart.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Display width of one grapheme cluster in terminal cells.
///
/// Control characters measure 0 (the renderer never emits them; frag
/// construction strips them), and everything else defers to Unicode
/// width tables over the whole cluster.
#[must_use]
pub fn grapheme_width(grapheme: &str) -> usize {
    UnicodeWidthStr::width(grapheme)
}

/// Display width of a string in terminal cells.
///
/// The input must be display text only — escape sequences are structurally
/// unrepresentable in the render pipeline, so there is nothing to skip here.
#[must_use]
pub fn width(text: &str) -> usize {
    text.graphemes(true).map(grapheme_width).sum()
}

/// Split `text` at the last grapheme boundary whose prefix width is `<= cols`.
///
/// Returns `(head, head_width, tail)`. A wide grapheme that would straddle the
/// boundary goes entirely to the tail, so `head_width` can be `cols - 1` when
/// the split lands next to a double-width cluster.
#[must_use]
pub fn split_at_col(text: &str, cols: usize) -> (&str, usize, &str) {
    let mut used = 0;
    for (idx, grapheme) in text.grapheme_indices(true) {
        let w = grapheme_width(grapheme);
        if used + w > cols {
            return (&text[..idx], used, &text[idx..]);
        }
        used += w;
    }
    (text, used, "")
}

/// Truncate `text` to at most `cols` cells, appending `ellipsis` when anything
/// was cut. The result never exceeds `cols` even after the ellipsis is added.
#[must_use]
pub fn truncate(text: &str, cols: usize, ellipsis: &str) -> String {
    if width(text) <= cols {
        return text.to_owned();
    }
    let ell_w = width(ellipsis);
    let budget = cols.saturating_sub(ell_w);
    let (head, _, _) = split_at_col(text, budget);
    let mut out = head.to_owned();
    out.push_str(ellipsis);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_widths() {
        assert_eq!(width("hello"), 5);
        assert_eq!(width(""), 0);
    }

    #[test]
    fn cjk_is_double_width() {
        assert_eq!(width("日本語"), 6);
        assert_eq!(width("a日b"), 4);
    }

    #[test]
    fn combining_marks_do_not_add_width() {
        // "e" + U+0301 combining acute is one cluster, one cell.
        assert_eq!(width("e\u{301}"), 1);
        // Precomposed é likewise.
        assert_eq!(width("\u{e9}"), 1);
    }

    #[test]
    fn emoji_zwj_sequence_is_one_cluster() {
        // Family emoji: four codepoints joined by ZWJ = one grapheme.
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}";
        assert_eq!(family.graphemes(true).count(), 1);
        assert_eq!(width(family), grapheme_width(family));
    }

    #[test]
    fn split_lands_on_grapheme_boundary() {
        let (head, w, tail) = split_at_col("hello world", 5);
        assert_eq!((head, w, tail), ("hello", 5, " world"));
    }

    #[test]
    fn split_never_bisects_a_wide_grapheme() {
        // 日=2, 本=2; col 3 falls inside 本, which must go to the tail whole.
        let (head, w, tail) = split_at_col("日本語", 3);
        assert_eq!(head, "日");
        assert_eq!(w, 2);
        assert_eq!(tail, "本語");
    }

    #[test]
    fn split_with_zero_cols_yields_empty_head() {
        let (head, w, tail) = split_at_col("abc", 0);
        assert_eq!((head, w, tail), ("", 0, "abc"));
    }

    #[test]
    fn truncate_is_a_noop_when_it_fits() {
        assert_eq!(truncate("short", 10, "…"), "short");
    }

    #[test]
    fn truncate_respects_the_budget_including_ellipsis() {
        let out = truncate("a very long label", 8, "…");
        assert!(width(&out) <= 8, "{out:?} is {} cells", width(&out));
        assert!(out.ends_with('…'));
    }

    #[test]
    fn truncate_wide_text_stays_within_budget() {
        let out = truncate("日本語のラベル", 7, "…");
        assert!(width(&out) <= 7, "{out:?} is {} cells", width(&out));
    }
}
