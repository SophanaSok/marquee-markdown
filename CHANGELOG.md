# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The library API is in two halves. **Stable**, and covered by semver from 1.0:
`render::{render, Document, RenderedDoc, LineMeta, LineKind, Anchor,
LayoutOptions}`, `render::{ansi, tui, overlay, measure}`, and all of `theme`.
**Internal**, marked `#[doc(hidden)]` and free to change in any release: the
pipeline — `parse`, `block`, `frag`, `wrap`, `sink`, `layout`, `highlight`.
Until 1.0 both halves may change.

## [Unreleased]

Nothing yet.

## [0.1.0] - 2026-08-19

The first release. Feature-complete against `glow`, plus the contents pane,
search, and a configuration file with rebindable keys.

### Added

#### Rendering engine (`src/render/`)

- `Document`: markdown parsed once and laid out at any number of widths, which
  is what a reader that resizes needs. Opaque on purpose — it is the part of
  the pipeline a consumer needs, without freezing the shape of what is behind
  it.

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

#### File browser

- `marquee-markdown` with no argument, or with a directory, lists the markdown
  under it — most recently edited first, with the age of each file.
- The walk streams from a background thread, so the first screenful appears
  immediately and a large tree fills in behind it rather than blocking the
  first frame.
- Hidden and git-ignored files are left out unless `-a` asks for them. Ignore
  files are honored outside a git repository too.
- Fuzzy filtering on `/`, applied as you type. Query and file names are both
  normalized first, so a name written with combining marks still matches one
  typed with precomposed characters.
- `enter` reads a file and `esc` goes back to the list, so the browser is where
  a reading session lives rather than somewhere passed through once. A file
  named on the command line has no browser behind it, and `esc` there still
  hints rather than opening one.
- glow's paging inconsistency is reproduced deliberately: `f`/`d` move a whole
  screen in the browser and half a screen in the pager, and `h`/`l` page in the
  browser but scroll sideways in a document. Anyone with the muscle memory
  would find a silent correction more surprising than the quirk.

#### Remote sources

- `marquee-markdown github.com/owner/repo`, `github://owner/repo`,
  `gitlab://owner/repo`, and any `http(s)://` URL.
- Repository shorthands resolve through the forge's API to the *raw* README,
  rather than fetching the page around it. GitLab's `readme_url` points at the
  page showing the file, so it is rewritten to the raw path; GitHub answers 403
  without a `User-Agent`, so one is always sent.
- The extension in a URL is trusted ahead of the `Content-Type` header, since
  servers routinely hand out markdown as `text/plain` or `text/html`. A URL
  with no extension served as HTML is shown as highlighted markup rather than
  run through the markdown renderer.
- Relative links resolve against where the body actually came from, after
  redirects, rather than against what was asked for.
- Bodies are capped at 8 MiB and requests time out after 20 seconds.
- Fetching is behind a `Fetcher` trait with a `FakeFetcher` beside it, so every
  remote path is unit-tested with no network. Live checks against the real
  forges exist as `#[ignore]`d tests in `tests/network.rs`.

#### Live reload, links, and the rest of glow's keys

- The open document is watched and re-renders when it changes on disk. The
  reader keeps the *section* they were in rather than the line number, because
  an edit above them moves every line below it. `r` reloads by hand.
- The watch is on the containing directory, not the file: most editors save by
  writing a temporary file and renaming it over the original, and a watch on
  the file itself survives exactly one save.
- `]` and `[` step through the document's links, `enter` opens the selected
  one, and `y` copies its address. Relative links resolve against wherever the
  document came from.
- `c` copies the markdown as written rather than as rendered — the source is
  what a reader wants to paste elsewhere.
- Copying goes through the terminal (OSC 52) before the system clipboard, so it
  works over SSH: the text lands on the machine the reader is at rather than on
  the server. In tmux this needs `set -g set-clipboard on`.
- `e` opens the document in `$VISUAL`, `$EDITOR`, or `vi`, at the line on
  screen, and reloads when the editor exits. Editor settings carrying arguments
  (`EDITOR="emacsclient -nw"`) are split properly, and line arguments are
  spelled the way each editor wants them; an unrecognized editor gets the path
  alone rather than a flag it would take for a second filename.
- `ctrl+z` suspends to the shell, on unix. The action does not exist on other
  platforms rather than existing and doing nothing, so the key reference stays
  truthful.
- `-p` renders through `$PAGER`, defaulting to `less -R` — without `-R` it
  would print the escape sequences as text.
- Bursts of events are coalesced: dragging a window edge costs one re-layout
  and one frame per batch rather than per event.

#### Configuration

- `~/.config/marquee-markdown/config.toml`, with `--config` and
  `MARQUEE_CONFIG` naming a different file.
- Precedence is defined in exactly one function: command line, then
  environment, then file, then defaults. A switch that was not given
  contributes nothing rather than `false`, so a setting turned on in a file is
  not undone by every invocation that omits the flag.
- `[keys.<mode>]` rebinds any key in any mode, and an action of `none` takes a
  key away.
