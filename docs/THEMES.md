# Themes

A theme is a TOML file. Nothing about it is compiled, and the palettes that
ship with the reader are the same kind of file as one you write — they live in
[`themes/`](../themes), are embedded at build time, and go through the same
parser. A shipped theme is not a privileged code path, which is the only way
to be sure a contributed one works.

## Using one

```sh
mmd -s catppuccin-mocha README.md    # a shipped palette, by name
mmd -s ./mine.toml README.md         # a file, without installing it
```

Or press `s` in the reader for a picker that previews as you move and writes
your choice back to the config file.

Themes in `~/.config/marquee-markdown/themes/` are found by name too. A file
there **wins over a shipped palette of the same name**, so retuning one for
your terminal means copying it out of `themes/` and editing it — not forking
the project.

## What ships

| Name | Appearance | Upstream |
| --- | --- | --- |
| `paper` | light | the reader's own light palette |
| `slate` | dark | the reader's own dark palette, and the default |
| `catppuccin-latte` | light | [Catppuccin](https://github.com/catppuccin/catppuccin) |
| `catppuccin-mocha` | dark | [Catppuccin](https://github.com/catppuccin/catppuccin) |
| `dracula` | dark | [Dracula](https://github.com/dracula/dracula-theme) |
| `gruvbox-dark` | dark | [Gruvbox](https://github.com/morhetz/gruvbox) |
| `nord` | dark | [Nord](https://github.com/nordtheme/nord) |
| `solarized-dark` | dark | [Solarized](https://ethanschoonover.com/solarized) |
| `solarized-light` | light | [Solarized](https://ethanschoonover.com/solarized) |
| `tokyo-night` | dark | [Tokyo Night](https://github.com/folke/tokyonight.nvim) |

`system` is also always offered, and is not a file: it builds a palette from
what the terminal answers about its own colors.

## Writing one

```toml
name = "mine"             # must match the file stem
appearance = "dark"       # light | dark
syntax = "base16-eighties.dark"

[palette]
bg = "#262624"            # page, painted edge to edge
surface = "#1f1e1d"       # code cards, inline chips, table headers
fg = "#f5f4ef"
muted = "#87867f"         # gutters, line numbers, metadata
accent = "#d97757"        # headings, links, the cursor
accent_soft = "#d4a27f"   # secondary accents
border = "#3d3d3a"        # card and table rules

[palette.alerts]
note = "#7ea3c4"
tip = "#85b085"
important = "#b094c0"
warning = "#d9a441"
caution = "#dd7a6d"

[icons]                   # optional; the defaults draw in any monospace font
note = "ⓘ"
tip = "✦"
important = "‼"
warning = "⚠"
caution = "✖"
image = "▣"
```

Two things are worth knowing before you tune one.

**`surface` is recessed, not raised.** In both shipped palettes it is *darker*
than `bg`, in the light one as well as the dark one. Code cards read as set
into the page rather than floating above it. `solarized-dark` deliberately
breaks this — base02 is Solarized's own highlight background, and porting a
scheme faithfully beats matching a house convention.

**`syntax` names a syntect theme, and there are only seven.** They are
`InspiredGitHub`, `Solarized (dark)`, `Solarized (light)`,
`base16-eighties.dark`, `base16-mocha.dark`, `base16-ocean.dark`, and
`base16-ocean.light`. A name outside that list is caught by the tests for
shipped palettes, and falls back to unhighlighted text for one of yours.

Only Solarized has an exact counterpart there; every other shipped palette is
paired with the nearest of the seven by eye. **This pairing is the real work in
a new theme.** The code card is a large block of a palette that is not quite
yours, and getting it wrong is what makes an otherwise good port look cheap.
Budget for looking at a highlighted code block in the theme, not just at the
hex values.

## Contributing one

A theme PR needs no Rust.

1. Add `themes/<name>.toml`, with `name` matching the file stem.
2. Add a line to `ALL` in `src/theme/bundled.rs`, keeping it sorted.
3. Add a row to the table above and to the README's theme list.
4. `cargo test` — the bundled-theme tests check that it parses, that its name
   matches its file, that `ALL` stays sorted, and that its `syntax` is a theme
   the highlighter actually has.

Ports of established colorschemes are the easiest to review, because the hex
values are not a matter of opinion. If you are porting one, link the upstream
palette in a comment at the top of the file, as the shipped ones do.
