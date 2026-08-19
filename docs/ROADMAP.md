# Roadmap

Where the project is and what comes next. Effort is in relative units where
1 ≈ half a day.

## Goal

A terminal markdown reader with full `glow` feature parity, rendered in the
Claude artifact visual language, plus the two things glow lacks: a
scroll-tracking table-of-contents sidebar and in-document search.

## Status

| Phase | Contents | Effort | State |
| --- | --- | --- | --- |
| **P0** Skeleton | Manifest with crates.io + deb/rpm metadata, lib/bin split, clippy config, pure-Rust syntect backend, layering test | 2 | **Done** |
| **P1** One-shot render + theming | Source classification, frontmatter, code-file wrapping, ANSI output, `-l -n -w -s`, theme loader, `themes`/`man`/`completion` | 3 | **Done** |
| **P2** Document reader | Terminal guard + panic hook, event loop, view/anchor/render cache, pager keys via `Action`, status bar, keymap-rendered help, `-t` | 3 | **Done** (resize debounce open) |
| **P3** TOC + search | Outline tree, active-section derivation, focus model, filter/collapse/auto-hide, `/` `n` `N` | 3 | **Done** (no TOC filter) |
| **P4** Browser | Streaming gitignore-aware walk, paging, fuzzy filter with Unicode normalization, humanized modtimes, `-a` | 3 | **Done** (no rescan) |
| **P5** Remote sources | `Fetcher` trait, http(s), `github://`/`gitlab://`, bare-host README API | 2 | **Done** |
| **P6** Parity polish | Live reload, `e` at scroll line, `c` copy, `-p` pager, `ctrl+z`, link following, `y` | 2 | **Done** |
| **P7** Config + keymaps | TOML schema, `MARQUEE_` env layer, precedence, user keymap merge, `config` subcommand | 2 | **Done** |
| **P8** Release | `packaging/`, deb/rpm, release workflow, `docs/ARCHITECTURE.md`, crates.io | 2 | **Done** bar the first publish |

## What works today

`marquee-markdown file.md` is a working replacement for `glow file.md` on local
sources: files, directory READMEs, stdin, and syntax-highlighted source files.
Themes load from TOML. Output degrades correctly when redirected.

`marquee-markdown -t file.md` is a working pager: every glow pager key, a
status bar, a key reference rendered from the live keymap, light/dark switching,
and a resize that keeps your place instead of teleporting you.

529 tests, plus four `#[ignore]`d live checks against the real forges;
`cargo clippy --all-targets -- -D warnings` and `cargo doc --no-deps`
clean.

## What is left

The build is done. What remains is the part that needs a person rather than a
commit:

1. **Push to GitHub and watch CI go green.** Everything in `.github/` has been
   validated locally — the workflows parse, every command in them runs here —
   but no workflow is real until a runner has executed it. Expect the
   `cargo deny` licence list in `deny.toml` to need one or two additions.
2. **Both pre-1.0 decisions are now made**, and implemented:
   - **The short alias is `mmd`**, installed alongside `marquee-markdown` by
     every install method. Both binaries are stubs over `cli::run`, so they
     cannot drift, and the generated man page and completions are named after
     whichever was invoked.
   - **The library API is in two halves.** A small stable surface —
     `render::{render, Document, RenderedDoc, LayoutOptions, ansi, tui,
     overlay, measure}` and `theme` — and the pipeline behind it, marked
     `#[doc(hidden)]` and free to change. `Document` was added to make that
     split possible: parse-once-lay-out-many is the thing a consumer actually
     needs, and having it opaque means the block tree never has to be frozen.
     Add `cargo semver-checks` to CI once there is a published version to use
     as a baseline.
3. **Tag `v0.1.0` and publish.** `packaging/README.md` has the sequence.

Beyond that, the deferrals below are the backlog — images, a scrollable wide
table, and a scrollable key reference are the three most likely to be asked
for.

## What each phase built, and why it is shaped that way

- `app/terminal.rs` — an RAII alternate-screen/raw-mode guard, plus a panic
  hook that restores the terminal *before* the message is printed. Without the
  hook the message lands on the alternate screen and disappears with it.
- `app/action.rs` — `Action` came first, and input is routed through it. No
  code anywhere matches on a `KeyCode` except the keymap, which is what makes
  P7 a data swap. A test forces a new variant to be added to `Action::ALL`, so
  it cannot be unbindable and invisible in the help overlay.
