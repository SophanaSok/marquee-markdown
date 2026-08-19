# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Until 1.0 the `render` module's public API may change within minor versions.

## [Unreleased]

Pre-release. The rendering engine and the full-screen reader — contents pane
and search included — are complete; the file browser and remote sources are not
built yet.

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

#### Full-screen reader (`-t`)

- Alternate-screen reader with a scroll-tracking status bar showing the
  document, the section being read, and progress.
- Every glow pager key, kept pointing at what it points at in glow: `j`/`k`,
  `d`/`u`, `f`/`b`, `g`/`G`, `space`, the arrow and page keys, `q`.
- `T` switches between the light and dark palette; `?` opens a key reference;
  `esc` closes what is open rather than quitting.
- Horizontal scrolling with `h`/`l`, which is what makes `-w 0` usable.
- Resizing or switching theme re-lays out and keeps the reading position,
  carried by source byte offset rather than by line number.
- Mouse wheel scrolling under `-m`.
- `-t` into a pipe or a file renders once instead of writing cursor movements
  into it.

#### Contents pane and search

- A scroll-tracking table-of-contents pane beside the document, with folding
  (`h`/`l`), movement on the same keys the document uses, and `enter` to go to
  an entry.
- Nesting comes from the order headings appear in, so a document that jumps
  from `#` to `###` nests sensibly instead of producing an orphan.
- The pane distinguishes the **active** entry, which follows the scroll
  position, from the **cursor**, which is where the reader left it. Scrolling
  never drags the cursor away and moving the cursor never pretends the reader
  scrolled.
- The pane hides itself on a terminal under 60 columns and on a document with
  fewer than two headings, where it would cost a quarter of the screen to say
  nothing.
- In-document search on `/`, with `n` and `N` to step through hits and a count
  in the status bar. Lowercase queries ignore case; a capital makes the query
  case-sensitive.
- Hits are highlighted through a draw-time overlay, so searching never re-lays
  out the document, and a background-only highlight keeps the syntax colors
  underneath it.
- Hits are re-found whenever the document is laid out again, so a resize or a
  theme switch cannot leave the highlight pointing at a stale line.
- A prompt captures every printable key: typing `q` into a search box types a
  `q` rather than quitting the reader.
- `esc` unwinds one layer at a time — overlay, then prompt, then focus, then
  the search highlight — and hints rather than quitting at the bottom.

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

- `-p` and bare invocation (the file browser) exit with a clear message rather
  than doing something unexpected.
- Search matches the rendered text, so a phrase broken across a soft wrap does
  not match. Searching is deliberately not incremental: the query runs when
  `enter` is pressed.
- The key reference does not scroll, so on a terminal shorter than about 22
  rows the last few bindings are cut off.
- URL and forge (`github.com/owner/repo`) sources report that they are not
  implemented yet.
- `--config` is parsed but no configuration file is read yet.
- `-n` (`--preserve-new-lines`) is parsed but not yet honored.
- `-a` affects the browser, which does not exist yet.
- Resizing re-lays out on every event; a large document dragged by a window
  edge will work harder than it needs to until a debounce lands.
