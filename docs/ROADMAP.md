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
| **P2** Document reader | Terminal guard + panic hook, event loop, view/anchor/render cache, pager keys via `Action`, status bar, keymap-rendered help, `-t` | 3 | **Done** |
| **P3** TOC + search | Outline tree, active-section derivation, focus model, collapse/auto-hide, `/` `n` `N` | 3 | **Done** (no TOC filter) |
| **P4** Browser | Streaming gitignore-aware walk, paging, fuzzy filter with Unicode normalization, humanized modtimes, rescan and a live hidden-files toggle, `-a` | 3 | **Done** |
| **P5** Remote sources | `Fetcher` trait, http(s), `github://`/`gitlab://`, bare-host README API | 2 | **Done** |
| **P6** Parity polish | Live reload, `e` at scroll line, `c` copy, `-p` pager, `ctrl+z`, link following, `y` | 2 | **Done** |
| **P7** Config + keymaps | TOML schema, `MARQUEE_` env layer, precedence, user keymap merge, `config` subcommand | 2 | **Done** |
| **P8** Release | `packaging/`, deb/rpm, release workflow, `docs/ARCHITECTURE.md`, crates.io | 2 | **Done** |

## Known gaps

- **Block nesting is capped at 256 levels.** Deeper than that, a container is
  not represented and its children are laid out at the capped indent instead.
  Layout walks the tree by recursion, and a stack overflow aborts rather than
  unwinding, so a document could otherwise kill the reader without the panic
  hook ever restoring the terminal. Two cells of lead per level fills an
  80-column line by about level 40, so nothing showable is lost.

- **`--style system` does not ask on Windows.** The `OSC` replies arrive there
  through the console input API rather than as bytes on a device, which is a
  different mechanism from the `/dev/tty` exchange in `src/util/osc.rs` rather
  than a variation on it. Windows Terminal does answer these questions, so
  this is worth writing; until it is, `system` falls back to a shipped
  palette, as it does for any terminal that stays quiet. It also means the
  Windows console never becomes a terminal worth re-asking, which the
  follow-the-terminal path below gets right for free.