- `app/keymap.rs` — the single table of default bindings, per mode. Duplicate
  chords in a mode are an error rather than a silent overwrite, because the
  loser would still appear in the help overlay.
- `app/state.rs` — `Mode` is *derived* from what is open and what has focus,
  never stored. The "closed prompt still swallows keys" bug is unreachable
  rather than fixed. A prompt binds almost nothing on purpose: any printable
  key it has not bound is text, which is what keeps `q` in a search box from
  quitting.
- `app/event.rs` — the loop consumes its own `Event` enum, and everything that
  can wake the reader posts into one queue: the terminal on its own thread, the
  directory walk on another. A headless test feeds exactly what a terminal
  would, and the browser tests feed walk results the same way. P6's file
  watcher is another producer, nothing more.
- `app/update.rs` — the only mutation site.
- `app/mod.rs` — `reconcile` before `draw`: pane geometry, then the layout
  cache, then derived state.
- `app/layout.rs` — pure pane geometry. The contents pane is `Option<Rect>`
  rather than a zero-width rectangle, so a widget cannot draw into a pane that
  is not there.
- `doc/cache.rs` — the single re-render funnel, which also owns the heading
  tree so the tree cannot outlive the lines it points into.
- `doc/outline.rs` — the heading tree, flattened back into rows that know
  their own subtree extent, so folding is a range skip.
- `doc/search.rs` — hits found over the plain mirror, as line-and-column
  ranges. Re-found rather than remapped when the document is laid out again.
- `render/overlay.rs` — draw-time restyling of column ranges, which is what
  lets search highlight without re-laying anything out.
- `render/tui.rs` — the buffer serializer, paired with `render/ansi.rs`. One
  layout engine, two destinations.
- `source/fetch.rs` — the `Fetcher` seam, with `FakeFetcher` beside it in the
  library rather than behind `#[cfg(test)]`, so integration tests and
  downstream users can exercise the remote paths too. `HttpFetcher` builds its
  client on first use, so the local path pays nothing for it.
- `browser/` — the walk, the filter and the selection, none of which know
  about a terminal. The walk reports in batches through a callback, so the
  browser does not depend on the application's event type either.
- `config/` — the file schema, the environment, and precedence, each a pure
  function of its input. `Layer::over` is the single definition of precedence
  for the whole program; defining it per field at each call site is how a
  program ends up with a flag that beats the config in one place and loses in
  another.
- `ui/` — draw-only widgets taking `&App`.

Five things that cost a debugging session each, recorded so they are not
rediscovered:

- **Pane geometry may not depend on anything only a layout can produce.** The
  contents pane first decided whether to show itself from the outline, which
  does not exist until the first layout — so it appeared on frame two, changed
  the content width, re-laid out the document and moved the reader a line. It
  now asks `DocCache::heading_count`, counted from the block tree at parse
  time. Anything else pane geometry needs must be available equally early.
- **The cursor and the active entry are different state.** Collapsing them is
  the single easiest way to make the contents pane feel broken: scrolling would
  drag the selection out from under the reader mid-keystroke.
- **A background reader thread owns standard input, so nothing may ask the
  terminal a question.** `Terminal::clear` snapshots the cursor position first,
  which is a round trip the event thread swallows the reply to; it times out
  and fails. Forcing a redraw after an external program means resetting both
  ratatui buffers instead. Anything else that round-trips — `cursor::position`,
  a colour query — has the same problem.
- **A watch matched on the whole path never fires for a relative argument.**
  `marquee-markdown README.md` gives a relative path and the events come back
  absolute. Matching on the file name is enough, because exactly one directory
  is watched and not recursively. The unit tests missed it by using absolute
  temporary paths; running the binary caught it.
- **Reordering a list invalidates every index into it.** The browser sorts by
  modification time, so results arriving mid-scan insert themselves above the
  cursor. Sorting inside `extend` left `matches` — and the cursor with it —
  pointing at different files, silently. Sorting now happens only in `refresh`,
  which rebuilds the indices in the same breath and re-finds the selected file
  by path.

Tests worth knowing about before changing any of it:

- `tests/keyseq.rs` drives whole key sequences headlessly and asserts on a
  one-line state summary. This is the cheapest coverage in the project for
  modal bugs, and the place to add a case when a mode is added.
- `tests/frame.rs` draws at seven terminal sizes down to 1×1 and asserts every
  cell carries an explicit background — the painted page has no other
  mechanical guard.
