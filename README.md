# marquee-markdown

A terminal markdown reader with the functionality of
[`glow`](https://github.com/charmbracelet/glow), rendering documents the way
Claude artifacts do — a centered reading column on a painted page, typographic
headings, sealed code cards — with a table-of-contents panel for navigation.

> **Status: pre-release.** The rendering engine and the non-interactive reader
> work today. The full-screen reader and its table-of-contents sidebar are not
> built yet; see [docs/ROADMAP.md](docs/ROADMAP.md).

## What it looks different from

Rendering the same document through glow 3.0.0 shows what this fixes:

| glow | marquee-markdown |
| --- | --- |
| `## Heading` — hash marks reach the output | typography: weight, color, rhythm, a hairline rule |
| Blockquotes prefixed with ASCII `\|` | an accent `▎` gutter bar |
| `[!NOTE]` printed literally | an icon-and-title callout in its own hue |
| Tables with no border, bare `---\|---` | box drawing with a shaded header band |
| Thematic breaks as `--------` | a hairline `─` across the column |
| Long code lines wrap *out* of the block | they stay sealed inside the card |
| OSC 8 escapes counted as width, leaving ragged lines | escapes cannot reach width math |
| 2-space margin | a centered reading column, page painted edge to edge |
| No outline, no in-document search | both (once the reader lands) |

## Install

Requires Rust 1.85 or newer. No C toolchain is needed — syntax highlighting
uses a pure-Rust regex backend, so the project builds anywhere Rust does.

```sh
cargo install --path .
```

## Usage

```sh
marquee-markdown README.md          # render a file
marquee-markdown docs/              # render a directory's README
marquee-markdown -                  # read standard input
cat notes.md | marquee-markdown     # same
marquee-markdown src/main.rs        # source files render as highlighted code

marquee-markdown -w 80 doc.md       # fixed width (0 disables wrapping)
marquee-markdown -s paper doc.md    # light theme
marquee-markdown -l doc.md          # line numbers

marquee-markdown themes             # list available themes
marquee-markdown man                # man page to stdout
marquee-markdown completion fish    # shell completions
```

Output degrades on its own: piping or redirecting drops color, gutters, and
hyperlinks, so `marquee-markdown doc.md > out.txt` contains just the text.
`NO_COLOR` is honored.

## Themes

Two palettes ship compiled in — `paper` (light) and `slate` (dark) — and
`--style auto` is the default. A theme is also just a file, so adding one needs
no Rust and no recompile:

```sh
mkdir -p ~/.config/marquee-markdown/themes
$EDITOR ~/.config/marquee-markdown/themes/mine.toml
marquee-markdown -s mine doc.md
# or, without installing it:
marquee-markdown -s ./mine.toml doc.md
```

```toml
name = "mine"
appearance = "dark"       # light | dark
syntax = "base16-eighties.dark"

[palette]
bg = "#262624"            # page, painted edge to edge
surface = "#1f1e1d"       # code cards, inline chips, table headers
fg = "#f5f4ef"
muted = "#87867f"
accent = "#d97757"
accent_soft = "#d4a27f"
border = "#3d3d3a"

[palette.alerts]
note = "#7ea3c4"
tip = "#85b085"
important = "#b094c0"
warning = "#d9a441"
caution = "#dd7a6d"
```

`marquee-markdown themes` lists what is available and where each came from.

## Using the renderer as a library

`src/render/` is a standalone engine with no dependency on the application
shell — a test enforces that — so it can be used to build other frontends.

```rust
use marquee_markdown::render::{self, LayoutOptions};
use marquee_markdown::theme::{Theme, ThemeVariant};

let doc = render::render(
    "# Title\n\nSome prose.",
    &Theme::new(ThemeVariant::Slate),
    LayoutOptions { width: 80, code_line_numbers: false },
);
assert_eq!(doc.outline[0].text, "Title");
assert!(doc.lines.iter().all(|l| l.width() == 80));
```

## Development

See [AGENTS.md](AGENTS.md) for commands, architecture, and the two invariants
the design rests on, and [docs/ROADMAP.md](docs/ROADMAP.md) for what is built
and what comes next.

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all

# render a file without installing, to look at the output
cargo run --example preview -- tests/fixtures/kitchen-sink.md 80 slate
```

## License

MIT. See [LICENSE](LICENSE).
