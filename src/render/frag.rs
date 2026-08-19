//! Inline content → styled, measured fragments: the wrapper's input.
//!
//! A [`Frag`] carries display text, a resolved style, an optional link index,
//! and a pre-computed cell width. Escape sequences are structurally
//! unrepresentable here — `text` only ever holds what will occupy cells, which
//! is what makes the width invariant enforceable at all.

use ratatui::style::{Modifier, Style};

use super::block::Inline;
use super::measure;
use crate::theme::Theme;

/// A minimal unit of inline layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Frag {
    pub text: String,
    pub style: Style,
    /// Index into the document's interned link table.
    pub link: Option<u32>,
    /// Display width in cells, computed once at construction.
    pub width: usize,
    pub kind: FragKind,
}

/// How the wrapper may treat a fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragKind {
    /// Breakable before; not droppable.
    Word,
    /// A space: breakable, dropped at line ends.
    Space,
    /// Must stay attached to the previous fragment (inline-code padding).
    Glue,
    /// Forced line break.
    Break,
}

impl Frag {
    fn new(text: impl Into<String>, style: Style, link: Option<u32>, kind: FragKind) -> Self {
        let text = text.into();
        let width = measure::width(&text);
        Self {
            text,
            style,
            link,
            width,
            kind,
        }
    }
}

/// Sink for interned links discovered during fragmentation.
pub trait LinkSink {
    /// Intern `dest` and return its index.
    fn intern(&mut self, dest: &str) -> u32;
}

/// A no-op sink for contexts that don't track links (e.g. table measurement).
pub struct IgnoreLinks;

impl LinkSink for IgnoreLinks {
    fn intern(&mut self, _dest: &str) -> u32 {
        0
    }
}

/// What a single newline inside a paragraph means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Breaks {
    /// Markdown's own rule: a newline inside a paragraph is a space, and the
    /// paragraph is re-wrapped to the column.
    #[default]
    Collapse,
    /// Keep the line breaks the author typed. This is `glow`'s `-n`, and it is
    /// what a document written one-sentence-per-line wants.
    Preserve,
}

/// Flatten inline content into fragments under `base` style.
pub fn fragment(
    content: &[Inline],
    base: Style,
    theme: &Theme,
    links: &mut dyn LinkSink,
    breaks: Breaks,
) -> Vec<Frag> {
    let mut out = Vec::new();
    walk(content, base, None, theme, links, breaks, &mut out);
    out
}

fn walk(
    content: &[Inline],
    style: Style,
    link: Option<u32>,
    theme: &Theme,
    links: &mut dyn LinkSink,
    breaks: Breaks,
    out: &mut Vec<Frag>,
) {
    for inline in content {
        match inline {
            Inline::Text(text) => push_text(text, style, link, out),
            Inline::Code(code) => {
                // A padded chip: the pads glue to the code so they never orphan
                // at a line edge, while the code itself may wrap word-by-word.
                let chip = theme.inline_code().patch(style_only_modifiers(style));
                out.push(Frag::new(" ", chip, link, FragKind::Word));
                let start = out.len();
                push_text(code, chip, link, out);
                // Re-kind: first piece glues to the opening pad.
                if let Some(first) = out.get_mut(start) {
                    first.kind = FragKind::Glue;
                }
                out.push(Frag::new(" ", chip, link, FragKind::Glue));
            }
            Inline::Emphasis(children) => {
                walk(
                    children,
                    style.add_modifier(Modifier::ITALIC),
                    link,
                    theme,
                    links,
                    breaks,
                    out,
                );
            }
            Inline::Strong(children) => {
                walk(
                    children,
                    style.add_modifier(Modifier::BOLD),
                    link,
                    theme,
                    links,
                    breaks,
                    out,
                );
            }
            Inline::Strikethrough(children) => {
                walk(
                    children,
                    style.add_modifier(Modifier::CROSSED_OUT),
                    link,
                    theme,
                    links,
                    breaks,
                    out,
                );
            }
            Inline::Link { dest, content } => {
                let idx = links.intern(dest);
                let style = theme.link().patch(style_only_modifiers(style));
                walk(content, style, Some(idx), theme, links, breaks, out);
            }
            Inline::Image { dest, alt } => {
                // Terminal-safe placeholder: alt text as a link to the image.
                let idx = links.intern(dest);
                let style = theme.link().patch(style_only_modifiers(style));
                out.push(Frag::new("\u{f03e} ", style, Some(idx), FragKind::Word));
                walk(alt, style, Some(idx), theme, links, breaks, out);
            }
            Inline::FootnoteReference(label) => {
                let sup = theme.muted().add_modifier(Modifier::BOLD);
                out.push(Frag::new(format!("[{label}]"), sup, link, FragKind::Glue));
            }
            Inline::SoftBreak => {
                let kind = match breaks {
                    Breaks::Collapse => FragKind::Space,
                    Breaks::Preserve => FragKind::Break,
                };
                let text = if kind == FragKind::Break { "" } else { " " };
                out.push(Frag::new(text, style, link, kind));
            }
            Inline::HardBreak => out.push(Frag::new("", style, link, FragKind::Break)),
        }
    }
}