- Unknown settings, keys, and actions are reported and ignored rather than
  refused: a file written for a newer version has to keep working with an older
  binary, and one typo should cost one key rather than the whole keymap. A file
  that is not valid TOML is still an error, because that is a mistake rather
  than version skew.
- `marquee-markdown config` prints the settings in force as a file that would
  produce them — the only practical way to answer "why is this setting what it
  is?" — and the output round-trips.
- `marquee-markdown keys` prints every binding as markdown;
  `docs/KEYBINDINGS.md` is generated from it and a test fails if it drifts.
- `-n` (`--preserve-new-lines`) is now honored, closing the last flag that was
  parsed but ignored. A document written one sentence per line keeps its shape
  instead of being re-flowed.

#### Theming

- Two Claude palettes, `paper` (light) and `slate` (dark), compiled in as
  constants.
- Themes are also a data format: `Palette` deserializes from TOML, so a theme
  can be dropped into `~/.config/marquee-markdown/themes/` or passed to
  `--style` as a path, with no recompile. Built-ins and user themes are built
  through the same constructor.
- `--style auto` (default), plus `notty` for unstyled output.

#### Two names

- `mmd` is installed alongside `marquee-markdown` and is the same program.
  Both binaries are stubs over one implementation, so they cannot drift.
- The generated man page and completions are named after whichever name was
  invoked: completions registered for a name the reader does not type would
  simply never fire.

#### Release engineering

- CI across Linux, macOS and Windows: format, clippy with warnings denied, the
  test suite, `cargo doc` with warnings denied, a build at the minimum
  supported Rust version read from the manifest, `cargo deny`, and a check that
  the packaged crate's own tests compile.
- A smoke-test job that runs the binary, because several bugs here were
  invisible to the whole suite and obvious on first use — piped stdin, closed
  pipes, `-n`, and every generated surface.
- A release workflow producing archives for four targets, Debian and RPM
  packages, checksums, and release notes taken from this file. Man pages and
  completions are generated by the binary into every archive rather than
  checked in, so they cannot drift from the flags it accepts.
- `docs/ARCHITECTURE.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`,
  `SECURITY.md`, and issue templates.
- Homebrew and Scoop packaging templates.

### Fixed

- The minimum supported Rust version was wrong: the manifest claimed 1.85 while
  the code uses let-chains, which need 1.88. Anyone on 1.85 to 1.87 would have
  hit a compile error rather than a clear message. CI now checks it, reading
  the version from the manifest so the promise and the test cannot drift.
- `marquee-markdown completion <shell> | head` aborted with a panic instead of
  stopping quietly: `clap_complete` panics internally on a write error, and
  `print!` panics on a closed pipe. Every subcommand now writes through one
  fallible path, so closing a pipe means what it is supposed to mean.
- `docs/` was excluded from the published crate while a test read a file from
  it, so `cargo test` on the published package would not have compiled.
- Two advisories against `quick-xml`, reached through `plist` through
  `syntect`. Neither is reachable from this program — syntax and theme data
  come from binary dumps, not XML — but rather than argue that, `syntect` is
  now built with exactly the features used, which takes `plist` and
  `quick-xml` out of the dependency tree altogether.
- **A named source is no longer ignored in favour of standard input.** With
  input redirected — a cron job, a CI step, an editor shelling out —
  `marquee-markdown notes.md` rendered whatever was on stdin instead of
  `notes.md`, or nothing at all when stdin was empty, with nothing on screen to
  say so. Standard input is now read when it is the only thing on offer, or
  when `-` asks for it. This is a deliberate divergence from glow, whose rule
  is fine when a person types the pipe and quietly wrong everywhere else.
- The file browser's paths are spelled with the platform's own separator; the
  tests asserting them assumed `/` and failed on Windows.

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

- Callout and image icons are Nerd Font glyphs; a terminal without one shows a
  missing-glyph box. They should be part of the theme format.
- On macOS, file system events are reported per directory, so saving a
  different file beside the open one can trigger a redundant reload. The
  document is re-read rather than shown wrongly.

- A fetch failure at startup exits with a message rather than opening the
  reader with an explanation in the status bar.
- Opening a link hands off to the system handler; a link that is relative to a
  fetched document is resolved by joining, without normalizing `..`.
- `ctrl+z` needs a shell with job control, as any suspend does.
- A fetched document cannot be reloaded or opened in an editor, because it has
  no path on this machine.
- The browser scans once at startup: files created while it is open do not
  appear until it is restarted.
- `-a` is read at startup and cannot be toggled from inside the browser.
- Search matches the rendered text, so a phrase broken across a soft wrap does
  not match. Searching is deliberately not incremental: the query runs when
  `enter` is pressed.
- The key reference does not scroll, so on a terminal shorter than about 22
  rows the last few bindings are cut off.
- Resizing re-lays out on every event; a large document dragged by a window
  edge will work harder than it needs to until a debounce lands.

[Unreleased]: https://github.com/SophanaSok/marquee-markdown/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/SophanaSok/marquee-markdown/releases/tag/v0.1.0
