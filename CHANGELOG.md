# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Until 1.0 the `render` module's public API may change within minor versions.

## [Unreleased]

Pre-release. The rendering engine and the non-interactive reader are complete;
the full-screen reader is not built yet.

### Added

#### Rendering engine (`src/render/`)

- Markdown → fixed-width styled line buffer, built on `pulldown-cmark` with
  GFM tables, footnotes, strikethrough, task lists, alerts, and smart
  punctuation enabled.
- Claude-artifact visual language: centered reading column with the page
  background painted edge to edge, headings styled by weight/color/rhythm
  instead of hash marks, rounded code containers with the language in the top
  border, accent-bar blockquotes, box-drawn tables, real list markers with
  hanging indent.
- GFM alert callouts (`[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`,
  `[!CAUTION]`) as icon-and-title callouts, each in its own hue.
- Syntax highlighting via `syntect` + `two-face`, mapped directly to ratatui
  styles with the surface background forced so a syntax theme cannot leak its
  own page color into a code card.
- Narrow-width fallback: tables become stacked `label: value` cards when a
  framed layout would shred prose into unreadable columns.
- `RenderedDoc` carries the outline, interned links with their exact column
  ranges, source byte ranges per line, and a plain-text mirror for search.

#### Sources and CLI

- `marquee-markdown [FILE|DIR|-]` renders to standard output.
- Source classification for all six argument forms (stdin, `-`, forge
  shorthands, URLs, directories, plain paths), kept pure behind an `FsProbe`
  trait.
- Directory arguments resolve a README case-insensitively in declared priority
  order.
- YAML frontmatter is stripped before rendering.
- Non-markdown files render as a syntax-highlighted fenced block with line
  numbers forced on, matching glow.
- Full glow flag surface parsed: `-a -l -m -n -p -s -t -w --config`, with
  `-p` + `-t` rejected as a hard error.
- `themes`, `man`, and `completion <shell>` subcommands.
- ANSI output with real OSC 8 hyperlinks; color and gutters degrade
  automatically when the destination is not a terminal.

#### Theming

- Two Claude palettes, `paper` (light) and `slate` (dark), compiled in as
  constants.
- Themes are also a data format: `Palette` deserializes from TOML, so a theme
  can be dropped into `~/.config/marquee-markdown/themes/` or passed to
  `--style` as a path, with no recompile. Built-ins and user themes are built
  through the same constructor.
- `--style auto` (default), plus `notty` for unstyled output.

### Fixed

Behaviors that differ from `glow`, verified against glow 3.0.0:

- Long lines inside a fenced block stay inside the container. In glow they wrap
  out of it.
- Lines containing links keep their width. Glow counts OSC 8 escape bytes as
  display columns, which leaves link-bearing lines ragged. Here escape
  sequences are structurally unrepresentable in the layout path, so no code
  path can count one as a column.
- Headings, blockquote bars, tables, and thematic breaks render as typography
  rather than as `#`, `|`, and `--------`.

### Known gaps

- The full-screen reader is not built: `-t`, `-p`, and bare invocation exit
  with a clear message rather than doing something unexpected.
- URL and forge (`github.com/owner/repo`) sources report that they are not
  implemented yet.
- `--config` is parsed but no configuration file is read yet.
- `-n` (`--preserve-new-lines`) is parsed but not yet honored.
- `-a` and `-m` only affect the browser and reader, which do not exist yet.