- **`--style system` follows the terminal by inference, not by being told.**
  DEC mode 2031 is the terminal-native way to be told a palette changed, and
  Ghostty and kitty both implement it. It is not used, because crossterm 0.29
  cannot receive it: `parse_csi` handles only `u` and `c` after `CSI ?`, and
  its own comment says `Ok(None)` means *wait for more bytes* — so a
  `CSI ?997;1n` notification does not degrade, it stalls the parser on a
  buffer that never completes and swallows every later keystroke. That is
  crossterm #1104, filed independently; the fix (#1106) and mode 2031 support
  (#1052) are both open and unmerged, and crossterm's last release is 0.29.0
  from April 2025.

  Until that lands, `src/app/recolor.rs` infers instead: a focus regain, a
  watched path, `SIGUSR1` or the `R` key, each answered with a two-sequence
  probe. When crossterm can parse it, 2031 becomes one more trigger into the
  same path and nothing else changes.

## What works today

`marquee-markdown file.md` is a working replacement for `glow file.md` on local
sources: files, directory READMEs, stdin, and syntax-highlighted source files.
Themes load from TOML. Output degrades correctly when redirected.

`marquee-markdown -t file.md` is a working pager: every glow pager key, a
status bar, a scrolling key reference rendered from the live keymap, the mouse
wheel claimed from the terminal, light/dark switching, and a resize that keeps
your place instead of teleporting you.

`marquee-markdown -t -s system file.md` follows the terminal while you read:
change the colorscheme, or the desktop theme behind it, and the page is
repainted without a keystroke.

826 tests and a doctest, plus five `#[ignore]`d live checks against the real
forges; `cargo clippy --all-targets -- -D warnings` and `cargo doc --no-deps`
clean. Three pty checks under `scripts/` cover what a unit test cannot reach —
handing an editor the terminal, claiming the wheel, and following a retint
without eating the keyboard.

## How it got here

The pre-1.0 launch runbook, kept because each item records what it cost:

1. **Push to GitHub and watch CI go green.** Worth what it cost:
   the first four runs found a licence to allow, two advisories (removed by
   narrowing syntect's features rather than waived), an MSRV violation that
   compiles fine on a current toolchain, three Windows tests that assumed `/`
   as a path separator, a macOS file-watch test asserting a precision FSEvents
   does not offer — and a real bug in which a named source was silently
   ignored in favour of redirected standard input.
2. **Both pre-1.0 decisions are now made**, and implemented:
   - **The short alias is `mmd`**, installed alongside `marquee-markdown` by
     every install method. Both binaries are stubs over `cli::run`, so they
     cannot drift, and the generated man page and completions are named after
     whichever was invoked.
   - **The library API is in two halves.** A small stable surface —
     `render::{render, render_with, Document, RenderedDoc, LayoutOptions,
     ParseOptions, HtmlMode, ansi, tui, overlay, measure}` and `theme` — and the pipeline behind it, marked
     `#[doc(hidden)]` and free to change. `Document` was added to make that
     split possible: parse-once-lay-out-many is the thing a consumer actually
     needs, and having it opaque means the block tree never has to be frozen.
     `cargo semver-checks` now runs in CI against the published version.
3. **Tag `v0.1.0` and publish.** On crates.io and GitHub releases,
   verified by installing from both. The release workflow's first run found
   the retired Intel macOS runners; the Intel build is now cross-compiled.

Beyond that, the deferrals below are the backlog — HTML tables and lists, a
scrollable wide table, and images are the three most likely to be asked for.

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
   than remapping them separately. A resize *burst* is already coalesced: the
   loop drains the whole queue per iteration and `Event::Resize` is a no-op, so
   dragging an edge costs one re-layout per frame rather than one per event.
   The cost of that one re-layout was almost entirely re-highlighting — 97% of
   it on a document of `rust` fences — and highlighting is now memoized on the
   `Document` for as long as the parse it belongs to, keyed on the theme it
   was produced for. Nothing here is open.

   The other half of this is that the geometry has to be decided from the size
   the terminal *is*. `drive` asks `Terminal::autoresize` before `reconcile`,
   because `get_frame` reports the area as of the last draw: reading it
   directly laid the document out one width behind the window it was drawn
   into, and a resize that stopped left that standing.

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

## Known rough edges

All four that 0.1.0 shipped with were resolved in 0.2.0 (icons as theme data,
a scrolling key reference, browser rescan with a live hidden-files toggle,
and cross-wrap search that narrows as you type). What remains is smaller:

- On macOS, file system events arrive per directory, so saving a sibling file
  can trigger a redundant reload — re-read, never shown wrongly.
- A single overlong word hard-split at the column edge does not match across
  its split; there is no space there for the search joiner to stand in for.

## Deferred deliberately

- **HTML with no emitter behind it.** `<table>`, `<ul>`/`<ol>`/`<li>`,
  `<details>` folding, and `style="text-align:…"`. Each falls back to literal
  markup, which is no worse than before HTML was interpreted at all. Tables
  are the one worth doing, and the reason not to yet is that the column
  solver is the most delicate code in the project — feeding it a second
  source of cells wants its own change, not a rider on this one.
- **Images.** The target terminal (foot) has sixel, but Alacritty has nothing
  and no terminal here supports the kitty protocol. `ratatui-image` would add a
  blocking resize on the draw thread. Revisit only if asked.
- **Table `Scroll` overflow mode.** The `label: value` card fallback covers
  narrow widths well enough. A horizontally pannable wide-table view is the
  right answer for data-heavy documents; the solver is already factored so it
  slots in without disturbing the fitting logic.
- **A workspace split.** One crate with a strict lib/bin split; the layering
  test keeps extracting `render` into its own crate a move rather than an
  excavation. Do it if someone actually wants to depend on the renderer
  without the reader.

## Reference

The upstream surface being matched is `glow` 3.0.0. The complete flag set,
keybindings, source-resolution order, and file-discovery rules were captured
during design; `src/cli/mod.rs` and `src/source/classify.rs` encode them, and
their tests are the executable specification.
