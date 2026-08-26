# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The library API is in two halves. **Stable**, and covered by semver from 1.0:
`render::{render, render_with, Document, RenderedDoc, LineMeta, LineKind,
Anchor, LayoutOptions, ParseOptions, HtmlMode}`, `render::{ansi, tui, overlay,
measure}`, and all of `theme`. **Internal**, marked `#[doc(hidden)]` and free
to change in any release: the pipeline — `parse`, `block`, `frag`, `wrap`,
`sink`, `layout`, `highlight`, `html`.
Until 1.0 both halves may change.

## [Unreleased]

### Changed

- **Resizing a document full of code is no longer dominated by re-highlighting
  it.** Syntax highlighting depends on the text, the language and the theme,
  and a resize changes none of them — but it was being done from inside the
  layout emitter, so every width change re-ran syntect over every fenced block
  in the document and threw the result away.

  It was not a small part of the bill. A document of 120 `rust` fences laid
  out in 199 ms, against 6 ms for the same text with the language taken off:
  about 97% of a re-layout was highlighting, paid again for every step of a
  drag. Six hundred fences took a full second, each time.

  Highlighting is now kept for as long as the parse it belongs to, which is
  exactly how long it stays valid:

  | document | before | after |
  | --- | --- | --- |
  | this README | 8.6 ms | 2.0 ms |
  | 120 `rust` fences | 198.8 ms | 8.9 ms |
  | 600 `rust` fences | 999.3 ms | 49.8 ms |
  | 600 fences, no language | 32.4 ms | 32.0 ms |

  The last row is the control: nothing to highlight, so nothing to save.

  Switching themes does pay again, once, because the theme is one of the three
  things highlighting depends on — the syntax theme it names, the surface
  colour forced onto every span, and the fill used where there is no language.
  A one-shot render keeps nothing, since it lays the document out once and a
  memo could only cost it memory; its peak stays where it was. A reader
  holding a 300 KB code-heavy document pays about 10 MB for this, and about
  0.1 MB for a document the size of this README.

### Fixed

- **A document can no longer take the terminal down with it.** Nesting deeply
  enough — about 3,000 levels of `> - `, or 8,000 nested `<div>`s — overflowed
  the stack while the document was being laid out: layout walks the block tree
  by recursion, so the depth of the call chain was the depth of the document,
  and the document chose it.

  That is the worst way this program can fail. A stack overflow *aborts*, and
  an abort does not unwind, so neither the RAII terminal guard nor the panic
  hook that exists for exactly this ever ran. The reader died with the
  alternate screen still up, the cursor still hidden and mouse reporting still
  on — a terminal that needed `reset` before it could be used again. Nor did
  the document have to be your own: `https://` and `github://` are ordinary
  sources.

  Nesting is now capped at 256 levels, in the markdown tree and the HTML one
  alike. Past the cap a container is not represented and its children are
  spliced into its parent, so the text still renders — at the capped indent
  rather than a deeper one — instead of being truncated or refused. The cap is
  far above anything a terminal can show: each level of quote or list costs two
  cells of lead, so 80 columns is full of decoration by about level 40, and
  from there the content is already pinned to the one cell the lead leaves it.

- **A resize that stops no longer leaves the last frame mangled.** Pane
  geometry is recomputed before every draw, which is why the resize event
  itself does nothing — but the recompute asked `Terminal::get_frame` for the
  area, and ratatui only updates that inside `draw`. So it was always reading
  the size of the *previous* frame: the resize event woke the loop, the loop
  laid the document out for the width the terminal used to be, and drew it
  into the width it now is. Ratatui clips widgets to the buffer, so nothing
  overflowed — it just came out wrong, with the contents pane still divided
  where it had been and headings cut off mid-word.

  Dragging a window edge hid this, because each new event redrew from the
  freshly-learned size. A resize that *stops* — the ordinary case — had
  nothing following it, so the mangled frame stayed up until a key was
  pressed. The terminal is now asked its size before the geometry is decided,
  the same call the loop already makes after handing the screen to an editor,
  and for the same reason.

## [0.6.0] - 2026-08-26