/// Split a text run into word and space fragments, stripping control chars and
/// expanding tabs. Adjacent non-space runs stay one fragment; the wrapper
/// hard-splits overlong atoms at grapheme boundaries when it must.
fn push_text(text: &str, style: Style, link: Option<u32>, out: &mut Vec<Frag>) {
    let clean: String = text
        .replace('\t', "    ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let mut rest = clean.as_str();
    while !rest.is_empty() {
        let is_space = rest.starts_with(' ');
        let end = rest
            .find(|c: char| (c == ' ') != is_space)
            .unwrap_or(rest.len());
        let (piece, tail) = rest.split_at(end);
        let kind = if is_space {
            FragKind::Space
        } else {
            FragKind::Word
        };
        out.push(Frag::new(piece, style, link, kind));
        rest = tail;
    }
}

/// Keep only the modifier bits of an inherited style, so chips and links take
/// their colors from the theme but keep surrounding bold/italic.
fn style_only_modifiers(style: Style) -> Style {
    Style::new().add_modifier(style.add_modifier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Theme, ThemeVariant};

    fn theme() -> Theme {
        Theme::new(ThemeVariant::Paper)
    }

    fn frags(content: &[Inline]) -> Vec<Frag> {
        let t = theme();
        fragment(content, t.body(), &t, &mut IgnoreLinks, Breaks::Collapse)
    }

    #[test]
    fn words_and_spaces_alternate() {
        let out = frags(&[Inline::Text("two words".into())]);
        let kinds: Vec<_> = out.iter().map(|f| f.kind).collect();
        assert_eq!(kinds, [FragKind::Word, FragKind::Space, FragKind::Word]);
        assert_eq!(out[0].text, "two");
        assert_eq!(out[2].text, "words");
    }

    #[test]
    fn widths_are_precomputed() {
        let out = frags(&[Inline::Text("日本 ok".into())]);
        assert_eq!(out[0].width, 4);
        assert_eq!(out[2].width, 2);
    }

    #[test]
    fn inline_code_pads_are_glued() {
        let out = frags(&[Inline::Code("x".into())]);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].kind, FragKind::Word);
        assert_eq!(out[1].kind, FragKind::Glue);
        assert_eq!(out[2].kind, FragKind::Glue);
        let surface = theme().palette.surface.color();
        assert!(out.iter().all(|f| f.style.bg == Some(surface)));
    }

    #[test]
    fn bold_survives_into_link_style() {
        let out = frags(&[Inline::Strong(vec![Inline::Link {
            dest: "https://x".into(),
            content: vec![Inline::Text("go".into())],
        }])]);
        let link_frag = &out[0];
        assert!(link_frag.style.add_modifier.contains(Modifier::BOLD));
        assert!(link_frag.style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(link_frag.style.fg, Some(theme().palette.accent.color()));
    }

    #[test]
    fn hard_break_becomes_break_frag() {
        let out = frags(&[
            Inline::Text("a".into()),
            Inline::HardBreak,
            Inline::Text("b".into()),
        ]);
        assert!(out.iter().any(|f| f.kind == FragKind::Break));
    }

    #[test]
    fn control_chars_are_stripped_and_tabs_expanded() {
        let out = frags(&[Inline::Text("a\u{7}b\tc".into())]);
        let text: String = out.iter().map(|f| f.text.as_str()).collect();
        assert_eq!(text, "ab    c");
        assert!(out.iter().all(|f| f.text.chars().all(|c| !c.is_control())));
    }

    #[test]
    fn links_are_interned_in_order() {
        struct Collect(Vec<String>);
        impl LinkSink for Collect {
            fn intern(&mut self, dest: &str) -> u32 {
                self.0.push(dest.to_owned());
                (self.0.len() - 1) as u32
            }
        }
        let mut sink = Collect(Vec::new());
        let t = theme();
        let out = fragment(
            &[
                Inline::Link {
                    dest: "https://a".into(),
                    content: vec![Inline::Text("A".into())],
                },
                Inline::Text(" ".into()),
                Inline::Link {
                    dest: "https://b".into(),
                    content: vec![Inline::Text("B".into())],
                },
            ],
            t.body(),
            &t,
            &mut sink,
            Breaks::Collapse,
        );
        assert_eq!(sink.0, ["https://a", "https://b"]);
        assert_eq!(
            out.iter().filter_map(|f| f.link).collect::<Vec<_>>(),
            [0, 1]
        );
    }

    #[test]
    fn a_newline_inside_a_paragraph_is_a_space_by_default() {
        let t = theme();
        let out = fragment(
            &[
                Inline::Text("one".into()),
                Inline::SoftBreak,
                Inline::Text("two".into()),
            ],
            t.body(),
            &t,
            &mut IgnoreLinks,
            Breaks::Collapse,
        );
        assert!(out.iter().all(|frag| frag.kind != FragKind::Break));
        let text: String = out.iter().map(|frag| frag.text.as_str()).collect();
        assert_eq!(text, "one two");
    }

    #[test]
    fn preserving_newlines_turns_them_into_line_breaks() {
        // What `-n` is for: a document written one sentence per line keeps its
        // shape instead of being re-flowed into a wall.
        let t = theme();
        let out = fragment(
            &[
                Inline::Text("one".into()),
                Inline::SoftBreak,
                Inline::Text("two".into()),
            ],
            t.body(),
            &t,
            &mut IgnoreLinks,
            Breaks::Preserve,
        );
        assert_eq!(
            out.iter()
                .filter(|frag| frag.kind == FragKind::Break)
                .count(),
            1
        );
        // And the break contributes no width of its own.
        assert!(
            out.iter()
                .filter(|frag| frag.kind == FragKind::Break)
                .all(|frag| frag.width == 0)
        );
    }
}