- `tests/docs.rs` fails if a bound key is missing from the README tables.
- `tests/layering.rs` enforces the module boundaries and that no widget takes
  `&mut App`.

## Sequencing constraints

These exist to avoid rewrites, and each one has already been paid for once in
the design:

- **Input goes through the `Action` enum** and the help overlay is a keymap
  renderer, both since P2. Undoing either turns P7 into a rewrite of the event
  loop.
- **`classify` takes `FsProbe` and `resolve` takes `Fetcher`.** Neither is
  optional: they are what let source resolution be tested with no filesystem
  and no network, and CI must never need either. Live checks live in
  `tests/network.rs` behind `#[ignore]`.
- **The theme loader is the only path to a `Theme`**, including built-ins, so
  user themes never become a second-class code path. Keep it that way.

## Known risks

1. **The width invariant is the whole design.** One leaky emitter ruins the
   painted column. Threats: measuring by `char` instead of grapheme cluster,
   emoji ZWJ sequences and variation selectors where `unicode-width` and the
   terminal disagree, and Nerd Font glyphs that report width 1 but draw as 2.
   Mitigation is in place: a single `measure::width` chokepoint and a
   `debug_assert!` in `LineSink`. Keep both.

2. **Line-index coherence across re-render.** Scroll position, outline anchors,
   search matches, and link spans are all indices into `RenderedDoc.lines`, and
   every one is invalidated by a resize, theme switch, reload, or `-w` change.
   Uncoordinated invalidation is what makes a table of contents feel broken —
   the highlight drifts a section off, `n` jumps to the wrong line, the view
   leaps on resize. **In place since P2:** `doc::cache::ensure_rendered()` is the
   only caller of the layout engine. It snapshots a scroll anchor from the
   `LineMeta.source` byte offsets, re-lays out, remaps the position, and bumps
   a revision counter. Nothing else may assign to `lines`, and P3's search
   matches must join the remapping rather than being remapped separately.
   P3 joined search hits to it by re-finding them on a revision change rather
   than remapping them separately. **Still open:** a resize debounce (~80 ms),
   or dragging a window edge re-renders a large document on every event.

3. **Purity vs. layout-dependent state.** Half-page scroll, TOC auto-scroll,
   clamping, and TOC auto-hide all need pane dimensions that naturally only
   exist inside `draw`, and ratatui's `StatefulWidget`/`ListState` is *designed*
   to mutate offsets during render. Taking `&mut App` in draw destroys headless
   testability. **In place since P2:** `app::reconcile` computes
   `layout::compute(area, &App) -> Panes` and the derived state before drawing,
   and `ui::draw(&mut Frame, &App)` has no `&mut` to abuse. The contents pane
   slices its rows by hand rather than using `ListState`, and
   `tests/layering.rs` fails the build if a widget takes `&mut App`. P4's
   browser list must do the same.

4. **Syntax theme clash.** Syntect themes carry their own background and
   foreground, which fight the palette. `highlight.rs` already forces the
   surface background on every span. The per-theme `syntax = ` key exists so a
   community theme can pair itself with a suitable syntax theme; picking good
   defaults for new palettes is real color-tuning work, not a config line.

## Deferred deliberately

- **Images.** The target terminal (foot) has sixel, but Alacritty has nothing
  and no terminal here supports the kitty protocol. `ratatui-image` would add a
  blocking resize on the draw thread. Revisit only if asked.
- **Table `Scroll` overflow mode.** The `label: value` card fallback covers
  narrow widths well enough. A horizontally pannable wide-table view is the
  right answer for data-heavy documents; the solver is already factored so it
  slots in without disturbing the fitting logic.
- **Workspace split.** One crate with a strict lib/bin split, with the render
  isolation test keeping extraction mechanical. Split only if someone actually
  wants to depend on the renderer.
- **A workspace split.** The layering test keeps extracting `render` into its
  own crate a move rather than an excavation. Do it if someone actually wants
  to depend on the renderer without the reader.
- **A scrollable key reference.** Below about 22 rows the overlay clips its
  last few bindings. `marquee-markdown keys` shows all of them, so nothing is
  unreachable, but the overlay should scroll.

## Reference

The upstream surface being matched is `glow` 3.0.0. The complete flag set,
keybindings, source-resolution order, and file-discovery rules were captured
during design; `src/cli/mod.rs` and `src/source/classify.rs` encode them, and
their tests are the executable specification.