`--style system`: a palette built from the colors your terminal is already
using. Nothing resolves differently than it did — the only change to `auto`,
the default, is that its documentation now describes it accurately.

### Added

- **`--style system`** builds the whole palette out of the terminal's own
  colors rather than shipping one. The page and the text are taken verbatim
  from `OSC 10` and `OSC 11`; cards, borders and the muted tone step off them;
  headings, links and the five callout hues come from the `OSC 4` ANSI slots.
  Every color that ends up as text is held to a WCAG contrast floor against
  the page and walked toward the foreground until it clears — which is what
  keeps a light scheme's yellow from becoming an unreadable heading, and what
  makes this survive a colorscheme that reports every slot as black.

  It is listed by `themes`, selectable in the `s` picker, and saved to the
  configuration file like any other. Anything that will not answer falls back
  to a shipped palette rather than refusing to start: `screen`, which swallows
  the question, and **tmux**, which answers the device query and nothing else.
  Only a terminal that answers nothing at all pays the 100 ms timeout; where
  the device query comes back — tmux included — the fallback is immediate.

- **`terminal-query`**, in `[general]`, and `MARQUEE_TERMINAL_QUERY`. Default
  on; off stops the terminal being asked anything, for a terminal that prints
  the question instead of answering it.

### Changed

- **`--style auto` is now documented as what it is: an alias for `slate`.** It
  has carried a promise to follow the terminal's background since the
  beginning and has never kept it — the seam was there from the start and both
  callers passed "unknown", so it has always answered the dark palette. The
  behavior is untouched, deliberately: it was the default, and the first
  release to start keeping that promise would move every reader on a light
  terminal from slate to paper without being asked. The documentation now
  matches the code instead of the other way round, and `--style system` is
  where following the terminal lives.

- **The default style is now `slate` rather than `auto`**, which is the same
  theme by another name — `mmd config` says `style = "slate"` where it used to
  say `style = "auto"`. Naming the palette states the default instead of
  deferring it to whatever `auto` happens to mean, so changing what a reader
  who never chose a theme sees now takes changing the default, deliberately,
  rather than falling out of a change to `auto`. `auto` remains a name
  `--style` and `[general] style` accept.

  Nothing resolves differently. `-s auto` is byte-for-byte `-s slate`, with a
  terminal sitting there answering or without one.

- **The reader takes the mouse wheel by default**, where it used to wait to be
  asked with `-m`. Not a preference: a terminal whose alternate scroll mode is
  on — which is the default in most of them — answers a wheel that nobody
  claimed by manufacturing arrow keys, multiplied by whatever scroll factor it
  was configured with. They arrive as ordinary keystrokes, indistinguishable
  from a hand on `j`, so a stray touchpad brush would yank the document away
  from someone reading it with the keyboard, and only while the pointer
  happened to be over the window. Claiming the wheel is what stops the terminal
  doing that, and it makes a tick the same three lines everywhere instead of a
  number the terminal picked.

  The cost is that selecting text with the mouse needs `shift` held while the
  reader is open, which is the trade `less --mouse` makes. **`--no-mouse`** and
  `mouse = false` hand the wheel back; `-m` is still accepted, still means the
  same thing, and still overrides a configuration file that turned it off.

- **The wheel moves the pane the movement keys would move**, rather than the
  document whatever has focus. Tab into the contents pane and the wheel moves
  the outline; open the key reference and it moves the reference; in the file
  browser it moves the list. It resolves to the same `Action` a key does, so
  the two cannot drift apart, and a pane that grows movement keys gets the
  wheel with them. A sideways tick still only pans a document — `h` and `l`
  fold the outline, which is not something to do to somebody by accident.

### Fixed

- **Pointer movement redrew the reader, for nothing.** Asking a terminal to
  report the mouse asked it, through crossterm, for any-event tracking as well
  as the wheel — a report per cell the pointer crossed, each one a wakeup, a
  re-layout and a whole frame drawn and diffed away, for as long as a hand
  rested on the mouse. Nothing here has ever read a mouse column. The reader
  now asks for the wheel and nothing else, and drops the rest on arrival in
  case it was asked anyway.

