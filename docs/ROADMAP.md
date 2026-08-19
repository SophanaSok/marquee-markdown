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
| **P3** TOC + search | Outline tree, active-section derivation, focus model, filter/collapse/auto-hide, `/` `n` `N` | 3 | Next |
| **P4** Browser | Streaming gitignore-aware walk, paging, fuzzy filter with Unicode normalization, humanized modtimes, `-a` | 3 | |
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

269 tests; `cargo clippy --all-targets -- -D warnings` and `cargo doc --no-deps`
clean.

## Immediate next steps (P3)

The sidebar is the reason the project exists, and it is where the risks below
stop being theoretical.

1. `doc/outline.rs` — the flat `Vec<Anchor>` the renderer already produces,
   folded into a tree with skipped levels handled (`#` straight to `###` is
   common and must not produce an orphan).
2. `app/state.rs` — add `Focus`, and keep `Mode` **derived** from it the way it
   is derived from the overlay today. Adding a stored mode alongside is how the
   two start disagreeing.
3. `keymap.rs` — a `Mode::Toc` block. Per the collision table: `h`/`l` collapse
   and expand there, `/` filters, and neither steals what those keys do in the
   document.
4. `app/layout.rs` — the sidebar pane, with auto-hide below a width threshold.
   `Panes` already exists to be extended; a hidden sidebar must be `None`, not
   a zero-width rectangle, so widgets cannot draw into nothing.
5. `ui/toc.rs` — **slice rows manually.** `ListState` mutates its offset during
   render and would take `&mut App` into the draw path, which is the one thing
   the architecture does not allow.
6. `doc/search.rs` — scan `RenderedDoc::plain` once, convert byte matches to
   `(line, column range)`, and highlight at draw time through an overlay
   primitive. Do not re-lay out to search.
7. `/` `n` `N` through `Action`, and a prompt mode that **captures all
   printable input** — a `q` typed into a filter must not quit.

Two things to watch, both recorded here because they are cheap now and
expensive later: the TOC cursor (user-moved) and the active section
(scroll-derived) are separate pieces of state and must stay that way, and
search matches are line indices, so they belong in the `ensure_rendered`
remapping alongside the scroll anchor.

## What P2 built, and why it is shaped that way

- `app/terminal.rs` — an RAII alternate-screen/raw-mode guard, plus a panic
  hook that restores the terminal *before* the message is printed. Without the
  hook the message lands on the alternate screen and disappears with it.
- `app/action.rs` — `Action` came first, and input is routed through it. No
  code anywhere matches on a `KeyCode` except the keymap, which is what makes
  P7 a data swap. A test forces a new variant to be added to `Action::ALL`, so
  it cannot be unbindable and invisible in the help overlay.
- `app/keymap.rs` — the single table of default bindings. Duplicate chords in a
  mode are an error rather than a silent overwrite, because the loser would
  still appear in the help overlay.
- `app/state.rs` — `Mode` is *derived* from what is open, never stored. The
  "closed prompt still swallows keys" bug is unreachable rather than fixed.
- `app/event.rs` — the loop consumes its own `Event` enum, so a headless test
  feeds exactly what a terminal would, and later producers (file watcher,
  directory walk) plug in without touching the update logic.
- `app/update.rs` — the only mutation site.
- `app/mod.rs` — `reconcile` before `draw`: pane geometry, then the layout
  cache, then derived state.
- `doc/cache.rs` — the single re-render funnel.
- `render/tui.rs` — the buffer serializer, paired with `render/ansi.rs`. One
  layout engine, two destinations.
- `ui/` — draw-only widgets taking `&App`.

Tests worth knowing about before changing any of it:

- `tests/keyseq.rs` drives whole key sequences headlessly and asserts on a
  one-line state summary. This is the cheapest coverage in the project for
  modal bugs, and the place to add a case when a mode is added.
- `tests/frame.rs` draws at seven terminal sizes down to 1×1 and asserts every
  cell carries an explicit background — the painted page has no other
  mechanical guard.
- `tests/docs.rs` fails if a bound key is missing from the README table.

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
   **Still open:** a resize debounce (~80 ms), or dragging a window edge
   re-renders a large document on every event.

3. **Purity vs. layout-dependent state.** Half-page scroll, TOC auto-scroll,
   clamping, and TOC auto-hide all need pane dimensions that naturally only
   exist inside `draw`, and ratatui's `StatefulWidget`/`ListState` is *designed*
   to mutate offsets during render. Taking `&mut App` in draw destroys headless
   testability. **In place since P2:** `app::reconcile` computes
   `layout::compute(area, &App) -> Panes` and the derived state before drawing,
   and `ui::draw(&mut Frame, &App)` has no `&mut` to abuse. The remaining half
   of the mitigation is P3's: slice TOC rows manually instead of reaching for
   `ListState`.

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
