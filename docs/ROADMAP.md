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
| **P0** Skeleton | Manifest with crates.io + deb/rpm metadata, lib/bin split, clippy config, pure-Rust syntect backend, layering test | 2 | **Done** (CI and contributor docs still open) |
| **P1** One-shot render + theming | Source classification, frontmatter, code-file wrapping, ANSI output, `-l -n -w -s`, theme loader, `themes`/`man`/`completion` | 3 | **Done** except `-n` |
| **P2** Document reader | Terminal guard + panic hook, event loop, view/anchor/render cache, pager keys via `Action`, status bar, keymap-rendered help, `-t` | 3 | **Done** (resize debounce open) |
| **P3** TOC + search | Outline tree, active-section derivation, focus model, filter/collapse/auto-hide, `/` `n` `N` | 3 | **Done** (no TOC filter) |
| **P4** Browser | Streaming gitignore-aware walk, paging, fuzzy filter with Unicode normalization, humanized modtimes, `-a` | 3 | Next |
| **P5** Remote sources | `Fetcher` trait, http(s), `github://`/`gitlab://`, bare-host README API | 2 | |
| **P6** Parity polish | Live reload, `e` at scroll line, `c` copy, `-p` pager, `-m` mouse, `ctrl+z`, link following, `y`, theme cycling | 2 | |
| **P7** Config + keymaps | TOML schema, `MARQUEE_` env layer, precedence, user keymap merge, `config` subcommand | 2 | |
| **P8** Release | `packaging/`, deb/rpm, release workflow, `docs/ARCHITECTURE.md`, crates.io | 2 | |

## What works today

`marquee-markdown file.md` is a working replacement for `glow file.md` on local
sources: files, directory READMEs, stdin, and syntax-highlighted source files.
Themes load from TOML. Output degrades correctly when redirected.

`marquee-markdown -t file.md` is a working pager: every glow pager key, a
status bar, a key reference rendered from the live keymap, light/dark switching,
and a resize that keeps your place instead of teleporting you.

348 tests; `cargo clippy --all-targets -- -D warnings` and `cargo doc --no-deps`
clean.

## Immediate next steps (P4)

The file browser. It is the last piece of glow parity that changes the shape of
the application, because it adds a second screen.

1. `browser/walk.rs` — a streaming `ignore`-crate walk on a worker thread,
   feeding results through the existing `Event` enum. It must stream: a walk of
   a large tree that blocks the first frame is the thing that makes a browser
   feel broken.
2. `app/state.rs` — a `Screen` enum. Keep `Mode` derived from it the way it is
   derived from focus today.
3. `keymap.rs` — `Mode::Browser`. Per the collision table, glow's browser paging
   keys (`b`/`u`/`f`/`d` full-page there, half-page in the pager) are reproduced
   verbatim and mode-scoped. That inconsistency is glow's, and `[keys.*]` is the
   fix glow cannot offer — document it as a quirk rather than silently
   improving it.
4. `browser/filter.rs` — fuzzy matching with `nucleo-matcher`, over
   NFC-normalized names, so a filename typed with combining marks still
   matches.
5. The filter prompt is a second `PromptKind`. It shares `Mode::Prompt`, which
   already captures all printable input; the sigil is what tells the two apart.
6. `ui/browser.rs` — draw-only, slicing rows by hand as the contents pane does.

## What P2 and P3 built, and why it is shaped that way

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
- `app/event.rs` — the loop consumes its own `Event` enum, so a headless test
  feeds exactly what a terminal would, and P4's directory walk plugs in as
  another producer without touching the update logic.
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
- `ui/` — draw-only widgets taking `&App`.

Two things that cost a debugging session each, recorded so they are not
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
- **`classify` already takes `FsProbe`** so P5 does not have to retrofit a seam
  under tests that already exist. Add `Fetcher` the same way.
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
- **A short binary alias.** `marquee-markdown` is long to type for a tool
  invoked constantly; a second `[[bin]]` is a three-line change whenever wanted.

## Reference

The upstream surface being matched is `glow` 3.0.0. The complete flag set,
keybindings, source-resolution order, and file-discovery rules were captured
during design; `src/cli/mod.rs` and `src/source/classify.rs` encode them, and
their tests are the executable specification.
