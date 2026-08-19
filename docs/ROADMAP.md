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
| **P0** Skeleton | Manifest with crates.io + deb/rpm metadata, lib/bin split, clippy config, pure-Rust syntect backend, render-isolation test | 2 | **Done** (CI and contributor docs still open) |
| **P1** One-shot render + theming | Source classification, frontmatter, code-file wrapping, ANSI output, `-l -n -w -s`, theme loader, `themes`/`man`/`completion` | 3 | **Done** except `-n` |
| **P2** Document reader | Terminal guard + panic hook, event loop, view/anchor/render cache, pager keys via `Action`, status bar, keymap-rendered help, `-t` | 3 | Next |
| **P3** TOC + search | Outline tree, active-section derivation, focus model, filter/collapse/auto-hide, `/` `n` `N` | 3 | |
| **P4** Browser | Streaming gitignore-aware walk, paging, fuzzy filter with Unicode normalization, humanized modtimes, `-a` | 3 | |
| **P5** Remote sources | `Fetcher` trait, http(s), `github://`/`gitlab://`, bare-host README API | 2 | |
| **P6** Parity polish | Live reload, `e` at scroll line, `c` copy, `-p` pager, `-m` mouse, `ctrl+z`, link following, `y`, theme cycling | 2 | |
| **P7** Config + keymaps | TOML schema, `MARQUEE_` env layer, precedence, user keymap merge, `config` subcommand | 2 | |
| **P8** Release | `packaging/`, deb/rpm, release workflow, `docs/ARCHITECTURE.md`, crates.io | 2 | |

## What works today

`marquee-markdown file.md` is a working replacement for `glow file.md` on local
sources: files, directory READMEs, stdin, and syntax-highlighted source files.
Themes load from TOML. Output degrades correctly when redirected.

176 tests; `cargo clippy --all-targets -- -D warnings` clean.

## Immediate next steps (P2)

1. `app/terminal.rs` — RAII alternate-screen/raw-mode guard with a panic hook
   that restores the terminal, so a panic never leaves a wedged shell.
2. `app/action.rs` — the `Action` enum first. **Route input through it from the
   very first commit**, even while the keymap is a hardcoded table; pattern
   matching raw `KeyCode` in `update` makes P7 a rewrite instead of a data swap.
3. `app/state.rs` — `App`, `Screen`, `Focus`, with `InputMode` *derived* rather
   than stored, so focus and keymap cannot disagree.
4. `app/event.rs` + `app/update.rs` — one mpsc of events; `update` is the only
   mutation site.
5. `doc/cache.rs` — the single re-render funnel (see the risk note below).
6. `ui/` — draw-only widgets taking `&App`.
7. Help overlay rendered **from the live keymap**, never a string literal.

## Sequencing constraints

These exist to avoid rewrites, and each one has already been paid for once in
the design:

- **P2 must use the `Action` enum from day one.** Otherwise P7 rewrites the
  event loop.
- **The help overlay must be a keymap renderer**, not a literal, for the same
  reason.
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
   leaps on resize. **Mitigation for P2/P3:** a single `doc::cache::ensure_rendered()`
   funnel that snapshots a scroll anchor (from `LineMeta.source` byte offsets,
   already recorded), re-renders, remaps everything, and bumps a revision
   counter. Nothing else may assign to `lines`. Add a resize debounce (~80 ms)
   or dragging a window edge will re-render a large document on every event.

3. **Purity vs. layout-dependent state.** Half-page scroll, TOC auto-scroll,
   clamping, and TOC auto-hide all need pane dimensions that naturally only
   exist inside `draw`, and ratatui's `StatefulWidget`/`ListState` is *designed*
   to mutate offsets during render. Taking `&mut App` in draw destroys headless
   testability. **Mitigation:** a pure `layout::compute(term_size, &App) -> Panes`
   called in a reconcile step before draw; `ui::draw(&mut Frame, &App)` with no
   `&mut` available; slice rows manually instead of using `ListState`.

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
