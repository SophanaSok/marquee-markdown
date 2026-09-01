//! Raw HTML → the block tree.
//!
//! README files are written in markdown with HTML holes in them: a centered
//! `<h1>`, a `<p align="center">` of badge images, `<br>` inside a tagline,
//! `<sub>` captions under screenshots. Printing those tags is what a markdown
//! reader must not do, and dropping them silently loses the title.
//!
//! So this interprets the vocabulary those documents actually use, and refuses
//! everything else. Refusal is safe: the caller falls back to rendering the
//! block as muted literal text, which is what it did before this module
//! existed. There is no case where a guess is rendered as if it were known.
//!
//! This is deliberately not an HTML parser. It does not build a DOM to spec,
//! it does not implement the tag-omission rules, and it never will — anything
//! needing that is opaque to it and goes back to the caller untouched.

use std::ops::Range;
use std::str::FromStr;

use super::block::{Alignment, Block, BlockKind, Inline, MAX_NESTING};
use super::parse::InlineKind;

/// What to do with raw HTML in a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HtmlMode {
    /// Interpret the tags this module knows; fall back to [`Self::Literal`]
    /// for a block containing anything it does not.
    #[default]
    Render,
    /// Drop it. Nothing an author wrote in HTML reaches the page.
    Hide,
    /// Print the markup itself, as muted text. What the reader did before
    /// interpretation existed.
    Literal,
}

impl HtmlMode {
    /// The spelling used in configuration files and `MARQUEE_HTML`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Render => "render",
            Self::Hide => "hide",
            Self::Literal => "literal",
        }
    }

    /// Every mode, for help text and tests.
    pub const ALL: [Self; 3] = [Self::Render, Self::Hide, Self::Literal];
}

impl std::fmt::Display for HtmlMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for HtmlMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "render" => Ok(Self::Render),
            "hide" => Ok(Self::Hide),
            "literal" => Ok(Self::Literal),
            other => Err(format!(
                "`{other}` is not an html mode; expected render, hide or literal"
            )),
        }
    }
}

// --- tokens ---------------------------------------------------------------

/// One piece of a raw HTML run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Token {
    Open {
        name: String,
        attrs: Vec<(String, String)>,
    },
    Close(String),
    /// A tag that can never have children: a void element, or one written
    /// self-closing.
    Void {
        name: String,
        attrs: Vec<(String, String)>,
    },
    Text(String),
}

/// Elements that are void in HTML whether or not they are written with a
/// trailing slash.
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Split a raw HTML run into tags and text.
///
/// Returns `None` for input this cannot make sense of — an unterminated tag,
/// an attribute that never closes its quote. Malformed *nesting* is not an
/// error here; [`tree`] is forgiving about that.
pub(super) fn scan(raw: &str) -> Option<Vec<Token>> {
    let bytes: Vec<char> = raw.chars().collect();
    let mut tokens = Vec::new();
    let mut text = String::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != '<' {
            text.push(bytes[i]);
            i += 1;
            continue;
        }

        // A `<` that cannot begin a tag is literal text, which is what a
        // browser does and what `a < b` in a paragraph needs.
        let next = bytes.get(i + 1).copied();
        let starts_tag = matches!(next, Some(c) if c.is_ascii_alphabetic() || c == '/' || c == '!');
        if !starts_tag {
            text.push('<');
            i += 1;
            continue;
        }

        // Comments and doctypes carry nothing to render.
        if raw_starts_at(&bytes, i, "<!--") {
            let end = find_at(&bytes, i + 4, "-->")?;
            i = end + 3;
            continue;
        }
        if raw_starts_at(&bytes, i, "<!") {
            let end = index_of(&bytes, i, '>')?;
            i = end + 1;
            continue;
        }

        if !text.is_empty() {
            tokens.push(Token::Text(std::mem::take(&mut text)));
        }

        let (token, after) = scan_tag(&bytes, i)?;
        tokens.push(token);
        i = after;
    }

    if !text.is_empty() {
        tokens.push(Token::Text(text));
    }
    Some(tokens)
}

