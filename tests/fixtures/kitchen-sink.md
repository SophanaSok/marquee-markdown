---
title: Kitchen Sink
author: fixture
---

# Kitchen Sink

Every construct the renderer has to handle, in one document. This paragraph is
deliberately long enough to wrap at any sensible reading width, so that the
wrapping pass has something to chew on and the centered column is visible.

## Inline styling

Plain, **bold**, *italic*, ***bold italic***, ~~struck through~~, `inline code`,
and a [link to a spec](https://commonmark.org/) plus a bare autolink
<https://spec.commonmark.org/>. A [relative link](./other.md) must resolve
against the source directory. Inline code with a very long token like
`some::extremely::long::path::that::will::not::fit::on::one::line::at::all` has
to wrap without shredding the chip background.

Unicode widths matter: 日本語のテキスト, emoji 🎨🚀, and combining marks é vs é.

## Headings

### Level three

#### Level four

##### Level five

###### Level six

## Lists

- First bullet
- Second bullet with enough text that it wraps and the continuation must line up
  under the text, not under the marker
  - Nested second level
    - Nested third level
- Fourth

1. Ordered one
2. Ordered two
10. Ordered ten, to check numeral alignment
    1. Nested ordered

- [ ] Unchecked task
- [x] Checked task

Term-style definition content:

Coffee
: Hot brown liquid

## Blockquotes

> A single-level quote that is long enough to wrap so the gutter bar repeats on
> every wrapped line rather than only the first.
>
> > A nested quote, which stacks bars.

> [!NOTE]
> Useful information a user should know.

> [!TIP]
> Helpful advice for doing things better.

> [!IMPORTANT]
> Key information users need to know.

> [!WARNING]
> Urgent info needing immediate attention.

> [!CAUTION]
> Advises about risks or negative outcomes.

## Code

Inline first: run `cargo clippy --all-targets -- -D warnings` before pushing.

```rust
/// A fenced block with a language, to exercise syntect.
fn main() {
    let greeting = "hello";
    println!("{greeting}, world");
    let this_line_is_extremely_long = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
    assert_eq!(this_line_is_extremely_long.len(), 18);
}
```

```
A fence with no language at all.
   Leading whitespace must survive.
```

```jsonc
{
  // an unknown-ish language, to test fallback
  "key": "value"
}
```

    An indented code block, four spaces.

## Tables

| Left | Center | Right | Notes |
|:-----|:------:|------:|-------|
| a | b | c | short |
| longer cell | centered | 42 | a much longer cell that will need to wrap inside its column |
| `code` | **bold** | *em* | [link](https://example.com) |

| Single |
|--------|
| column |

## Rules

Above the rule.

---

Below the rule.

***

Another style of rule.

## Images and HTML

![alt text](https://example.com/image.png "A title")

<div align="center">Raw HTML block</div>

Inline <b>HTML</b> too, and <em>emphasis</em>, <code>code</code>,
<del>struck through</del>, <kbd>Ctrl</kbd> and a <my-widget>custom element</my-widget>.

<h3 align="center">An HTML heading</h3>

<p align="center">
  Centered prose with a <a href="https://example.com">link</a>, an entity
  &mdash; a numeric one &#8212; and a bare a &lt; b comparison,<br>
  broken across lines.
</p>

<p align="center">
  <a href="https://example.com/ci"><img alt="CI" src="https://img.example/ci.svg"></a>
  <img alt="" src="https://img.example/decorative.svg">
</p>

<p><sub>A caption, in a size a terminal does not have.</sub></p>

<!-- A comment, which renders as nothing at all. -->

<div align="center">

An unclosed container: a blank line ends the HTML block, so the closing tag
arrives in a block of its own.

</div>

<details>
<summary>Unrecognized, so this whole block stays literal markup</summary>
<table><tr><td>including this</td></tr></table>
</details>

## Footnotes

A statement needing a citation.[^1]

[^1]: The footnote body.

## Hard breaks

Line one with two trailing spaces  
line two after a hard break.

Line one with a backslash\
line two.