- **Mouse tracking left on by another program is now turned off on the way
  in.** An editor opened with `e` and then killed leaves it on for good, and
  the reader it comes back to had no way to know: it would report every pointer
  movement into a program with no use for a mouse at all.

- **A README key table ran past its own end on Windows.** The doc-drift scan
  split on a blank line, which a CRLF checkout does not contain, so every
  table continued to the end of the file and fed the chord parser whatever
  backticks it met. Nothing after the key tables had a backticked cell in its
  first column until now, which is why it had never shown.

### Notes

The question and its answer travel the same stream as keystrokes, and reading
is destructive. So the exchange happens once, before the screen is taken and
before the thread that owns standard input exists; on `/dev/tty` rather than
either standard stream; declined outright if anything is already queued; and
ended by a device-attributes sentinel rather than by the clock. Only `system`
asks at all.

Windows falls back for now: the replies arrive there through the console input
API rather than as bytes on a device, which is a different mechanism rather
than a variation on this one.

`registry::resolve` takes what the terminal answered in place of its old
`Option<bool>`, and `registry::Origin` has gained a variant and become
`#[non_exhaustive]`. Both are in the stable half of the API, which is what
makes this 0.6.0 rather than 0.5.2.

## [0.5.1] - 2026-08-21

Packaging and test-suite fixes. Nothing a reader will notice: of the three
changes here, two are to files excluded from the published crate, and the
third is internal.

### Fixed

- **The Homebrew formula had no `url` and no `sha256`.** Not stale — absent,
  since it was written, through four releases. `brew install` had nothing but
  `--HEAD` to fetch, and the `version` its own test block asserts against did
  not exist. It now points at the tag's source tarball, which is what the
  formula builds from; the `checksums.txt` on a release describes the prebuilt
  archives it never downloads.
- **The configuration tests read whoever was running them.**
  `Config::load(.., None, ..)` stubbed the environment but not the filesystem,
  so `locate` reached for the real default path and "nothing configured" meant
  "nothing configured, unless this machine has a file". Three tests asserting
  the defaults were asserting the machine had no configuration. They passed for
  as long as they did because hardly anyone had one — and then the theme picker
  started writing it, so trying the headline feature of 0.5.0 turned the suite
  red for reasons unconnected to anything the contributor touched. `locate` is
  handed the location now, which is what the module header always claimed.

### Changed

- `tests/docs.rs` checks the Homebrew formula, and the smoke job checks that a
  configuration file in the usual place is read — the one line no unit test can
  see, since it reaches through `dirs`. Both were confirmed by breaking them.

## [0.5.0] - 2026-08-21

### Added

- **A theme picker in the reader.** `s` opens a list of every theme the
  registry can find — the two built-ins and anything in
  `~/.config/marquee-markdown/themes/` — and the document behind it redraws as
  the cursor moves, so a theme is chosen by looking at your own text rather
  than at a swatch. `enter` keeps what is on screen and `esc` puts back the
  theme you opened with. Themes were already data; until now there was no way
  to reach one from inside the reader, so a theme you had written could only be
  selected by relaunching with `-s` or editing the configuration file.

  A theme that will not load says so on its row and leaves the previous one up,
  rather than the picker refusing to open because of one bad file.
- **Accepting a theme records it**, so the next run starts there. Only the
  `style` line is rewritten: comments, key order, and every other setting in
  the file survive, which is what `toml_edit` was added for. When `-s` or
  `MARQUEE_STYLE` is in force the save says so, because either would beat the
  file on the next run and a setting that silently does nothing is worse than
  one that says why.
- `Mode::Themes` and the `theme-picker`, `theme-down`, `theme-up`,
  `theme-top`, `theme-bottom` and `theme-accept` actions, all rebindable
  through `[keys.themes]` like any other.
- `Theme::overlay_meta`, for secondary text on an overlay panel. Composed from
  the existing palette, so no theme file needs changing.

### Changed

- `app::Options` is no longer `Copy`: it now carries the path of the
  configuration file the picker writes to. It stays `Clone` and
  `#[non_exhaustive]`, and is built from `Default` outside this crate.