/// Scan one tag starting at `start`, which is known to be `<`.
fn scan_tag(bytes: &[char], start: usize) -> Option<(Token, usize)> {
    let mut i = start + 1;
    let closing = bytes.get(i) == Some(&'/');
    if closing {
        i += 1;
    }

    let name_start = i;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == '-') {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name: String = bytes[name_start..i]
        .iter()
        .collect::<String>()
        .to_lowercase();

    if closing {
        // A closing tag takes no attributes; skip to `>` rather than object.
        let end = index_of(bytes, i, '>')?;
        return Some((Token::Close(name), end + 1));
    }

    let mut attrs = Vec::new();
    let mut self_closing = false;
    loop {
        while i < bytes.len() && bytes[i].is_whitespace() {
            i += 1;
        }
        match bytes.get(i) {
            None => return None, // unterminated tag
            Some('>') => {
                i += 1;
                break;
            }
            Some('/') if bytes.get(i + 1) == Some(&'>') => {
                self_closing = true;
                i += 2;
                break;
            }
            Some('/') => {
                i += 1;
                continue;
            }
            Some(_) => {}
        }

        let key_start = i;
        while i < bytes.len() && !bytes[i].is_whitespace() && !matches!(bytes[i], '=' | '>' | '/') {
            // A `<` here means the previous tag never closed, so what looked
            // like an attribute is the start of the next one.
            if bytes[i] == '<' {
                return None;
            }
            i += 1;
        }
        if i == key_start {
            return None;
        }
        let key: String = bytes[key_start..i]
            .iter()
            .collect::<String>()
            .to_lowercase();

        while i < bytes.len() && bytes[i].is_whitespace() {
            i += 1;
        }
        if bytes.get(i) != Some(&'=') {
            attrs.push((key, String::new())); // bare attribute, e.g. `hidden`
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_whitespace() {
            i += 1;
        }

        let value = match bytes.get(i) {
            Some(&quote @ ('"' | '\'')) => {
                let end = index_of(bytes, i + 1, quote)?;
                let value: String = bytes[i + 1..end].iter().collect();
                i = end + 1;
                value
            }
            Some(_) => {
                let value_start = i;
                while i < bytes.len() && !bytes[i].is_whitespace() && bytes[i] != '>' {
                    i += 1;
                }
                bytes[value_start..i].iter().collect()
            }
            None => return None,
        };
        attrs.push((key, decode_entities(&value)));
    }

    let void = self_closing || VOID.contains(&name.as_str());
    let token = if void {
        Token::Void { name, attrs }
    } else {
        Token::Open { name, attrs }
    };
    Some((token, i))
}

fn raw_starts_at(bytes: &[char], at: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(offset, c)| bytes.get(at + offset) == Some(&c))
}

fn find_at(bytes: &[char], from: usize, needle: &str) -> Option<usize> {
    (from..bytes.len()).find(|&i| raw_starts_at(bytes, i, needle))
}

fn index_of(bytes: &[char], from: usize, needle: char) -> Option<usize> {
    (from..bytes.len()).find(|&i| bytes[i] == needle)
}

// --- entities -------------------------------------------------------------

/// The named entities worth knowing, chosen from what documentation actually
/// contains. Anything else is left as written, which reads better than a
/// replacement character.
const ENTITIES: &[(&str, &str)] = &[
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    ("nbsp", "\u{a0}"),
    ("ensp", "\u{2002}"),
    ("emsp", "\u{2003}"),
    ("thinsp", "\u{2009}"),
    ("mdash", "—"),
    ("ndash", "–"),
    ("hellip", "…"),
    ("copy", "©"),
    ("reg", "®"),
    ("trade", "™"),
    ("deg", "°"),
    ("plusmn", "±"),
    ("times", "×"),
    ("divide", "÷"),
    ("middot", "·"),
    ("bull", "•"),
    ("dagger", "†"),
    ("sect", "§"),
    ("para", "¶"),
    ("laquo", "«"),
    ("raquo", "»"),
    ("ldquo", "\u{201c}"),
    ("rdquo", "\u{201d}"),
    ("lsquo", "\u{2018}"),
    ("rsquo", "\u{2019}"),
    ("larr", "←"),
    ("rarr", "→"),
    ("uarr", "↑"),
    ("darr", "↓"),
    ("harr", "↔"),
    ("check", "✓"),
    ("cross", "✗"),
    ("star", "★"),
    ("hearts", "♥"),
];

/// Replace HTML entities with what they stand for.
///
/// Markdown text arrives already decoded from pulldown-cmark; only text inside
/// a raw HTML run still carries them.
#[must_use]
pub(super) fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '&' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // An entity is short; a `&` with no `;` close by is just an ampersand.
        let Some(end) = (i + 1..chars.len().min(i + 12)).find(|&j| chars[j] == ';') else {
            out.push('&');
            i += 1;
            continue;
        };
        let body: String = chars[i + 1..end].iter().collect();
        let decoded = if let Some(digits) = body.strip_prefix("#x").or(body.strip_prefix("#X")) {
            u32::from_str_radix(digits, 16)
                .ok()
                .and_then(char::from_u32)
        } else if let Some(digits) = body.strip_prefix('#') {
            digits.parse().ok().and_then(char::from_u32)
        } else {
            None
        };
        if let Some(c) = decoded {
            out.push(c);
            i = end + 1;
            continue;
        }
        let named = ENTITIES
            .iter()
            .find(|(name, _)| *name == body)
            .map(|(_, value)| *value);
        match named {
            Some(value) => {
                out.push_str(value);
                i = end + 1;
            }
            None => {
                out.push('&');
                i += 1;
            }
        }
    }
    out
}

// --- the element tree -----------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Node {
    Element {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<Node>,
    },
    Text(String),
}

