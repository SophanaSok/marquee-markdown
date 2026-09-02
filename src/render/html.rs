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
//! and it never will — anything needing that is opaque to it and goes back to
//! the caller untouched. The two tag-omission rules it knows are the ones
//! tables and lists lean on — `<tr><td>a<td>b` and `<li>a<li>b` — and each is
//! applied by the walk that needs it, `Walk` and `lift_items`, rather than by
//! the tree.

use std::ops::Range;
use std::str::FromStr;

use super::block::{Alignment, Block, BlockKind, Inline, ListItem, MAX_NESTING};
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
    /// `<ul>` or `<ol>` — becomes the same block a markdown list does, and is
    /// drawn by the same emitter. See [`list_block`].
    List,
    /// `<li>` — one item of a list. Only meaningful inside one; a run of them
    /// met loose is gathered back into a list, which is what a blank line
    /// inside a `<ul>` leaves behind.
    ListItem,
    /// `<table>` — becomes the same block a markdown pipe table does, and is
    /// drawn by the same emitter. See [`table_blocks`].
    Table,
    /// `<thead>`, `<tbody>`, `<tfoot>` or `<tr>` met at block level, which is
    /// what a blank line inside a table leaves behind: a `CommonMark` HTML
    /// block ends at the blank line, so the rest of the table arrives as later
    /// blocks with its rows at the root. A run of them is laid out as a table
    /// of its own.
    TableFragment,
    /// Understood well enough to know this renderer would make a mess of it.
    /// One of these anywhere sends the whole block back to be rendered as
    /// literal text, because the markup carries more than the flattened words
    /// would: a list read as one run-on sentence is worse than a list read as
    /// tags.
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
                | Self::List
                | Self::ListItem
                | Self::Table
                | Self::TableFragment
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
        // `<center>` is a `<div align="center">` with the alignment in its
        // name; `alignment` reads it from there.
        "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "center" => {
            Role::Paragraph
        }
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
        "ul" | "ol" => Role::List,
        "li" => Role::ListItem,
        "table" => Role::Table,
        "thead" | "tbody" | "tfoot" | "tr" => Role::TableFragment,
        // The rest of a table's vocabulary means something only inside the
        // table walk, which matches on the name; loose, a cell is its words.
        // Structure this renderer has no emitter for. Literal is the honest
        // answer: the tags say more than the words would on their own.
        "dl" | "dt" | "dd" | "pre" | "script" | "style" | "textarea" | "iframe" | "object"
        | "embed" | "svg" | "math" | "form" | "input" | "button" | "select" | "option"
        | "textpath" => Role::Opaque,
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
        // `middle` is the legacy spelling for a table cell; old hand-written
        // tables still carry it.
        Some("center" | "middle") => Some(Alignment::Center),
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
    // One pass for both reasons to decline. `open` counts tables currently
    // open: a table inside a table is declined, because the inner one has
    // nowhere to go — a cell holds inline content, so nesting could only
    // flatten it, and a table read as one run-on sentence is the thing this
    // module exists to avoid.
    let mut open = 0usize;
    for token in &tokens {
        let name = match token {
            Token::Open { name, .. } | Token::Void { name, .. } | Token::Close(name) => name,
            Token::Text(_) => continue,
        };
        if role(name) == Role::Opaque {
            return None;
        }
        if name == "table" {
            match token {
                Token::Open { .. } => {
                    open += 1;
                    if open > 1 {
                        return None;
                    }
                }
                Token::Close(_) => open = open.saturating_sub(1),
                // `<table/>` holds nothing; it opens nothing either.
                Token::Void { .. } | Token::Text(_) => {}
            }
        }
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

    let mut index = 0;
    while index < nodes.len() {
        let node = &nodes[index];
        index += 1;
        let Node::Element {
            name,
            attrs,
            children,
        } = node
        else {
            inlines_from(std::slice::from_ref(node), &mut pending);
            continue;
        };
        let role = self::role(name);
        // A wrapper with no meaning of its own still delimits blocks: a
        // `<span>` or a custom element around a heading is a container, not a
        // sentence. Deciding by content is the same rule `<div>` uses, so the
        // container case is handled the same way too.
        let container = !role.is_block() && holds_blocks(children);
        if !role.is_block() && !container {
            inlines_from(std::slice::from_ref(node), &mut pending);
            continue;
        }

        flush(&mut pending, span, out);
        let align = alignment(name, attrs);
        let mut produced = Vec::new();
        match role {
            _ if container => blocks_from(children, span, slug, &mut produced),
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
            Role::List => list_block(name, attrs, children, span, slug, &mut produced),
            // A blank line ends a `CommonMark` HTML block, and writing one
            // inside `<ul>` is how GitHub asks for markdown to render in an
            // item — so the items arrive at the root of a later block with
            // their list left behind. Gather the run and give it one back.
            // Which list is lost with the tag, so the run is bulleted; an
            // `<ol>` split this way loses its numbers.
            Role::ListItem => {
                let run = gather(nodes, node, &mut index, Role::ListItem);
                list_block("ul", &[], &run, span, slug, &mut produced);
            }
            Role::Table => table_blocks(children, span, &mut produced),
            // A blank line ends a `CommonMark` HTML block, so a table written
            // with one inside it arrives in pieces: `<table>` alone, then its
            // rows or sections at the root of a later block. Gather the run of
            // them and lay it out as a table of its own, which is the shape
            // the author wrote even if the frame is drawn once per piece.
            Role::TableFragment => {
                let run = gather(nodes, node, &mut index, Role::TableFragment);
                table_blocks(&run, span, &mut produced);
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

/// Collect the run of same-role siblings starting at `node`, advancing `index`
/// past the ones it takes.
///
/// A `CommonMark` HTML block ends at a blank line, so a list or a table
/// written with one inside it arrives in pieces, with the parts that were
/// inside the container loose at the root of a later block. The run is the
/// shape the author wrote, even though the tag saying so was left behind.
fn gather(nodes: &[Node], node: &Node, index: &mut usize, want: Role) -> Vec<Node> {
    let mut run = vec![node.clone()];
    while let Some(next) = nodes.get(*index) {
        match next {
            Node::Element { name, .. } if role(name) == want => run.push(next.clone()),
            // The whitespace between two tags is not a row or an item, and it
            // is not content either.
            Node::Text(text) if collapse(text).trim().is_empty() => {}
            _ => break,
        }
        *index += 1;
    }
    run
}

// --- lists ----------------------------------------------------------------

/// Lay a `<ul>` or `<ol>`'s children out as the same block a markdown list
/// produces, so both reach the one emitter: same markers, same indent, same
/// handling of a list nested in an item.
///
/// An item holds blocks, so everything [`blocks_from`] can make reaches an
/// item too — a paragraph, a heading, a quote, a table, another list. What
/// HTML can say and the block cannot is dropped: `type` picks a marker glyph,
/// `reversed` counts down, and `value` renumbers one item, none of which the
/// block represents. A `<li>` holding a task checkbox is not a task list
/// either; `<input>` is opaque, so a block containing one never gets here.
fn list_block(
    name: &str,
    attrs: &[(String, String)],
    children: &[Node],
    span: &Range<usize>,
    slug: &mut dyn FnMut(&str) -> String,
    out: &mut Vec<Block>,
) {
    // `<ol start>` is the one numbering attribute the block carries. A value
    // that is not a number is the author saying nothing, not the author
    // saying zero.
    let start = (name == "ol").then(|| {
        attr(attrs, "start")
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(1)
    });

    let mut nodes = Vec::new();
    for child in children {
        lift_items(child, &mut nodes);
    }

    let mut items: Vec<ListItem> = Vec::new();
    let mut loose: Vec<Node> = Vec::new();
    for node in &nodes {
        match node {
            Node::Element { name, children, .. } if role(name) == Role::ListItem => {
                close_item(&mut items, &mut loose, span, slug);
                let mut blocks = Vec::new();
                blocks_from(children, span, slug, &mut blocks);
                items.push(ListItem {
                    task: None,
                    children: blocks,
                });
            }
            // The whitespace laying the source out is not an item.
            Node::Text(text) if collapse(text).trim().is_empty() => {}
            other => loose.push(other.clone()),
        }
    }
    close_item(&mut items, &mut loose, span, slug);

    if !items.is_empty() {
        out.push(Block::at(BlockKind::List { start, items }, span.clone()));
    }
}

/// Attach content found loose in a list to the item above it, or make an item
/// of it when it came before the first one.
///
/// A list may hold only items, and authors put a stray `<p>` or a bare
/// sentence in one anyway. Both readings keep the words on the page; dropping
/// them is the one thing that must not happen.
fn close_item(
    items: &mut Vec<ListItem>,
    loose: &mut Vec<Node>,
    span: &Range<usize>,
    slug: &mut dyn FnMut(&str) -> String,
) {
    if loose.is_empty() {
        return;
    }
    let nodes = std::mem::take(loose);
    let mut blocks = Vec::new();
    blocks_from(&nodes, span, slug, &mut blocks);
    if blocks.is_empty() {
        return;
    }
    match items.last_mut() {
        Some(item) => item.children.append(&mut blocks),
        None => items.push(ListItem {
            task: None,
            children: blocks,
        }),
    }
}

/// Lift the siblings an item swallowed back out of it.
///
/// `</li>` is the end tag a list author leaves out most, and a tree built by
/// name-matching nests what they meant to close: in `<li>a<li>b`, the second
/// item is a *child* of the first. A direct `<li>` child is never what an
/// author means — a list nested in an item arrives wrapped in its own `<ul>`
/// — so one found here is a sibling. This is the list's half of the rule
/// [`Walk`] applies to `<tr><td>a<td>b`, and it unwinds a whole run of
/// omissions because each lifted item is lifted again in turn.
fn lift_items(node: &Node, out: &mut Vec<Node>) {
    let Node::Element {
        name,
        attrs,
        children,
    } = node
    else {
        out.push(node.clone());
        return;
    };
    let split = if role(name) == Role::ListItem {
        children.iter().position(
            |child| matches!(child, Node::Element { name, .. } if role(name) == Role::ListItem),
        )
    } else {
        None
    };
    let Some(split) = split else {
        out.push(node.clone());
        return;
    };
    out.push(Node::Element {
        name: name.clone(),
        attrs: attrs.clone(),
        children: children[..split].to_vec(),
    });
    for rest in &children[split..] {
        lift_items(rest, out);
    }
}

// --- tables ---------------------------------------------------------------

/// Largest `colspan`/`rowspan` honoured, which is the value HTML's own parser
/// clamps `colspan` to. Nothing is allocated per span — the number only bounds
/// the arithmetic — but a cell claiming a million columns still has to stop
/// somewhere.
const MAX_SPAN: usize = 1000;

/// Which part of the table a row belongs to. `<tfoot>` is rendered last
/// whatever its source position, as a browser does, and the header is chosen
/// after that move so a footer row can never become the header.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    Head,
    #[default]
    Body,
    Foot,
}

/// One cell, placed on the grid but not yet given a dense column index.
struct Cell {
    column: usize,
    /// Columns the cell covers, from `colspan`. Only the first holds the
    /// content; the rest push the cells after it along.
    width: usize,
    /// `<th>`, which decides the header row and bolds a row-header cell.
    header: bool,
    align: Option<Alignment>,
    content: Vec<Inline>,
}

/// One row of placed cells, with the columns a `rowspan` from an earlier row
/// has taken out of it.
struct Row {
    section: Section,
    align: Option<Alignment>,
    cells: Vec<Cell>,
}

/// A column range one cell holds down for the rows below it.
struct Reservation {
    columns: Range<usize>,
    /// Last row index the reservation covers.
    through: usize,
}

/// Lay a `<table>`'s children out as the same block a markdown pipe table
/// produces, so both are drawn by the one emitter and the column solver has no
/// idea which it is looking at.
///
/// A `<caption>` becomes a paragraph before the table, sharing its alignment.
/// Nothing else is added to the page: the frame, the header band and the
/// narrow-width card fallback all belong to the emitter.
fn table_blocks(nodes: &[Node], span: &Range<usize>, out: &mut Vec<Block>) {
    let mut walk = Walk::default();
    walk.nodes(nodes);
    let Walk {
        mut rows,
        caption,
        table_align,
        ..
    } = walk;

    if let Some(content) = caption {
        out.push(Block {
            kind: BlockKind::Paragraph(vec![Inline::Strong(content)]),
            span: span.clone(),
            align: table_align.unwrap_or_default(),
        });
    }

    // `<tfoot>` last, everything else in the order it was written. A stable
    // sort by section is the whole of it.
    rows.sort_by_key(|row| row.section);

    // The header is the last `<thead>` row — an earlier one is a spanning
    // title above the labels, not the labels — or else a leading row whose
    // every cell is a `<th>`.
    let head_count = rows.iter().filter(|r| r.section == Section::Head).count();
    let header_at = if head_count > 0 {
        Some(head_count - 1)
    } else {
        rows.first()
            .filter(|row| !row.cells.is_empty() && row.cells.iter().all(|c| c.header))
            .map(|_| 0)
    };

    // Dense column indices, from the columns that actually hold something.
    // This drops the spacer columns authors use for gutters, and makes a
    // `colspan` of a thousand cost one column rather than a thousand.
    let mut used: Vec<usize> = rows
        .iter()
        .flat_map(|row| &row.cells)
        .filter(|cell| !cell.content.is_empty())
        .map(|cell| cell.column)
        .collect();
    used.sort_unstable();
    used.dedup();
    if used.is_empty() {
        return;
    }
    let dense = |column: usize| used.binary_search(&column).ok();

    let columns = used.len();
    let mut alignments: Vec<Option<Alignment>> = vec![None; columns];
    let mut header: Vec<Vec<Inline>> = Vec::new();
    let mut body: Vec<Vec<Vec<Inline>>> = Vec::new();

    for (index, row) in rows.iter().enumerate() {
        let is_header = header_at == Some(index);
        let mut cells: Vec<Vec<Inline>> = vec![Vec::new(); columns];
        for cell in &row.cells {
            let Some(column) = dense(cell.column) else {
                continue; // an empty cell in a column nothing else filled
            };
            // A column takes its alignment from the header cell if that one
            // states any, else from the first body cell that does; a `<tr
            // align>` stands in for a cell that says nothing.
            let stated = cell.align.or(row.align);
            if let Some(align) = stated
                && (is_header || alignments[column].is_none())
            {
                alignments[column] = Some(align);
            }
            // A `<th>` in a body row is a row header. There is no column for
            // it in the block, so it keeps its weight instead.
            cells[column] = if cell.header && !is_header && !cell.content.is_empty() {
                vec![Inline::Strong(cell.content.clone())]
            } else {
                cell.content.clone()
            };
        }
        // Trailing empties would draw a blank line each in card layout, and
        // the emitter reads a short row as empty cells anyway.
        while cells.last().is_some_and(Vec::is_empty) {
            cells.pop();
        }
        if is_header {
            header = cells;
        } else if !cells.is_empty() {
            body.push(cells);
        }
    }

    if header.is_empty() && body.is_empty() {
        return;
    }
    out.push(Block {
        kind: BlockKind::Table {
            alignments: alignments
                .into_iter()
                .map(Option::unwrap_or_default)
                .collect(),
            header,
            rows: body,
        },
        span: span.clone(),
        align: table_align.unwrap_or_default(),
    });
}

/// The table walk's state.
///
/// Flat on purpose. `</td>` and `</tr>` are the end tags authors leave out
/// most, and a tree built by name-matching nests what they meant to close: in
/// `<tr><td>a<td>b`, the second cell is a *child* of the first. So the walk
/// tracks the open row and cell as indices and lifts a cell or row met inside
/// another one out to where it belongs, at any depth. Anything else met with
/// no cell open opens one — which is also how the words survive when
/// [`MAX_NESTING`] flattens a `<tr>` and splices its cells into the table.
#[derive(Default)]
struct Walk {
    rows: Vec<Row>,
    caption: Option<Vec<Inline>>,
    table_align: Option<Alignment>,
    section: Section,
    /// Reservations from `rowspan`, one per spanning cell rather than one per
    /// covered position: a `rowspan` costs nothing to honour and nothing to
    /// ignore once the rows it names do not exist.
    reserved: Vec<Reservation>,
    row: Option<usize>,
    cell: Option<usize>,
}

impl Walk {
    fn nodes(&mut self, nodes: &[Node]) {
        for node in nodes {
            match node {
                Node::Text(text) => {
                    let text = collapse(&decode_entities(text));
                    if !text.trim().is_empty() {
                        self.content(std::slice::from_ref(node));
                    }
                }
                Node::Element {
                    name,
                    attrs,
                    children,
                } => self.element(name, attrs, children, node),
            }
        }
    }

    fn element(&mut self, name: &str, attrs: &[(String, String)], children: &[Node], node: &Node) {
        match name {
            "table" => {
                // Only reachable for a table inside a table, which `interpret`
                // declines — but the walk does not depend on that: the inner
                // table's words go into the open cell rather than nowhere.
                self.table_align = self.table_align.or_else(|| alignment(name, attrs));
                self.nodes(children);
            }
            "caption" => {
                let mut content = Vec::new();
                inlines_from(children, &mut content);
                trim(&mut content);
                // HTML uses the first caption and ignores the rest.
                if self.caption.is_none() && !content.is_empty() {
                    self.caption = Some(content);
                }
            }
            "thead" | "tbody" | "tfoot" => {
                let outer = self.section;
                self.section = match name {
                    "thead" => Section::Head,
                    "tfoot" => Section::Foot,
                    _ => Section::Body,
                };
                self.close_row();
                self.nodes(children);
                self.close_row();
                self.section = outer;
            }
            // Columns describe widths this renderer solves for itself.
            "colgroup" | "col" => {}
            "tr" => {
                self.close_row();
                self.open_row(alignment(name, attrs));
                let opened = self.row;
                self.nodes(children);
                // Only close the row this element opened: a lifted `<tr>`
                // inside it has already replaced the open one.
                if self.row == opened {
                    self.close_row();
                }
            }
            "td" | "th" => {
                self.open_cell(name == "th", alignment(name, attrs), attrs);
                let opened = (self.row, self.cell);
                self.nodes(children);
                if (self.row, self.cell) == opened {
                    self.cell = None;
                }
            }
            _ => {
                // Everything else is cell content — including a block-level
                // element, whose words would otherwise run into the next one.
                if role(name).is_block() && self.cell.is_some() {
                    self.hard_break();
                    self.content(std::slice::from_ref(node));
                    self.hard_break();
                } else {
                    self.content(std::slice::from_ref(node));
                }
            }
        }
    }

    /// Append inline content to the open cell, opening one if the table put
    /// content where only a cell can hold it.
    fn content(&mut self, nodes: &[Node]) {
        // A cell lifted out of another cell is not content: it is the next
        // cell, and `element` handles it. Everything reaching here is words.
        if self.cell.is_none() {
            self.open_cell(false, None, &[]);
        }
        let Some(cell) = self.current_cell() else {
            return;
        };
        inlines_from(nodes, cell);
    }

    fn hard_break(&mut self) {
        if let Some(cell) = self.current_cell()
            && !cell.is_empty()
            && !matches!(cell.last(), Some(Inline::HardBreak))
        {
            cell.push(Inline::HardBreak);
        }
    }

    fn current_cell(&mut self) -> Option<&mut Vec<Inline>> {
        let row = self.row?;
        let cell = self.cell?;
        Some(&mut self.rows[row].cells[cell].content)
    }

    fn open_row(&mut self, align: Option<Alignment>) {
        let index = self.rows.len();
        // Reservations that end above this row can never fire again.
        self.reserved.retain(|r| r.through >= index);
        self.rows.push(Row {
            section: self.section,
            align,
            cells: Vec::new(),
        });
        self.row = Some(index);
        self.cell = None;
    }

    fn close_row(&mut self) {
        if let Some(row) = self.row.take() {
            trim_cells(&mut self.rows[row].cells);
        }
        self.cell = None;
    }

    fn open_cell(&mut self, header: bool, align: Option<Alignment>, attrs: &[(String, String)]) {
        if self.row.is_none() {
            self.open_row(None);
        }
        if let Some(cell) = self.current_cell() {
            trim(cell);
        }
        let row = self.row.expect("a row was just opened");
        let index = self.rows[row].cells.len();
        // The next column no earlier row is holding down, and no cell in this
        // row has taken.
        let mut column = self.rows[row]
            .cells
            .last()
            .map_or(0, |last| last.column.saturating_add(last.width));
        while self
            .reserved
            .iter()
            .any(|r| r.through >= row && r.columns.contains(&column))
        {
            column += 1;
        }
        let columns = span_of(attrs, "colspan");
        let rows = span_of(attrs, "rowspan");
        if rows > 1 {
            self.reserved.push(Reservation {
                columns: column..column.saturating_add(columns),
                through: row.saturating_add(rows - 1),
            });
        }
        // A `colspan` is the one thing a single-column block cannot say, so
        // the cell keeps its own column, the columns it covers stay empty —
        // and are dropped unless another row fills them — and the cells after
        // it start beyond them.
        self.rows[row].cells.push(Cell {
            column,
            width: columns,
            header,
            align,
            content: Vec::new(),
        });
        self.cell = Some(index);
    }
}

/// Trim every cell of a finished row.
fn trim_cells(cells: &mut [Cell]) {
    for cell in cells {
        trim(&mut cell.content);
    }
}

/// A `colspan`/`rowspan` value, by HTML's non-negative-integer rules: leading
/// whitespace and a `+` are allowed, the digits that follow are the number,
/// and anything else — `0`, `auto`, empty — is one.
fn span_of(attrs: &[(String, String)], name: &str) -> usize {
    let raw = attr(attrs, name).unwrap_or_default();
    let digits: String = raw
        .trim_start()
        .trim_start_matches('+')
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse::<usize>().unwrap_or(1).clamp(1, MAX_SPAN)
}

/// Whether any descendant would become a block, deciding container from
/// paragraph.
///
/// Looks through elements that are not blocks themselves, so a `<table>` inside
/// a `<span>` inside a `<center>` still makes each of them a container. Bounded
/// by [`MAX_NESTING`], which is what caps the tree's depth.
fn holds_blocks(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| match node {
        Node::Element { name, children, .. } => role(name).is_block() || holds_blocks(children),
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
                // A list reached in an inline position is a list in a table
                // cell, because a cell holds inline content and nothing else.
                // One item per line is what survives of it. No marker is
                // synthesized: one added here would land in the plain mirror
                // and become searchable text, which the markers the layout
                // engine draws never are.
                Role::List => {
                    break_between(out);
                    inlines_from(children, out);
                    break_between(out);
                }
                Role::ListItem => {
                    break_between(out);
                    inlines_from(children, out);
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

/// Break the line between two runs that had no `<br>` between them.
///
/// Unlike `<br>`, which an author writes to make a blank line and which starts
/// one wherever it lands, this only breaks where there is something to break
/// from and no break already — so a cell does not open on an empty line, and
/// two adjacent lists do not leave one between them.
fn break_between(out: &mut Vec<Inline>) {
    if let Some(Inline::Text(prev)) = out.last_mut() {
        let trimmed = prev.trim_end().to_owned();
        if trimmed.is_empty() {
            out.pop();
        } else {
            *prev = trimmed;
        }
    }
    if !out.is_empty() && !matches!(out.last(), Some(Inline::HardBreak)) {
        out.push(Inline::HardBreak);
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
    /// Starts a line, but only where there is one to start: a list item in an
    /// inline position has no marker to draw, so its own line is all that is
    /// left to tell it from the item before it.
    BreakBetween,
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
        // A `<ul>` in a markdown table cell is how a cell gets a list, and a
        // cell holds inline content: one item per line is what survives. The
        // same shape `inlines_from` gives a list inside an HTML `<td>`.
        Role::ListItem => Some(InlineTag::BreakBetween),
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
        // Nothing was ever opened: void elements, and an item that only ever
        // started a line.
        Role::Break | Role::Image | Role::ListItem => None,
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
            "<dl><dt>term</dt><dd>meaning</dd></dl>",
            "<script>alert(1)</script>",
            "<style>body{}</style>",
            "<pre>preformatted</pre>",
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

    // --- tables -----------------------------------------------------------

    /// The one table a snippet interprets to, flattened to plain text:
    /// alignments, the header row, then the body.
    fn table(raw: &str) -> (Vec<Alignment>, Vec<String>, Vec<Vec<String>>) {
        let blocks = interpreted(raw).expect("interpreted");
        let Some(BlockKind::Table {
            alignments,
            header,
            rows,
        }) = blocks
            .iter()
            .map(|b| &b.kind)
            .find(|kind| matches!(kind, BlockKind::Table { .. }))
        else {
            panic!("a table, got {blocks:#?}");
        };
        let text = |cells: &Vec<Vec<Inline>>| -> Vec<String> {
            cells.iter().map(|c| Inline::plain_text(c)).collect()
        };
        (
            alignments.clone(),
            text(header),
            rows.iter().map(text).collect(),
        )
    }

    #[test]
    fn a_table_of_rows_and_cells_becomes_a_table_block() {
        let (alignments, header, rows) = table(
            "<table><tr><th>Name</th><th>Value</th></tr>\
             <tr><td>a</td><td>1</td></tr>\
             <tr><td>b</td><td>2</td></tr></table>",
        );
        assert_eq!(alignments, [Alignment::Left, Alignment::Left]);
        assert_eq!(header, ["Name", "Value"]);
        assert_eq!(rows, [["a", "1"], ["b", "2"]]);
    }

    #[test]
    fn the_header_is_the_last_thead_row_and_the_ones_above_it_are_body() {
        // A two-row `<thead>` is nearly always a spanning title over the
        // labels. Taking the first row would label every column with the
        // title and leave the labels loose in the body.
        let (_, header, rows) = table(
            "<table><thead>\
             <tr><th colspan=\"2\">Spanning title</th></tr>\
             <tr><th>Name</th><th>Value</th></tr>\
             </thead><tbody><tr><td>a</td><td>1</td></tr></tbody></table>",
        );
        assert_eq!(header, ["Name", "Value"]);
        assert_eq!(rows, [vec!["Spanning title"], vec!["a", "1"]]);
    }

    #[test]
    fn a_leading_row_of_only_th_is_the_header_without_a_thead() {
        // The shape a hand-written README table has, `<thead>` being the tag
        // authors leave out most.
        let (_, header, rows) =
            table("<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>");
        assert_eq!(header, ["A", "B"]);
        assert_eq!(rows, [["1", "2"]]);
    }

    #[test]
    fn a_table_whose_first_row_holds_a_td_has_no_header() {
        let (_, header, rows) =
            table("<table><tr><td>1</td><td>2</td></tr><tr><td>3</td><td>4</td></tr></table>");
        assert!(header.is_empty(), "{header:#?}");
        assert_eq!(rows, [["1", "2"], ["3", "4"]]);
    }

    #[test]
    fn a_th_in_a_body_row_keeps_its_weight_as_a_row_header() {
        // There is no row-header column in the block, so the only way to say
        // "this cell labels its row" is the weight the author asked for.
        let blocks = interpreted(
            "<table><tr><td>a</td><td>b</td></tr><tr><th>Label</th><td>c</td></tr></table>",
        )
        .expect("interpreted");
        let BlockKind::Table { rows, .. } = &blocks[0].kind else {
            panic!("a table, got {:?}", blocks[0].kind);
        };
        assert!(
            matches!(rows[1][0].as_slice(), [Inline::Strong(_)]),
            "{:#?}",
            rows[1][0]
        );
    }

    #[test]
    fn a_column_takes_its_alignment_from_the_header_then_the_first_body_cell() {
        let (alignments, ..) = table(
            "<table>\
             <tr><th align=\"right\">R</th><th align=\"middle\">C</th><th>?</th></tr>\
             <tr><td>1</td><td>2</td><td align=\"center\">3</td></tr>\
             <tr><td align=\"right\">4</td><td>5</td><td align=\"right\">6</td></tr>\
             </table>",
        );
        // `middle` is the legacy cell spelling of `center`; the third column
        // is stated by the first body cell that states anything, and a later
        // row does not overrule it.
        assert_eq!(
            alignments,
            [Alignment::Right, Alignment::Center, Alignment::Center]
        );
    }

    #[test]
    fn a_row_alignment_stands_in_for_a_cell_that_states_none() {
        let (alignments, ..) =
            table("<table><tr align=\"center\"><td>1</td><td align=\"right\">2</td></tr></table>");
        assert_eq!(alignments, [Alignment::Center, Alignment::Right]);
    }

    #[test]
    fn a_colspan_keeps_one_column_and_pushes_the_cells_after_it_along() {
        let (_, _, rows) = table(
            "<table><tr><td colspan=\"2\">wide</td><td>c</td></tr>\
             <tr><td>a</td><td>b</td><td>d</td></tr></table>",
        );
        // The covered column stays empty rather than repeating the text, and
        // `c` lands over `d` where the author put it.
        assert_eq!(rows, [["wide", "", "c"], ["a", "b", "d"]]);
    }

    #[test]
    fn a_rowspan_holds_its_column_down_for_the_rows_below() {
        let (_, _, rows) = table(
            "<table><tr><td rowspan=\"2\">tall</td><td>b</td></tr>\
             <tr><td>c</td></tr></table>",
        );
        assert_eq!(rows, [["tall", "b"], ["", "c"]]);
    }

    #[test]
    fn a_span_of_a_thousand_columns_costs_two_columns_not_a_thousand() {
        // The allocation bomb this design exists to avoid: nothing is stored
        // per covered position, so a document cannot multiply its own cell
        // count by writing a large span.
        let (alignments, _, rows) = table(
            "<table><tr><td colspan=\"1000\" rowspan=\"1000\">wide</td><td>b</td></tr>\
             <tr><td>c</td></tr></table>",
        );
        assert_eq!(alignments.len(), 2);
        assert_eq!(rows, [["wide", "b"], ["", "c"]]);
    }

    #[test]
    fn a_rowspan_over_two_hundred_rows_places_every_one_of_them() {
        let mut raw = String::from("<table><tr><td rowspan=\"200\">tall</td><td>0</td></tr>");
        for row in 1..200 {
            raw.push_str(&format!("<tr><td>{row}</td></tr>"));
        }
        raw.push_str("</table>");
        let (alignments, _, rows) = table(&raw);
        assert_eq!(alignments.len(), 2);
        assert_eq!(rows.len(), 200);
        assert_eq!(rows[0], ["tall", "0"]);
        assert_eq!(rows[199], ["", "199"]);
    }

    #[test]
    fn a_column_nothing_fills_is_dropped() {
        // Authors use an empty column as a gutter. Keeping it would spend
        // width on a separator the frame already draws.
        let (alignments, _, rows) = table(
            "<table><tr><td>a</td><td></td><td>b</td></tr>\
             <tr><td>c</td><td>   </td><td>d</td></tr></table>",
        );
        assert_eq!(alignments.len(), 2);
        assert_eq!(rows, [["a", "b"], ["c", "d"]]);
    }

    #[test]
    fn an_omitted_closing_cell_or_row_tag_still_lands_where_it_was_meant_to() {
        // `</td>` and `</tr>` are the end tags HTML lets you leave out, and a
        // tree built by name-matching nests what they meant to close: the
        // second cell arrives as a *child* of the first.
        let (_, _, rows) = table("<table><tr><td>a<td>b<tr><td>c<td>d</table>");
        assert_eq!(rows, [["a", "b"], ["c", "d"]]);
    }

    #[test]
    fn a_table_inside_a_table_is_declined_but_two_side_by_side_are_not() {
        // A cell holds inline content, so nesting could only flatten the
        // inner table into one run-on sentence — the thing this module
        // exists to avoid. Declining shows the markup instead, which at
        // least says what it is.
        assert!(
            interpreted("<table><tr><td><table><tr><td>x</td></tr></table></td></tr></table>")
                .is_none()
        );
        let blocks =
            interpreted("<table><tr><td>a</td></tr></table><table><tr><td>b</td></tr></table>")
                .expect("interpreted");
        assert_eq!(
            blocks
                .iter()
                .filter(|b| matches!(b.kind, BlockKind::Table { .. }))
                .count(),
            2,
            "{blocks:#?}"
        );
    }

    #[test]
    fn a_cell_holding_an_opaque_element_still_declines_the_whole_block() {
        // A known limitation, recorded so a change to it is deliberate: the
        // opaque scan runs over the whole block, so one `<pre>` in one cell
        // sends the table to the page as literal markup. Scoping the scan to
        // cells is a follow-up.
        assert!(interpreted("<table><tr><td><pre>a</pre></td></tr></table>").is_none());
    }

    /// The plain text of every item of the one list `raw` produces.
    fn items(raw: &str) -> (Option<u64>, Vec<String>) {
        let blocks = interpreted(raw).expect("interpreted");
        let [
            Block {
                kind: BlockKind::List { start, items },
                ..
            },
        ] = blocks.as_slice()
        else {
            panic!("one list, got {blocks:#?}");
        };
        let text = |item: &ListItem| {
            item.children
                .iter()
                .map(|block| match &block.kind {
                    BlockKind::Paragraph(content) | BlockKind::Heading { content, .. } => {
                        Inline::plain_text(content)
                    }
                    other => format!("{other:?}"),
                })
                .collect::<Vec<_>>()
                .join(" / ")
        };
        (*start, items.iter().map(text).collect())
    }

    #[test]
    fn a_bullet_list_carries_no_start_and_an_ordered_one_does() {
        assert_eq!(
            items("<ul><li>a</li><li>b</li></ul>"),
            (None, vec!["a".to_owned(), "b".to_owned()])
        );
        // An `<ol>` with no `start` begins at one, which is what the block
        // means by `Some(1)` and what a markdown `1.` produces.
        assert_eq!(items("<ol><li>a</li></ol>").0, Some(1));
        assert_eq!(items("<ol start=\"7\"><li>a</li></ol>").0, Some(7));
        // A `start` that is not a number is the author saying nothing, not
        // the author saying zero.
        assert_eq!(items("<ol start=\"soon\"><li>a</li></ol>").0, Some(1));
    }

    #[test]
    fn an_item_left_unclosed_does_not_swallow_the_ones_after_it() {
        // `</li>` is the end tag a list author leaves out most, and `tree`
        // matches by name: in `<li>a<li>b`, the second item is a *child* of
        // the first. Rendering that literally gives one item with the rest
        // nested inside it, one indent deeper per item.
        assert_eq!(
            items("<ul><li>a<li>b<li>c</ul>").1,
            ["a".to_owned(), "b".to_owned(), "c".to_owned()]
        );
    }

    #[test]
    fn a_list_nested_in_an_item_stays_nested() {
        // The distinction the lift rule must not break: an `<li>` inside a
        // `<ul>` inside an item is a sublist, not a sibling.
        let (_, items) = items("<ul><li>a<ul><li>inner</li></ul></li><li>b</li></ul>");
        assert_eq!(items.len(), 2);
        assert!(items[0].contains("inner"), "{items:#?}");
        assert_eq!(items[1], "b");
    }

    #[test]
    fn an_item_holds_blocks_not_just_a_sentence() {
        // An item's children go back through `blocks_from`, so everything
        // that module can make reaches an item too.
        let (_, items) = items("<ul><li><h3>Title</h3><p>Body.</p></li></ul>");
        assert_eq!(items, ["Title / Body."]);
    }

    #[test]
    fn a_list_split_by_a_blank_line_is_gathered_back_together() {
        // A CommonMark HTML block ends at a blank line, and writing one
        // inside `<ul>` is how GitHub asks for markdown to render in an item,
        // so the items arrive at the root of a later block with no list
        // around them. Same shape as a table split the same way.
        assert_eq!(
            items("<li>a</li>\n<li>b</li>").1,
            ["a".to_owned(), "b".to_owned()]
        );
        // The tag that said which list it was is in an earlier block, so the
        // run is bulleted.
        assert_eq!(items("<li>a</li>").0, None);
    }

    #[test]
    fn content_loose_in_a_list_joins_the_item_above_it() {
        // A list may hold only items, and authors put a stray paragraph in
        // one anyway. Dropping the words is the one thing that must not
        // happen; the whitespace laying the source out is not content.
        let (_, joined) = items("<ul>\n  <li>a</li>\n  <p>stray</p>\n  <li>b</li>\n</ul>");
        assert_eq!(joined, ["a / stray", "b"]);
        // With nothing above it to join, it is an item of its own.
        assert_eq!(
            items("<ul><p>stray</p><li>a</li></ul>").1,
            ["stray".to_owned(), "a".to_owned()]
        );
    }

    #[test]
    fn a_list_with_nothing_in_it_produces_nothing() {
        // The `<ul>` half of a list split by a blank line: an opener whose
        // items are all in a later block. A bare frame would draw an empty
        // line the author never wrote.
        assert!(interpreted("<ul>\n</ul>").expect("interpreted").is_empty());
        assert!(interpreted("<ul>   </ul>").expect("interpreted").is_empty());
    }

    #[test]
    fn a_list_nested_past_the_cap_does_not_overflow_the_stack() {
        // `list_block` recurses through `blocks_from`, so the tree's depth
        // cap is what bounds it — the same guard `<div>` nesting leans on.
        let deep = MAX_NESTING + 50;
        let raw = format!("{}x{}", "<ul><li>".repeat(deep), "</li></ul>".repeat(deep));
        assert!(interpreted(&raw).is_some());
    }

    #[test]
    fn a_list_in_a_cell_keeps_one_item_per_line() {
        // The table renders — a list is no longer opaque — but a cell holds
        // inline content, so the markers cannot come with it. A line each is
        // what is left, and it beats sending the whole table to the page as
        // markup, which is what a cell holding a list used to do.
        let blocks = interpreted("<table><tr><td><ul><li>a</li><li>b</li></ul></td></tr></table>")
            .expect("interpreted");
        let [
            Block {
                kind: BlockKind::Table { rows, .. },
                ..
            },
        ] = blocks.as_slice()
        else {
            panic!("one table, got {blocks:#?}");
        };
        let [row] = rows.as_slice() else {
            panic!("one row, got {rows:#?}");
        };
        let [cell] = row.as_slice() else {
            panic!("one cell, got {row:#?}");
        };
        assert_eq!(
            cell.as_slice(),
            [
                Inline::Text("a".into()),
                Inline::HardBreak,
                Inline::Text("b".into()),
            ]
        );
    }

    #[test]
    fn a_caption_becomes_a_strong_paragraph_before_the_table() {
        let blocks = interpreted(
            "<table><caption>First</caption><caption>Second</caption>\
             <tr><td>a</td></tr></table>",
        )
        .expect("interpreted");
        let [caption, table] = blocks.as_slice() else {
            panic!("a caption and a table, got {blocks:#?}");
        };
        let BlockKind::Paragraph(content) = &caption.kind else {
            panic!("a paragraph, got {:?}", caption.kind);
        };
        // HTML uses the first caption and ignores the rest.
        assert!(
            matches!(content.as_slice(), [Inline::Strong(_)]),
            "{content:#?}"
        );
        assert_eq!(Inline::plain_text(content), "First");
        assert!(matches!(table.kind, BlockKind::Table { .. }));
    }

    #[test]
    fn a_caption_survives_a_table_with_no_cells_in_it() {
        // The table itself is nothing to draw, but the words the author wrote
        // are still words.
        let blocks =
            interpreted("<table><caption>Only this</caption></table>").expect("interpreted");
        let [block] = blocks.as_slice() else {
            panic!("just the caption, got {blocks:#?}");
        };
        assert_eq!(
            Inline::plain_text(match &block.kind {
                BlockKind::Paragraph(c) => c,
                other => panic!("a paragraph, got {other:?}"),
            }),
            "Only this"
        );
    }

    #[test]
    fn a_tfoot_is_rendered_last_wherever_it_was_written() {
        let (_, header, rows) = table(
            "<table><thead><tr><th>H</th></tr></thead>\
             <tfoot><tr><td>foot</td></tr></tfoot>\
             <tbody><tr><td>body</td></tr></tbody></table>",
        );
        assert_eq!(header, ["H"]);
        assert_eq!(rows, [["body"], ["foot"]]);
    }

    #[test]
    fn a_footer_row_of_th_is_never_mistaken_for_the_header() {
        // `<tfoot>` sorts last before the header is chosen, so the
        // leading-all-`th` rule cannot reach it — a totals row written with
        // `<th>` would otherwise become the labels.
        let (_, header, rows) = table(
            "<table><tfoot><tr><th>Total</th></tr></tfoot>\
             <tbody><tr><td>a</td></tr></tbody></table>",
        );
        assert!(header.is_empty(), "{header:#?}");
        assert_eq!(rows, [["a"], ["Total"]]);
    }

    #[test]
    fn rows_loose_at_the_root_are_gathered_into_one_table() {
        // A blank line ends a CommonMark HTML block, so a table written with
        // one inside it reaches us in pieces and the rows arrive with no
        // `<table>` around them.
        let (_, header, rows) =
            table("<thead><tr><th>H</th></tr></thead>\n<tr><td>a</td></tr>\n<tr><td>b</td></tr>");
        assert_eq!(header, ["H"]);
        assert_eq!(rows, [["a"], ["b"]]);
    }

    #[test]
    fn a_br_in_a_cell_is_a_line_break_and_not_a_lost_space() {
        let blocks =
            interpreted("<table><tr><td>one<br>two</td></tr></table>").expect("interpreted");
        let BlockKind::Table { rows, .. } = &blocks[0].kind else {
            panic!("a table, got {:?}", blocks[0].kind);
        };
        assert!(rows[0][0].contains(&Inline::HardBreak), "{:#?}", rows[0][0]);
        assert_eq!(Inline::plain_text(&rows[0][0]), "one two");
    }

    #[test]
    fn a_contributor_cell_of_a_badge_over_a_name_keeps_its_column() {
        // The all-contributors grid, which is the single most common HTML
        // table in a README. The image is decorative, so the cell's only
        // words are the name under it — a cell measured by its text alone
        // would look empty and lose the column.
        let (alignments, _, rows) = table(
            "<table><tr>\
             <td align=\"center\"><a href=\"https://github.com/one\">\
             <img src=\"1.png\" alt=\"\"/><br /><sub><b>One</b></sub></a></td>\
             <td align=\"center\"><a href=\"https://github.com/two\">\
             <img src=\"2.png\" alt=\"\"/><br /><sub><b>Two</b></sub></a></td>\
             </tr></table>",
        );
        assert_eq!(alignments, [Alignment::Center, Alignment::Center]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0][0].contains("One"), "{:?}", rows[0][0]);
        assert!(rows[0][1].contains("Two"), "{:?}", rows[0][1]);
    }

    #[test]
    fn a_wrapper_around_a_table_still_leaves_a_table_and_lends_its_alignment() {
        for raw in [
            "<center><table><tr><td>a</td></tr></table></center>",
            "<div align=\"center\"><table><tr><td>a</td></tr></table></div>",
        ] {
            let blocks = interpreted(raw).expect("interpreted");
            let [block] = blocks.as_slice() else {
                panic!("one table, got {blocks:#?}");
            };
            assert!(matches!(block.kind, BlockKind::Table { .. }), "{raw}");
            assert_eq!(block.align, Alignment::Center, "{raw}");
        }
    }

    #[test]
    fn a_table_inside_details_is_a_table_inside_the_quote() {
        let blocks = interpreted(
            "<details><summary>S</summary><table><tr><td>a</td></tr></table></details>",
        )
        .expect("interpreted");
        let BlockKind::BlockQuote { children, .. } = &blocks[0].kind else {
            panic!("a quote, got {:?}", blocks[0].kind);
        };
        assert!(
            matches!(children[1].kind, BlockKind::Table { .. }),
            "{children:#?}"
        );
    }

    #[test]
    fn a_span_attribute_follows_htmls_own_rules_for_reading_a_number() {
        let span = |value: &str| span_of(&[("colspan".into(), value.into())], "colspan");
        assert_eq!(span("3"), 3);
        assert_eq!(span(" +4"), 4, "leading space and a plus sign are allowed");
        assert_eq!(span("2x"), 2, "the digits that start it are the number");
        assert_eq!(span("0"), 1, "a span is at least one column");
        assert_eq!(span("auto"), 1);
        assert_eq!(span(""), 1);
        assert_eq!(span("-3"), 1, "not a non-negative integer");
        assert_eq!(span("99999"), MAX_SPAN, "clamped, not honoured");
        assert_eq!(span_of(&[], "colspan"), 1, "absent");
    }

    #[test]
    fn every_mode_round_trips_through_its_name() {
        for mode in HtmlMode::ALL {
            assert_eq!(mode.name().parse::<HtmlMode>(), Ok(mode));
        }
        assert!("nonsense".parse::<HtmlMode>().is_err());
    }
}
