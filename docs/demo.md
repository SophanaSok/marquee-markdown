# Marquee

A terminal markdown reader that renders documents the way Claude artifacts do.

> [!NOTE]
> Callouts get an icon, a title, and a hue of their own.

## Rendering

Code blocks are sealed containers — a long line wraps *inside* the card
rather than escaping it:

```rust
fn width(text: &str) -> usize {
    text.graphemes(true).map(grapheme_width).sum()
}
```

## Tables

| Construct | Rendered as |
| --- | --- |
| Heading | weight, colour, rhythm |
| Quote | an accent gutter bar |
| Rule | a hairline across the column |

## Navigation

- The contents pane tracks where you are reading
- `/` searches, `n` and `N` step through the hits
- `]` and `[` step through links