/// Build a tree from a token run.
///
/// Forgiving on purpose. A `CommonMark` HTML block ends at a blank line, so an
/// author who leaves one inside a `<p align="center">` hands us an opening tag
/// in one block and its closing tag in the next. Anything still open at the
/// end is closed here, and a closing tag matching nothing open is dropped —
/// which turns that split into two sensible halves instead of two failures.
pub(super) fn tree(tokens: Vec<Token>) -> Vec<Node> {
    /// An element whose children are still being collected.
    struct Open {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<Node>,
        /// Past [`MAX_NESTING`], the element is not represented in the tree:
        /// its children are spliced into its parent instead, exactly as
        /// `Role::Other` does for a tag that carries no meaning. It stays on
        /// the stack regardless, because `Token::Close` finds its match by
        /// name — drop the entry and a later `</div>` closes an *outer* div
        /// instead, unwinding structure that was perfectly fine.
        flattened: bool,
    }

    let mut stack: Vec<Open> = Vec::new();
    let mut root: Vec<Node> = Vec::new();
    // Open elements that the tree actually nests, kept as a running count so
    // a document of `<div>`s does not make each open O(depth) to record.
    let mut represented = 0usize;

    /// Close the innermost open element and hand it to its parent.
    fn close_one(stack: &mut Vec<Open>, root: &mut Vec<Node>, represented: &mut usize) {
        let Some(open) = stack.pop() else { return };
        let target = stack.last_mut().map_or(root, |parent| &mut parent.children);
        if open.flattened {
            target.extend(open.children);
            return;
        }
        *represented -= 1;
        target.push(Node::Element {
            name: open.name,
            attrs: open.attrs,
            children: open.children,
        });
    }

    for token in tokens {
        match token {
            Token::Text(text) => {
                let target = stack.last_mut().map_or(&mut root, |o| &mut o.children);
                target.push(Node::Text(text));
            }
            Token::Void { name, attrs } => {
                let target = stack.last_mut().map_or(&mut root, |o| &mut o.children);
                target.push(Node::Element {
                    name,
                    attrs,
                    children: Vec::new(),
                });
            }
            Token::Open { name, attrs } => {
                let flattened = represented >= MAX_NESTING;
                if !flattened {
                    represented += 1;
                }
                stack.push(Open {
                    name,
                    attrs,
                    children: Vec::new(),
                    flattened,
                });
            }
            Token::Close(name) => {
                let Some(depth) = stack.iter().rposition(|open| open.name == name) else {
                    continue; // a close with nothing open: ignore it
                };
                // Everything opened inside it closes with it.
                while stack.len() > depth {
                    close_one(&mut stack, &mut root, &mut represented);
                }
            }
        }
    }

    while !stack.is_empty() {
        close_one(&mut stack, &mut root, &mut represented);
    }
    root
}

// --- roles ----------------------------------------------------------------

/// What an element means to the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Heading(u8),
    /// Becomes a paragraph, or a container when it holds block-level children.
    Paragraph,
    Quote,
    Rule,
    Break,
    Image,
    Link,
    Strong,
    Emphasis,
    Strikethrough,
    Code,
    /// Carries no meaning this renderer can show: keep the children, drop the
    /// tag. `<sub>` and `<small>` are here because a terminal has one type
    /// size, not because they mean nothing.
    ///
    /// **Unknown tags land here too.** Falling back to literal for anything
    /// unrecognized sounds conservative, but it reproduces the bug this module
    /// exists to fix for every tag nobody thought of — and the tail is long
    /// (`<figure>`, `<figcaption>`, `<kbd>`, custom elements). In `render`
    /// mode the promise is that no markup reaches the page; a reader who wants
    /// the markup has `html = "literal"` for exactly that.
    Transparent,
    /// `<details>` — a collapsible aside. Rendered expanded, as a quote whose
    /// first paragraph is the `<summary>`.
    Details,
    /// `<summary>` — the label of a `<details>`. Only meaningful inside one;
    /// loose, it is an ordinary paragraph.
    Summary,
    /// Understood well enough to know this renderer would make a mess of it.
    /// One of these anywhere sends the whole block back to be rendered as
    /// literal text, because the markup carries more than the flattened words
    /// would: a table read as one run-on sentence is worse than a table read
    /// as tags.
    Opaque,
}

impl Role {
    /// Whether an element of this role interrupts a paragraph.
    fn is_block(self) -> bool {
        matches!(
            self,
            Self::Heading(_)
                | Self::Paragraph
                | Self::Quote
                | Self::Rule
                | Self::Details
                | Self::Summary
        )
    }
}

fn role(name: &str) -> Role {
    match name {
        "h1" => Role::Heading(1),
        "h2" => Role::Heading(2),
        "h3" => Role::Heading(3),
        "h4" => Role::Heading(4),
        "h5" => Role::Heading(5),
        "h6" => Role::Heading(6),
        "p" | "div" | "section" | "article" | "header" | "footer" | "main" => Role::Paragraph,
        "blockquote" => Role::Quote,
        "hr" => Role::Rule,
        "br" => Role::Break,
        "img" => Role::Image,
        "a" => Role::Link,
        "b" | "strong" => Role::Strong,
        "i" | "em" | "cite" | "var" | "dfn" => Role::Emphasis,
        "s" | "del" | "strike" => Role::Strikethrough,
        "code" | "kbd" | "samp" | "tt" => Role::Code,
        "details" => Role::Details,
        "summary" => Role::Summary,
        // Structure this renderer has no emitter for. Literal is the honest
        // answer: the tags say more than the words would on their own.
        "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" | "caption" | "colgroup"
        | "col" | "ul" | "ol" | "li" | "dl" | "dt" | "dd" | "pre" | "script" | "style"
        | "textarea" | "iframe" | "object" | "embed" | "svg" | "math" | "form" | "input"
        | "button" | "select" | "option" | "textpath" => Role::Opaque,
        _ => Role::Transparent,
    }
}