- `T` is unchanged. It still flips light and dark, which is the faster gesture
  for anyone who only uses two themes. Accepting a theme from the picker now
  re-points what `T` swaps to when it would otherwise have become the theme
  just chosen — picking the palette `T` was already going to reach used to
  leave both sides of the swap identical and `T` doing nothing.

## [0.4.0] - 2026-08-20

### Added

- **Raw HTML is interpreted rather than printed.** A README written the way
  GitHub READMEs are — a centered `<h1>`, a `<p align="center">` of badge
  images, `<br>` inside a tagline, `<sub>` captions — now reads as what it
  means instead of showing its tags. The title becomes a real heading, so it
  joins the contents pane and the outline; `<a href>` becomes a link; an
  image contributes its alt text, or nothing at all when it has none, which
  is what keeps a badge row from rendering as a row of blanks. `align` is
  honoured on headings and paragraphs.

  An element with no emitter behind it — `<table>`, `<details>`, `<ul>`,
  `<script>` — sends its whole block back to being shown as literal markup,
  because a table read as one run-on sentence is worse than a table read as
  tags. An element that is merely unrecognized keeps its words and loses its
  tag: the promise in this mode is that no markup reaches the page.
- `[render] html` in the configuration, and `MARQUEE_RENDER_HTML`, choosing
  between `render` (the default), `hide`, and `literal`. There is no flag:
  this program keeps glow's flag surface and glow has no equivalent.
- `render::{render_with, ParseOptions, HtmlMode}` and `Document::parse_with`,
  all additive. Parse options are separate from `LayoutOptions` because they
  change the block tree rather than its presentation — an HTML heading has to
  reach `Document::heading_count`, which is answered before anything is laid
  out, and which is what decides whether the contents pane exists.

### Fixed

- **A raw HTML block no longer gets a blank line between every source line.**
  pulldown-cmark delivers one event per *line* of an HTML block, and with
  nowhere to collect them each line became its own block with the layout's
  inter-block spacing between. This was visible in `literal` mode too, and is
  fixed there as well.
- **A list item no longer loses content that starts with formatting.**
  `- **Bold.** Rest.` rendered as `• Rest.` — the lead-in was gone, and seven
  bullets in this project's own README were missing theirs. A tight list item
  arrives with no wrapping paragraph, so the parser opens a synthetic one on
  the first inline event; the trigger listed the text-shaped events but not
  the emphasis and link *starts*, so `**` opened a frame with no root beneath
  it and the finished `Strong` was pushed into an empty stack and discarded.
  Emphasis, links, images, inline code and inline HTML in the lead position
  are all fixed, in list items, task items, and nested containers alike.
  `push_inline` now asserts it has somewhere to put content, so the next
  version of this loses loudly instead of silently.
- **A link around an unlabelled image no longer disappears.** `<a
  href="page"><img src="badge"></a>` with no `alt` rendered as nothing at all,
  taking the link off the page without saying so, where markdown's
  `[![](img)](page)` has always kept its placeholder. An `alt=""` written out
  is still honoured as the author declaring the image decorative; an `alt`
  that is merely absent now behaves the way markdown does.
- **An overlong word inside an inline-code chip no longer overflows the
  column by one cell.** The chip's opening pad is glued to its content, so
  breaking before the content carried the pad onto the new line — and pad
  plus content could exceed the column even though neither did alone. The
  width invariant is what the painted page rests on, and `LineSink` only
  asserts it in debug builds, so in a release this tore the page silently.
  Found by a property test, which now generates inline code.

### Changed

- `app::Options` and `oneshot::Settings` gained a field, and are now
  `#[non_exhaustive]` — breaking for anything that built one with an
  exhaustive struct literal, which is why this is a minor bump. It is the
  last time, for the same reason the configuration structs were closed in
  0.3.0: outside this crate they are built from `Default` and assigned to.
  `DocCache::new` and `Settings::detect` keep their signatures; the new
  settings arrive through `DocCache::with_options` and `Settings::with_html`.
- A document whose only headings are HTML now gets a contents pane, where
  before it got none.
