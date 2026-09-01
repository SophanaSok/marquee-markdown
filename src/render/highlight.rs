//! Syntax highlighting for code blocks.
//!
//! Maps syntect styles directly onto `ratatui::Style` — no ANSI round-trip —
//! and forces the theme's surface background so the syntax theme's own page
//! color never leaks into the code card. Syntax and theme sets are loaded once
//! per process.

use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use ratatui::style::{Color, Modifier, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::theme::Theme;

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static SET: OnceLock<ThemeSet> = OnceLock::new();
    SET.get_or_init(ThemeSet::load_defaults)
}

/// Whether the highlighter knows a syntax theme by this name.
///
/// A theme file naming one it does not know indexes past the end at the first
/// code block, which is a long way from the typo that caused it. The bundled
/// palettes are checked against this in their own tests.
#[must_use]
pub fn has_syntax_theme(name: &str) -> bool {
    theme_set().themes.contains_key(name)
}

/// Per-source-line styled pieces, as [`highlight`] returns them.
pub type Highlighted = Vec<Vec<(Style, String)>>;

/// Memoized [`highlight`] output, living as long as the document it belongs to.
///
/// Highlighting is a pure function of the text, the language and the theme,
/// and a resize changes none of them — but it was called from inside the
/// layout emitter, so every width change re-ran syntect over every code block
/// in the document. It is not a small part of the bill: 120 `rust` fences lay
/// out in 203 ms against 6 ms for the same text with the language taken off,
/// so around 97% of a re-layout was highlighting, computed and thrown away
/// again for every step of a drag.
///
/// The cache belongs to a [`Document`](crate::render::Document) because that
/// is exactly how long it is worth keeping: the same parse laid out many
/// times is the thing being paid for, and reloading the file should forget
/// all of it.
#[derive(Default)]
pub struct HighlightCache {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// The theme every entry was highlighted for; a change empties the cache.
    ///
    /// Whole themes are compared rather than the parts that happen to matter
    /// today — the syntax theme named, the surface forced onto every span,
    /// the fill used when there is no language. A theme gaining a fourth
    /// thing that reaches `highlight` would otherwise start serving stale
    /// colors silently, which is the failure this project takes most care to
    /// make unreachable.
    theme: Option<Theme>,
    entries: Vec<Option<Entry>>,
    /// How often the expensive call has actually been made.
    computed: usize,
}

struct Entry {
    /// What this entry was built from, re-checked on every hit.
    ///
    /// The index alone is trusted only as far as it can be verified. It is a
    /// count of the code blocks the walk has reached, which is a property of
    /// the tree and not of the width — but it is maintained by hand a few
    /// modules away, and an entry served for the wrong block would be wrong
    /// *colors on real code*, silently. Two `usize` comparisons and a short
    /// string one are not worth skipping to avoid that.
    span: Range<usize>,
    len: usize,
    language: Option<String>,
    spans: Arc<Highlighted>,
}

impl std::fmt::Debug for HighlightCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HighlightCache")
    }
}

impl HighlightCache {
    /// How many times this cache has had to highlight something.
    ///
    /// The point of the cache is a number that stops going up when the width
    /// changes, so it is worth being able to assert on rather than infer from
    /// a stopwatch.
    #[must_use]
    pub fn computed(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .computed
    }

    /// Highlighted spans for one code block, computing them only if this is
    /// the first time they have been asked for at this theme.
    ///
    /// `index` counts code blocks in tree order, and must not depend on the
    /// width — see [`Entry::span`], which is what checks that it did not.
    pub fn get_or_insert(
        &self,
        index: usize,
        span: &Range<usize>,
        text: &str,
        language: Option<&str>,
        theme: &Theme,
    ) -> Arc<Highlighted> {
        // A poisoned memo is still a memo: a panic elsewhere is no reason to
        // take the reader down, or to stop caching for the rest of the run.
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);

        if inner.theme.as_ref() != Some(theme) {
            inner.theme = Some(theme.clone());
            inner.entries.clear();
        }
        if index >= inner.entries.len() {
            inner.entries.resize_with(index + 1, || None);
        }
        if let Some(entry) = &inner.entries[index]
            && entry.span == *span
            && entry.len == text.len()
            && entry.language.as_deref() == language
        {
            return Arc::clone(&entry.spans);
        }

        // Held across the call on purpose. This is per-document state that
        // one reader walks in one thread; a second layout of the same
        // document racing the first would rather wait than highlight it
        // twice, and nothing reached from here takes this lock again.
        let spans = Arc::new(highlight(text, language, theme));
        inner.computed += 1;
        inner.entries[index] = Some(Entry {
            span: span.clone(),
            len: text.len(),
            language: language.map(str::to_owned),
            spans: Arc::clone(&spans),
        });
        spans
    }
}

/// Highlight `text`, returning per-source-line styled pieces.
///
/// Unknown or absent languages fall back to unstyled text in the theme's code
/// foreground; output always has exactly one entry per source line.
#[must_use]
pub fn highlight(text: &str, language: Option<&str>, theme: &Theme) -> Vec<Vec<(Style, String)>> {
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.lines().collect()
    };

    let Some(syntax) = language.filter(|l| !l.is_empty()).and_then(|l| {
        let set = syntax_set();
        set.find_syntax_by_token(l)
            .or_else(|| set.find_syntax_by_extension(l))
    }) else {
        let plain = theme.code_fill();
        return lines
            .iter()
            .map(|l| vec![(plain, (*l).to_owned())])
            .collect();
    };

    let syntect_theme = &theme_set().themes[theme.syntax_theme_name()];
    let mut highlighter = HighlightLines::new(syntax, syntect_theme);
    let surface = theme.palette.surface.color();

    lines
        .iter()
        .map(|line| {
            // syntect wants the newline for stateful parsing.
            let with_newline = format!("{line}\n");
            match highlighter.highlight_line(&with_newline, syntax_set()) {
                Ok(ranges) => ranges
                    .into_iter()
                    .map(|(style, piece)| {
                        (
                            convert(style, surface),
                            piece.trim_end_matches('\n').to_owned(),
                        )
                    })
                    .filter(|(_, piece)| !piece.is_empty())
                    .collect(),
                Err(_) => vec![(theme.code_fill(), (*line).to_owned())],
            }
        })
        .collect()
}