/// The alignment an element asks for, if any.
fn alignment(name: &str, attrs: &[(String, String)]) -> Option<Alignment> {
    if name == "center" {
        return Some(Alignment::Center);
    }
    let value = attrs
        .iter()
        .find(|(key, _)| key == "align")
        .map(|(_, value)| value.to_lowercase());
    match value.as_deref() {
        Some("center") => Some(Alignment::Center),
        Some("right") => Some(Alignment::Right),
        Some("left") => Some(Alignment::Left),
        _ => None,
    }
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

// --- interpretation -------------------------------------------------------

/// Turn a raw HTML block into blocks, or decline.
///
/// `slug` is the caller's heading-slug allocator, so ids stay deduplicated
/// across the whole document rather than per HTML block.
///
/// Returns `None` when the run cannot be scanned or contains an element this
/// module does not understand. The caller renders those literally, exactly as
/// it did before interpretation existed.
pub(super) fn interpret(
    raw: &str,
    span: &Range<usize>,
    slug: &mut dyn FnMut(&str) -> String,
) -> Option<Vec<Block>> {
    let tokens = scan(raw)?;
    let opaque = tokens.iter().any(|token| {
        let name = match token {
            Token::Open { name, .. } | Token::Void { name, .. } | Token::Close(name) => name,
            Token::Text(_) => return false,
        };
        role(name) == Role::Opaque
    });
    if opaque {
        return None;
    }

    let nodes = tree(tokens);
    let mut out = Vec::new();
    blocks_from(&nodes, span, slug, &mut out);
    Some(out)
}

/// Emit blocks for a run of nodes, gathering loose inline content into
/// paragraphs as it goes.
fn blocks_from(
    nodes: &[Node],
    span: &Range<usize>,
    slug: &mut dyn FnMut(&str) -> String,
    out: &mut Vec<Block>,
) {
    let mut pending: Vec<Inline> = Vec::new();

    for node in nodes {
        let Node::Element {
            name,
            attrs,
            children,
        } = node
        else {
            inlines_from(std::slice::from_ref(node), &mut pending);
            continue;
        };
        let role = role(name);
        if !role.is_block() {
            inlines_from(std::slice::from_ref(node), &mut pending);
            continue;
        }

        flush(&mut pending, span, out);
        let align = alignment(name, attrs);
        let mut produced = Vec::new();
        match role {
            Role::Heading(level) => {
                let mut content = Vec::new();
                inlines_from(children, &mut content);
                trim(&mut content);
                if !content.is_empty() {
                    let id = slug(&Inline::plain_text(&content));
                    produced.push(Block::at(
                        BlockKind::Heading { level, id, content },
                        span.clone(),
                    ));
                }
            }
            Role::Quote => {
                let mut children_blocks = Vec::new();
                blocks_from(children, span, slug, &mut children_blocks);
                if !children_blocks.is_empty() {
                    produced.push(Block::at(
                        BlockKind::BlockQuote {
                            alert: None,
                            children: children_blocks,
                        },
                        span.clone(),
                    ));
                }
            }
            Role::Rule => produced.push(Block::at(BlockKind::Rule, span.clone())),
            // Rendered expanded, because a terminal page has no click and
            // hiding the body would lose content the author shipped. The
            // quote's accent gutter is what marks the region as one thing;
            // the `<summary>` becomes its title, in strong, at the top.
            //
            // No disclosure glyph is added. A marker synthesized here would
            // land in the plain mirror and become searchable text, which the
            // real markers (bullets, gutter bars) added at layout time never
            // are.
            Role::Details => {
                let mut children_blocks = Vec::new();
                let (summary, body): (Vec<&Node>, Vec<&Node>) =
                    children.iter().partition(|node| {
                        matches!(node, Node::Element { name, .. } if self::role(name) == Role::Summary)
                    });
                // Only the first `<summary>` is a title; HTML ignores the
                // rest, and so does this.
                if let Some(Node::Element { children, .. }) = summary.first().copied() {
                    let mut content = Vec::new();
                    inlines_from(children, &mut content);
                    trim(&mut content);
                    if !content.is_empty() {
                        let content = vec![Inline::Strong(content)];
                        children_blocks
                            .push(Block::at(BlockKind::Paragraph(content), span.clone()));
                    }
                }
                let summary_only = children_blocks.len();
                let body: Vec<Node> = body.into_iter().cloned().collect();
                blocks_from(&body, span, slug, &mut children_blocks);

                // A blank line inside `<details>` ends the HTML block, which
                // is CommonMark's rule and the form GitHub requires for
                // markdown to render inside. The open tag and the body then
                // reach us as separate blocks and the body is not ours to
                // wrap. Quoting a title with nothing under it looks broken,
                // so in that case the summary stands on its own as a strong
                // paragraph and the body follows as itself.
                if children_blocks.len() == summary_only {
                    produced.append(&mut children_blocks);
                } else if !children_blocks.is_empty() {
                    produced.push(Block::at(
                        BlockKind::BlockQuote {
                            alert: None,
                            children: children_blocks,
                        },
                        span.clone(),
                    ));
                }
            }
            // A `<summary>` that escaped its `<details>`.
            Role::Summary => {
                let mut content = Vec::new();
                inlines_from(children, &mut content);
                trim(&mut content);
                if !content.is_empty() {
                    produced.push(Block::at(BlockKind::Paragraph(content), span.clone()));
                }
            }
            // A `<div>` wrapping other blocks is a container; one wrapping
            // text is a paragraph. Deciding by content is what lets a centered
            // `<div>` around a heading keep the heading.
            Role::Paragraph if holds_blocks(children) => {
                blocks_from(children, span, slug, &mut produced);
            }
            Role::Paragraph => {
                let mut content = Vec::new();
                inlines_from(children, &mut content);
                trim(&mut content);
                if !content.is_empty() {
                    produced.push(Block::at(BlockKind::Paragraph(content), span.clone()));
                }
            }
            _ => unreachable!("is_block covers exactly the arms above"),
        }

        // Alignment is inherited: a centered `<div>` centers the heading and
        // the paragraphs inside it, which is the whole reason it is written.
        if let Some(align) = align {
            for block in &mut produced {
                block.align = align;
            }
        }
        out.append(&mut produced);
    }

    flush(&mut pending, span, out);
}

/// Whether any child would become a block, deciding container from paragraph.
fn holds_blocks(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| match node {
        Node::Element { name, .. } => role(name).is_block(),
        Node::Text(_) => false,
    })
}