- The Scoop manifest is generated by the release workflow from the checksums
  it has just written, and attached to the release, rather than kept in the
  repository. A manifest pins a hash, and a hash cannot exist before the
  archive it describes, so a checked-in one is stale from the moment a
  release is tagged until somebody remembers to move it — which is how it
  came to sit at 0.1.0 through two releases. What remains here is the
  template it is filled from.

## [0.3.0] - 2026-08-20

### Added

- The reader mentions a newer release on the way out: one line on standard
  error, from an answer cached at most once a day and refreshed by a detached
  background thread, so the check never delays startup, rendering, or exit.
  It stays quiet in scripts and CI — standard error must be a terminal — and
  `update-check = false` in the configuration (or `MARQUEE_UPDATE_CHECK=0`)
  turns it off for good.
- `cargo binstall marquee-markdown` finds the prebuilt release archives, so
  upgrading no longer requires a compile.

### Changed

- The configuration structs — `config::{Config, Layer, File}` and the sections
  inside them — are `#[non_exhaustive]`, and gained a field in the same
  release. Both are breaking for anything that built one with an exhaustive
  struct literal, which is why this is a minor bump rather than a patch, and
  it is the last time: outside this crate they are now built from `Default`
  and assigned to, so the next setting costs nobody a release. Reading and
  writing their fields is unchanged.
- Releases publish to crates.io from the release workflow itself, through
  crates.io trusted publishing, so the GitHub tag and the published crate —
  including the README — always come from the same commit. The workflow now
  refuses a tag that does not match `Cargo.toml`, and a changelog with no
  notes for the version.

### Fixed

- `e` no longer makes the editor sluggish. The terminal reader blocked in
  `read` for the life of the process, including while the editor it had just
  launched owned the terminal — so two processes sat on the same tty and each
  took an arbitrary half of everything typed. Editors lost whole words, stalled
  waiting for the tail of escape sequences that had been eaten, and waited out
  timeouts on the questions they ask a terminal at startup; the stolen keys
  were then replayed as commands the moment the editor exited. The reader now
  stands down for as long as another program has the terminal, and does not
  hand it over until it has acknowledged from a point where it holds no read.
  Anything the other program left behind is discarded rather than parsed as
  keystrokes. `scripts/handoff-check.py` reproduces the original defect.
- Pasting into the search or filter prompt no longer runs as keystrokes.
  Bracketed paste was never enabled, so a paste arrived as ordinary keys: the
  newline in a multi-line paste submitted the prompt, and the rest was
  dispatched as bindings, where `q` quits and `e` opens an editor. The handler
  that strips control characters from a paste existed but could never run.
  Editors turn bracketed paste off on the way out, so a single `e` used to
  break pasting for the rest of the session.
- A window resized while an editor had the terminal is no longer drawn at the
  old width. The resize signal goes to the foreground process group, which by
  then is the editor's, so nothing reported it and the mangled frame stood
  until the next keypress.
- Saving repeatedly during one editing session no longer costs a re-read and a
  full re-layout per save when the editor exits. Only the first reload in a
  batch is kept; the rest could only reproduce it.
- Failing to take over the terminal no longer leaves it taken over. If building
  the backend failed there was no guard yet to restore it, and a write that
  failed part way through `restore` skipped leaving raw mode — the wedged shell
  that module exists to prevent.
- Table cells no longer wrap when they fit. The column solver measured a cell's
  plain text while the emitter drew its fragments, and an inline code span is
  drawn as a padded chip two cells wider than its text — so a cell like
  `` `Replay` `` was handed a column too narrow for itself and became a
  three-line row: a blank line, the text, another blank. Both paths now measure
  the fragments that get drawn.

### Documentation

- Screenshots of the file browser, the `paper` theme, search, and a
  side-by-side rendering of the same document through `glow`. All generated by
  `scripts/screenshot.py --all`, which now understands 256-colour, 16-colour
  and attribute escape sequences, can type into the program it is
  photographing, and enforces the terms of the comparison in code.
- Corrected three rows of the glow comparison table that described glow's
  piped output rather than what it does on a terminal.

## [0.2.1] - 2026-08-19