/// syntect style → ratatui style, with the surface bg forced.
fn convert(style: syntect::highlighting::Style, surface: Color) -> Style {
    let fg = style.foreground;
    let mut out = Style::new().fg(Color::Rgb(fg.r, fg.g, fg.b)).bg(surface);
    if style.font_style.contains(FontStyle::BOLD) {
        out = out.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        out = out.add_modifier(Modifier::ITALIC);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Theme, ThemeVariant};

    fn theme() -> Theme {
        Theme::new(ThemeVariant::Slate)
    }

    const CODE: &str = "fn main() { let x = 1; }";

    #[test]
    fn asking_twice_highlights_once_and_hands_back_the_same_spans() {
        let cache = HighlightCache::default();
        let first = cache.get_or_insert(0, &(0..10), CODE, Some("rust"), &theme());
        let second = cache.get_or_insert(0, &(0..10), CODE, Some("rust"), &theme());
        assert_eq!(cache.computed(), 1, "the second ask was a hit");
        assert!(
            Arc::ptr_eq(&first, &second),
            "and handed back what the first one produced"
        );
    }

    #[test]
    fn a_theme_change_throws_the_whole_cache_away() {
        // Highlighting depends on the theme three separate ways, so nothing
        // in here survives one changing.
        let cache = HighlightCache::default();
        let slate = Theme::new(ThemeVariant::Slate);
        let paper = Theme::new(ThemeVariant::Paper);

        let a = cache.get_or_insert(0, &(0..10), CODE, Some("rust"), &slate);
        let b = cache.get_or_insert(0, &(0..10), CODE, Some("rust"), &paper);
        assert_eq!(cache.computed(), 2);
        assert!(!Arc::ptr_eq(&a, &b));
        assert_ne!(a[0][0].0.bg, b[0][0].0.bg, "different surface underneath");

        // And switching back is a fresh highlight rather than a stale hit.
        let c = cache.get_or_insert(0, &(0..10), CODE, Some("rust"), &slate);
        assert_eq!(cache.computed(), 3);
        assert_eq!(c[0][0].0.bg, a[0][0].0.bg);
    }

    #[test]
    fn an_index_that_now_means_a_different_block_is_not_served_the_old_one() {
        // The index is a count kept by the layout walk. If it ever drifted,
        // the reader would get another block's colors on real code and
        // nothing would say so — which is why an entry is checked against the
        // block it was built from rather than trusted.
        let cache = HighlightCache::default();
        let t = theme();
        let first = cache.get_or_insert(0, &(0..10), CODE, Some("rust"), &t);

        let moved = cache.get_or_insert(0, &(40..50), CODE, Some("rust"), &t);
        assert_eq!(cache.computed(), 2, "a different span is a miss");
        assert!(!Arc::ptr_eq(&first, &moved));

        let relabelled = cache.get_or_insert(0, &(40..50), CODE, Some("toml"), &t);
        assert_eq!(cache.computed(), 3, "a different language is a miss");
        assert!(!Arc::ptr_eq(&moved, &relabelled));

        let longer = cache.get_or_insert(0, &(40..50), "fn other() {}", Some("toml"), &t);
        assert_eq!(cache.computed(), 4, "different text is a miss");
        assert_eq!(
            longer[0]
                .iter()
                .map(|(_, t)| t.as_str())
                .collect::<String>(),
            "fn other() {}"
        );
    }

    #[test]
    fn one_output_entry_per_source_line() {
        let out = highlight("a\nb\nc", Some("rust"), &theme());
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn unknown_language_falls_back_to_plain() {
        let out = highlight("whatever text", Some("no-such-lang"), &theme());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0][0].1, "whatever text");
    }

    #[test]
    fn empty_text_yields_one_empty_line() {
        let out = highlight("", None, &theme());
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn all_spans_carry_the_surface_background() {
        let t = theme();
        let out = highlight("fn main() { let x = 1; }", Some("rust"), &t);
        for (style, _) in out[0].iter() {
            assert_eq!(style.bg, Some(t.palette.surface.color()));
        }
    }

    #[test]
    fn rust_code_gets_more_than_one_color() {
        let out = highlight("fn main() { let x = \"s\"; }", Some("rust"), &theme());
        let distinct: std::collections::HashSet<_> = out[0].iter().map(|(s, _)| s.fg).collect();
        assert!(distinct.len() > 1, "expected varied colors: {distinct:?}");
    }

    #[test]
    fn text_round_trips_exactly() {
        let src = "let a = 1;\n    indented();";
        let out = highlight(src, Some("rust"), &theme());
        let rejoined: Vec<String> = out
            .iter()
            .map(|line| line.iter().map(|(_, t)| t.as_str()).collect())
            .collect();
        assert_eq!(rejoined.join("\n"), src);
    }
}