fn flush(pending: &mut Vec<Inline>, span: &Range<usize>, out: &mut Vec<Block>) {
    trim(pending);
    if pending.is_empty() {
        return;
    }
    out.push(Block::at(
        BlockKind::Paragraph(std::mem::take(pending)),
        span.clone(),
    ));
}

/// Emit inline content for a run of nodes.
fn inlines_from(nodes: &[Node], out: &mut Vec<Inline>) {
    for node in nodes {
        match node {
            Node::Text(text) => push_text(&collapse(&decode_entities(text)), out),
            Node::Element {
                name,
                attrs,
                children,
            } => match role(name) {
                Role::Break => {
                    if let Some(Inline::Text(prev)) = out.last_mut() {
                        let trimmed = prev.trim_end().to_owned();
                        if trimmed.is_empty() {
                            out.pop();
                        } else {
                            *prev = trimmed;
                        }
                    }
                    out.push(Inline::HardBreak);
                }
                Role::Image => out.extend(image(attrs)),
                Role::Link => {
                    let dest = attr(attrs, "href").unwrap_or_default().to_owned();
                    let mut content = Vec::new();
                    inlines_from(children, &mut content);
                    trim(&mut content);
                    // A badge is `<a href="the page"><img src="the picture">`,
                    // and an image inside a link would otherwise win: the
                    // fragmenter interns the *image's* destination for its
                    // whole run, so every badge would open the SVG it drew
                    // instead of the page it advertises. Unwrap the image and
                    // let its alt text carry the anchor's href.
                    if let [Inline::Image { alt, .. }] = content.as_slice() {
                        content = alt.clone();
                    }
                    if content.is_empty() {
                        // Nothing left to label it with, but the anchor still
                        // goes somewhere. Dropping it here would take a link
                        // off the page silently, so leave the placeholder the
                        // markdown path already uses for a captionless image.
                        if dest.is_empty() {
                            continue;
                        }
                        out.push(Inline::Image {
                            dest,
                            alt: Vec::new(),
                        });
                        continue;
                    }
                    out.push(Inline::Link { dest, content });
                }
                Role::Code => {
                    let mut content = Vec::new();
                    inlines_from(children, &mut content);
                    let text = Inline::plain_text(&content);
                    if !text.trim().is_empty() {
                        out.push(Inline::Code(text.trim().to_owned()));
                    }
                }
                Role::Strong => wrap_inline(children, Inline::Strong, out),
                Role::Emphasis => wrap_inline(children, Inline::Emphasis, out),
                Role::Strikethrough => wrap_inline(children, Inline::Strikethrough, out),
                // Block-level elements reached in an inline position, and
                // everything transparent: keep the words, drop the box.
                _ => inlines_from(children, out),
            },
        }
    }
}

/// What an `<img>` contributes, which is only ever its alt text.
///
/// `alt=""` written out is the author saying the image is decorative, and is
/// honoured: it is what keeps a row of separator images from rendering as a
/// row of placeholder icons. An `alt` that is simply *absent* says nothing at
/// all, so it behaves like markdown's `![](src)` and keeps the placeholder —
/// otherwise `<a href="page"><img src="badge"></a>` would vanish, link and
/// all.
fn image(attrs: &[(String, String)]) -> Option<Inline> {
    let dest = attr(attrs, "src").unwrap_or_default().to_owned();
    match attr(attrs, "alt") {
        Some(alt) if alt.trim().is_empty() => None,
        Some(alt) => Some(Inline::Image {
            dest,
            alt: vec![Inline::Text(collapse(alt).trim().to_owned())],
        }),
        None => Some(Inline::Image {
            dest,
            alt: Vec::new(),
        }),
    }
}

fn wrap_inline(children: &[Node], wrap: impl FnOnce(Vec<Inline>) -> Inline, out: &mut Vec<Inline>) {
    let mut content = Vec::new();
    inlines_from(children, &mut content);
    if !content.is_empty() {
        out.push(wrap(content));
    }
}

/// Append text, merging with the run before it so wrapping sees whole words.
fn push_text(text: &str, out: &mut Vec<Inline>) {
    // A `<br>` starts a line, so whitespace after it is the indentation of the
    // source rather than content. Same reasoning as trimming the ends.
    let text = if matches!(out.last(), Some(Inline::HardBreak)) {
        text.trim_start()
    } else {
        text
    };
    if text.is_empty() {
        return;
    }
    if let Some(Inline::Text(prev)) = out.last_mut() {
        prev.push_str(text);
        return;
    }
    out.push(Inline::Text(text.to_owned()));
}