A correctness release: a line could come out wider than the column it had to
fit, which is the one thing the rendering design cannot survive.

### Added

- **Property tests** over the invariants the design rests on: the width
  invariant against generated markdown at arbitrary widths, with a corpus
  chosen to disagree with itself about how wide it is (CJK, ZWJ emoji
  families, regional indicators, variation selectors, combining marks, Nerd
  Font private-use glyphs, RTL); search hits always naming a real place on
  the page; the highlight index agreeing with the matches; drawing staying
  inside its buffer; and scrolling never leaving the document. `proptest` had
  been a declared dependency doing nothing since the first commit.
- A link to a heading in the same document (`[x](#section)`) now scrolls to it
  instead of being handed to the system opener, which did nothing useful with
  a bare fragment. The outline already knows where every slug is. Copying such
  a link copies `#section` — what belongs back in a markdown file.

### Fixed

- **A lead wider than the column no longer overflows the line.** Quotes and
  lists nested deeply enough accumulate a prefix wider than a narrow
  terminal's whole column — three levels of `> - >` is already twelve cells —
  and the line came out wider than the column it had to fit. That is the one
  thing nothing downstream survives: the painted page tears and a code
  container stops sealing. The decoration now gives way to the text, which is
  the point of the line. Found by the new property tests, not by a fixture.
- Links in a fetched document are resolved properly rather than concatenated:
  a root-relative `/docs/x` resolved against the directory instead of the host
  and produced a 404, protocol-relative `//host/x` was mangled, and `..` was
  never folded away. `mailto:` and other schemes are recognised without
  relying on `://` appearing.

## [0.2.0] - 2026-08-19

Every rough edge 0.1.0 shipped with, resolved. One breaking change to the
library: `AlertKind::icon()` is gone — the icon accessor moved to
`Theme::alert_icon`, because glyph choice is a font question and fonts are the
theme's business.

### Added

- **Icons are theme data.** The callout and image glyphs default to standard
  Unicode symbols (ⓘ ✦ ‼ ⚠ ✖ ▣) that render in any monospace font — no more
  missing-glyph boxes without a Nerd Font — and an `[icons]` table in a theme
  file overrides them. The Nerd Font set is documented in the README.
- **The key reference scrolls.** On a terminal too short for every binding,
  the movement keys move the overlay itself, and its title shows where you
  are in the list.
- **The browser rescans.** `r` walks the directory again — keeping the filter
  and, when the file still exists, the cursor — and `.` toggles hidden and
  git-ignored files live. Every walk carries a generation, so reports from a
  superseded walk are dropped rather than repopulating a cleared list.
- **Search matches across soft wraps.** A phrase broken onto two lines by
  wrapping now matches, with the highlight split across both. Markers and
  gutter bars (`•`, `▎`, list numerals) are decoration and no longer match.
- **Search narrows as you type.** The count updates live in the status bar;
  `enter` commits and jumps, `esc` abandons the query and restores the
  previous highlight.

### Changed

- `AlertKind::icon()` removed; use `Theme::alert_icon(kind)` and
  `Theme::image_icon()`. `LineMeta` gains `lead_cols` and
  `LineSink::push_spans` takes the lead separately — decoration and content
  are now distinguishable in the rendered metadata.
- `Search`'s `refresh` is subsumed by `ensure(doc, revision, query,
  from_line)`, idempotent per `(query, revision)`; `Match` is multi-segment.
- `default-run = "marquee-markdown"`, so bare `cargo run` works again beside
  the `mmd` binary.
- Dropped the unused `insta` dev-dependency; the frame tests assert
  structural properties, which is sturdier than a snapshot and does not churn.

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

[Unreleased]: https://github.com/SophanaSok/marquee-markdown/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/SophanaSok/marquee-markdown/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/SophanaSok/marquee-markdown/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/SophanaSok/marquee-markdown/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/SophanaSok/marquee-markdown/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/SophanaSok/marquee-markdown/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/SophanaSok/marquee-markdown/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/SophanaSok/marquee-markdown/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/SophanaSok/marquee-markdown/releases/tag/v0.1.0