/// Collapse whitespace the way HTML does: every run becomes one space, and a
/// newline is no different from a space.
///
/// This is what turns the source layout of a `<p align="center">` — one phrase
/// per line, each indented two spaces — back into a sentence.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// Drop leading and trailing whitespace from an inline run, so a paragraph
/// does not start with the indentation its source happened to have.
fn trim(content: &mut Vec<Inline>) {
    while let Some(Inline::Text(text)) = content.first_mut() {
        let trimmed = text.trim_start();
        if trimmed.len() == text.len() {
            break;
        }
        *text = trimmed.to_owned();
        if text.is_empty() {
            content.remove(0);
        } else {
            break;
        }
    }
    while let Some(last) = content.last_mut() {
        match last {
            Inline::Text(text) => {
                let trimmed = text.trim_end();
                if trimmed.len() == text.len() {
                    break;
                }
                *text = trimmed.to_owned();
                if text.is_empty() {
                    content.pop();
                } else {
                    break;
                }
            }
            Inline::HardBreak | Inline::SoftBreak => {
                content.pop();
            }
            _ => break,
        }
    }
}

// --- the inline path ------------------------------------------------------

/// What an opening inline tag asks the tree builder to do.
pub(super) enum InlineTag {
    /// Push a frame; the matching close pops it.
    Open(InlineKind),
    /// Contributes content and closes immediately.
    Void(Vec<Inline>),
}

/// Interpret one opening inline tag, as `Event::InlineHtml` delivers it.
///
/// `None` for a tag that cannot be scanned, or whose element contributes
/// nothing an inline run can hold.
pub(super) fn inline_open(raw: &str) -> Option<InlineTag> {
    let tokens = scan(raw)?;
    let (name, attrs, void) = tokens.iter().find_map(|token| match token {
        Token::Open { name, attrs } => Some((name, attrs, false)),
        Token::Void { name, attrs } => Some((name, attrs, true)),
        Token::Text(_) => None,
        Token::Close(_) => None,
    })?;

    match role(name) {
        Role::Break => Some(InlineTag::Void(vec![Inline::HardBreak])),
        Role::Rule => Some(InlineTag::Void(vec![Inline::HardBreak])),
        Role::Image => Some(InlineTag::Void(image(attrs).into_iter().collect())),
        _ if void => Some(InlineTag::Void(Vec::new())),
        Role::Link => Some(InlineTag::Open(InlineKind::Link(
            attr(attrs, "href").unwrap_or_default().to_owned(),
        ))),
        Role::Strong => Some(InlineTag::Open(InlineKind::Strong)),
        Role::Emphasis => Some(InlineTag::Open(InlineKind::Emphasis)),
        Role::Strikethrough => Some(InlineTag::Open(InlineKind::Strikethrough)),
        Role::Code => Some(InlineTag::Open(InlineKind::Code)),
        // Everything else opens a frame that contributes no styling, so that
        // its closing tag has something to pop and the text between the two
        // survives either way.
        _ => Some(InlineTag::Open(InlineKind::Transparent)),
    }
}

/// The frame kind a closing inline tag should look for.
pub(super) fn inline_kind(name: &str) -> Option<InlineKind> {
    match role(name) {
        Role::Link => Some(InlineKind::Link(String::new())),
        Role::Strong => Some(InlineKind::Strong),
        Role::Emphasis => Some(InlineKind::Emphasis),
        Role::Strikethrough => Some(InlineKind::Strikethrough),
        Role::Code => Some(InlineKind::Code),
        Role::Break | Role::Image => None, // void: nothing was ever opened
        _ => Some(InlineKind::Transparent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How deeply the element tree nests.
    fn node_depth(nodes: &[Node]) -> usize {
        nodes
            .iter()
            .map(|node| match node {
                Node::Element { children, .. } => 1 + node_depth(children),
                Node::Text(_) => 0,
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn html_nesting_past_the_cap_is_flattened_rather_than_followed() {
        // `blocks_from` and `inlines_from` walk this tree by recursion, so a
        // document of 8,000 nested divs aborted on a stack overflow — which
        // does not unwind, and so left the terminal unrestored.
        let deep = MAX_NESTING + 50;
        let raw = format!("{}x{}", "<div>".repeat(deep), "</div>".repeat(deep));
        let nodes = tree(scan(&raw).expect("scans"));
        assert_eq!(node_depth(&nodes), MAX_NESTING);
    }

    #[test]
    fn a_close_past_the_cap_matches_its_own_element_not_an_outer_one() {
        // Past the cap an element is not represented, but it stays on the
        // open stack, because `Token::Close` finds its match by *name*. Drop
        // the entry outright and this `</div>` matches the outer div instead
        // — closing it and everything inside it, and spilling the rest of the
        // document out to the root.
        //
        // The outer div plus the filler take the count exactly to the cap, so
        // the inner div is the first element past it. The filler is a
        // different tag on purpose: with every element named `div` the two
        // behaviours coincide and this test cannot tell them apart.
        let filler = MAX_NESTING - 1;
        let raw = format!("<div>{}<div>inner</div>after", "<b>".repeat(filler));
        let nodes = tree(scan(&raw).expect("scans"));
        assert_eq!(
            nodes.len(),
            1,
            "`after` belongs inside the outer div, not beside it: {nodes:#?}"
        );
        assert!(
            matches!(&nodes[0], Node::Element { name, .. } if name == "div"),
            "the one root node is the outer div: {nodes:#?}"
        );
        // Flattened, not dropped: what the inner div held still arrives.
        let mut text = String::new();
        fn collect(nodes: &[Node], out: &mut String) {
            for node in nodes {
                match node {
                    Node::Element { children, .. } => collect(children, out),
                    Node::Text(t) => out.push_str(t),
                }
            }
        }
        collect(&nodes, &mut text);
        assert_eq!(text, "innerafter");
    }

    fn names(raw: &str) -> Vec<String> {
        scan(raw)
            .expect("scans")
            .into_iter()
            .map(|token| match token {
                Token::Open { name, .. } => format!("<{name}>"),
                Token::Void { name, .. } => format!("<{name}/>"),
                Token::Close(name) => format!("</{name}>"),
                Token::Text(text) => text,
            })
            .collect()
    }

    #[test]
    fn a_greater_than_inside_a_quoted_value_does_not_end_the_tag() {
        // The bug a naive `find('>')` has, and alt text is prose, so it bites.
        let tokens = scan(r#"<img alt="a > b" src="x.png">"#).expect("scans");
        let [Token::Void { name, attrs }] = tokens.as_slice() else {
            panic!("one void tag, got {tokens:#?}");
        };
        assert_eq!(name, "img");
        assert_eq!(attr(attrs, "alt"), Some("a > b"));
        assert_eq!(attr(attrs, "src"), Some("x.png"));
    }

    #[test]
    fn attribute_values_may_be_single_quoted_or_bare() {
        let tokens = scan("<a href='x' target=_blank hidden>").expect("scans");
        let [Token::Open { attrs, .. }] = tokens.as_slice() else {
            panic!("one open tag, got {tokens:#?}");
        };
        assert_eq!(attr(attrs, "href"), Some("x"));
        assert_eq!(attr(attrs, "target"), Some("_blank"));
        assert_eq!(attr(attrs, "hidden"), Some(""));
    }

    #[test]
    fn void_elements_are_void_written_either_way() {
        assert_eq!(names("<br><br/><hr />"), ["<br/>", "<br/>", "<hr/>"]);
        // A non-void element written self-closing is still void.
        assert_eq!(names("<div/>"), ["<div/>"]);
    }

    #[test]
    fn tag_names_are_matched_without_regard_to_case() {
        assert_eq!(names("<BR><IMG SRC=x>"), ["<br/>", "<img/>"]);
    }

    #[test]
    fn comments_and_doctypes_carry_nothing() {
        assert_eq!(names("<!-- gone --><!DOCTYPE html>a"), ["a"]);
        assert_eq!(names("<!--\nspanning\nlines\n-->x"), ["x"]);
    }

    #[test]
    fn a_bare_less_than_is_text_not_a_tag() {
        // `a < b` in prose must survive, which is also what a browser does.
        assert_eq!(names("a < b and 1<2"), ["a < b and 1<2"]);
    }

    #[test]
    fn markup_that_does_not_lex_is_declined() {
        assert_eq!(scan("<div"), None, "unterminated tag");
        assert_eq!(
            scan(r#"<img alt="never closed>"#),
            None,
            "unterminated value"
        );
        assert_eq!(scan("<!-- unterminated"), None, "unterminated comment");
        assert_eq!(scan("<p <>"), None, "malformed attribute");
    }

    #[test]
    fn entities_decode_named_decimal_and_hexadecimal() {
        assert_eq!(decode_entities("a &amp; b"), "a & b");
        assert_eq!(decode_entities("&mdash;"), "—");
        assert_eq!(decode_entities("&#8212;"), "—");
        assert_eq!(decode_entities("&#x2014;"), "—");
        assert_eq!(decode_entities("&lt;div&gt;"), "<div>");
    }

    #[test]
    fn an_entity_we_do_not_know_is_left_exactly_as_written() {
        // Better to show `&notareal;` than a replacement character.
        assert_eq!(decode_entities("&notareal; &"), "&notareal; &");
        assert_eq!(decode_entities("Tom & Jerry"), "Tom & Jerry");
        assert_eq!(decode_entities("&#xZZZZ;"), "&#xZZZZ;");
    }

    // --- the tree ---------------------------------------------------------

    fn interpreted(raw: &str) -> Option<Vec<Block>> {
        let mut n = 0;
        interpret(raw, &(0..raw.len()), &mut |text| {
            n += 1;
            format!("{}-{n}", text.to_lowercase())
        })
    }

    #[test]
    fn an_unclosed_container_closes_itself_at_the_end_of_the_block() {
        // A CommonMark HTML block ends at a blank line, so the very common
        // `<div align=center>` / blank / markdown / blank / `</div>` idiom
        // hands us an opener with no closer. Rejecting it would leave markup
        // on screen for the most popular README shape there is.
        let blocks = interpreted("<div align=\"center\">\nText\n").expect("interpreted");
        let [block] = blocks.as_slice() else {
            panic!("one block, got {blocks:#?}");
        };
        assert_eq!(block.align, Alignment::Center);
        let BlockKind::Paragraph(content) = &block.kind else {
            panic!("a paragraph, got {:?}", block.kind);
        };
        assert_eq!(Inline::plain_text(content), "Text");
    }

    #[test]
    fn a_closing_tag_with_nothing_open_is_dropped() {
        // The other half of the same split block.
        let blocks = interpreted("</div>\n").expect("interpreted");
        assert!(blocks.is_empty(), "{blocks:#?}");
    }

    #[test]
    fn a_div_of_blocks_is_a_container_and_a_div_of_words_is_a_paragraph() {
        let blocks =
            interpreted("<div align=\"center\"><h2>T</h2><p>Body</p></div>").expect("interpreted");
        assert_eq!(blocks.len(), 2, "{blocks:#?}");
        assert!(matches!(
            blocks[0].kind,
            BlockKind::Heading { level: 2, .. }
        ));
        assert!(matches!(blocks[1].kind, BlockKind::Paragraph(_)));
        // Alignment is inherited by both.
        assert!(blocks.iter().all(|b| b.align == Alignment::Center));
    }

    #[test]
    fn opaque_elements_send_the_whole_block_back() {
        for raw in [
            "<table><tr><td>a</td></tr></table>",
            "<script>alert(1)</script>",
            "<style>body{}</style>",
            "<ul><li>one</li></ul>",
        ] {
            assert!(interpreted(raw).is_none(), "should decline: {raw}");
        }
    }

    #[test]
    fn an_unknown_element_keeps_its_words_and_drops_its_tag() {
        // The opposite policy to `Opaque`: in `render` mode the promise is
        // that no markup reaches the page, and the tail of tags nobody
        // enumerated is long. A reader who wants the markup has `literal`.
        let blocks = interpreted("<p>Press <kbd>Ctrl</kbd> and <my-widget>go</my-widget></p>")
            .expect("interpreted");
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("a paragraph, got {:?}", blocks[0].kind);
        };
        assert_eq!(Inline::plain_text(content), "Press Ctrl and go");
    }

    #[test]
    fn source_line_breaks_inside_a_paragraph_become_ordinary_spaces() {
        // What turns a `<p align="center">` written one phrase per line back
        // into a sentence.
        let blocks = interpreted("<p>\n  one\n  two\n  three\n</p>").expect("interpreted");
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("a paragraph, got {:?}", blocks[0].kind);
        };
        assert_eq!(Inline::plain_text(content), "one two three");
    }

    #[test]
    fn details_becomes_a_quote_titled_by_its_summary() {
        let blocks = interpreted("<details><summary>Title</summary><p>Body text.</p></details>")
            .expect("interpreted");
        let BlockKind::BlockQuote { alert, children } = &blocks[0].kind else {
            panic!("a quote, got {:?}", blocks[0].kind);
        };
        assert_eq!(*alert, None);
        assert_eq!(children.len(), 2, "{children:#?}");

        let BlockKind::Paragraph(title) = &children[0].kind else {
            panic!("a paragraph, got {:?}", children[0].kind);
        };
        // The summary is the title, and it is strong so it reads as one.
        assert!(
            matches!(title.as_slice(), [Inline::Strong(_)]),
            "{title:#?}"
        );
        assert_eq!(Inline::plain_text(title), "Title");
        assert_eq!(
            Inline::plain_text(match &children[1].kind {
                BlockKind::Paragraph(c) => c,
                other => panic!("a paragraph, got {other:?}"),
            }),
            "Body text."
        );
    }

    #[test]
    fn a_details_with_no_body_does_not_quote_a_lone_title() {
        // The form GitHub requires for markdown inside: a blank line ends the
        // HTML block, so the open tag arrives without its body. A gutter bar
        // around nothing but a title reads as a rendering bug.
        let blocks =
            interpreted("<details><summary>Title</summary></details>").expect("interpreted");
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("a bare paragraph, got {:?}", blocks[0].kind);
        };
        assert_eq!(Inline::plain_text(content), "Title");
    }

    #[test]
    fn details_no_longer_falls_back_to_literal_markup() {
        // The regression this guards: `details`/`summary` back on the opaque
        // list, which sends the whole block to the page as tags.
        assert!(interpreted("<details><summary>S</summary><p>B</p></details>").is_some());
    }

    #[test]
    fn an_image_declared_decorative_contributes_nothing() {
        // `alt=""` is the author saying so; this keeps a row of separator
        // images from rendering as a row of placeholder icons.
        let blocks = interpreted("<p><img alt=\"\" src=\"x.svg\"> after</p>").expect("interpreted");
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("a paragraph, got {:?}", blocks[0].kind);
        };
        assert_eq!(content.len(), 1, "{content:#?}");
        assert_eq!(Inline::plain_text(content), "after");
    }

    #[test]
    fn a_link_around_an_unlabelled_image_still_goes_somewhere() {
        // `<a href="page"><img src="badge"></a>` — no alt to label it with,
        // but the anchor still points at a page. Dropping the pair took a
        // link off the document without saying so; markdown's
        // `[![](img)](page)` has always kept its placeholder.
        let blocks = interpreted("<p><a href=\"https://page\"><img src=\"b.svg\"></a></p>")
            .expect("interpreted");
        let BlockKind::Paragraph(content) = &blocks[0].kind else {
            panic!("a paragraph, got {:?}", blocks[0].kind);
        };
        let [Inline::Image { dest, alt }] = content.as_slice() else {
            panic!("a placeholder pointing at the page, got {content:#?}");
        };
        assert_eq!(dest, "https://page");
        assert!(alt.is_empty());
    }

    #[test]
    fn every_mode_round_trips_through_its_name() {
        for mode in HtmlMode::ALL {
            assert_eq!(mode.name().parse::<HtmlMode>(), Ok(mode));
        }
        assert!("nonsense".parse::<HtmlMode>().is_err());
    }
}
