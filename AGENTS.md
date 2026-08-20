# AGENTS.md

Build, test, and lint commands plus the architecture of `marquee-markdown`.

## Build

```sh
cargo build
cargo build --release
```

Requires Rust 1.88 (let-chains). `rust-version` in `Cargo.toml` is the single
statement of that, and CI reads it from there rather than repeating it.

No C toolchain is required: `syntect` uses the pure-Rust `fancy-regex` backend
(`default-features = false, features = ["default-fancy"]`, with `two-face` on
`syntect-fancy` to match). Do not switch to the default `onig` backend — it
would make Windows builds need MSVC and slow every contributor's cold build.
The ~2x highlighting cost is irrelevant because highlighting is per code block.

## Run

```sh
cargo run -- README.md
cargo run -- -w 80 -s paper doc.md

# Look at rendered output without installing:
cargo run --example preview -- tests/fixtures/kitchen-sink.md 80 slate
```

## Test

```sh
cargo test
cargo test --lib            # unit tests only, fast

cargo build && python3 scripts/handoff-check.py   # needs a pty, not a tty
```

Unit tests live inline in each module under `#[cfg(test)] mod tests`.
Cross-cutting invariants live in `tests/`. What needs two processes and a real
terminal lives in `scripts/`, because `cargo test` cannot express it: the
handoff check runs the binary under a pty with a stub `$EDITOR` that records
what it was sent. CI runs it on Linux; it is not part of `cargo test`, so run it
by hand when touching the terminal, and expect to have to.

**`cargo test` links a large dev-dependency tree and can take several minutes
cold.** Run `cargo check --all-targets` first to surface compile errors in
seconds.

## Lint and format

```sh
cargo clippy --all-targets -- -D warnings
cargo fmt --all
cargo fmt --all -- --check
```

Project clippy config is in `Cargo.toml` under `[lints.clippy]`.

Clippy is MSRV-aware: it suppresses suggestions that need a newer Rust than
`rust-version` promises. Getting that field wrong therefore hides lints as well
as misleading users.

## Architecture

Two halves, with a hard boundary between them.

### `src/render/` — the engine

Reusable, and **must not reference `app`, `ui`, `browser`, or `doc`**;
`tests/layering.rs` fails the build if it does. This keeps the renderer
extractable into its own crate later.

```
measure.rs   The single width chokepoint. Nothing else may call unicode_width.
block.rs     Intermediate block tree: Block, BlockKind, Inline, ListItem.
parse.rs     pulldown-cmark events -> block tree. Source byte ranges, heading
             slug dedup, GFM alerts. Cached; never re-run on resize.
html.rs      Raw HTML -> blocks and inlines, or a decision to show it as
             markup. A whitelist: an element with no emitter behind it sends
             its whole block back to literal rather than being half-rendered.
frag.rs      Inline content -> Frag (text + style + link + precomputed width).
wrap.rs      Span-aware line breaking. WrapMode::{Word, HardAtColumn}.
sink.rs      LineSink: the ONLY emitter of lines. Owns the width invariant.
layout/      Per-block emitters: heading, para, list, quote, rule, code, table.
highlight.rs syntect -> ratatui styles directly, surface background forced.
doc.rs       RenderedDoc: lines, per-line meta, outline, links, plain mirror.
overlay.rs   Draw-time restyling of column ranges (search highlight).
ansi.rs      RenderedDoc -> SGR bytes + OSC 8, for the stdout path.
tui.rs       RenderedDoc -> a ratatui buffer, for the reader.
```

Pipeline: `source -> parse -> layout (fragment, wrap, emit) -> RenderedDoc`,
then either a ratatui buffer (reader) or ANSI bytes (`ansi.rs`).

### `src/` — the shell

```
cli/         clap derive with the full glow flag surface; pure run-mode dispatch.
config/      The file schema, the environment layer, and precedence.
  layer.rs     `Layer::over` — the ONE definition of precedence.
  schema.rs    The TOML shape; unknown keys warn rather than fail.
  keys.rs      `[keys.*]` merged over the defaults; generates KEYBINDINGS.md.
source/      classify (pure, behind FsProbe) + resolve (I/O); frontmatter; kind.
  fetch.rs     The Fetcher seam: HttpFetcher for real, FakeFetcher for tests.
  remote.rs    URLs and the two forge APIs, written against Fetcher.
theme/       Palettes, the TOML theme format, and the theme registry.
util/        Terminal detection, width rules.
oneshot.rs   Non-interactive render to stdout.

app/         The reader: state, input, and the loop.
  action.rs    Every action the reader can perform. Input resolves to one of
               these before reaching any logic.
  keymap.rs    Chords, modes, and the one table of default bindings.
  state.rs     App. Mode is DERIVED from what is open, never stored.
  event.rs     The loop's own Event enum, plus a scripted source for tests.
  update.rs    The only mutation site.
  layout.rs    Pure: terminal size + state -> Panes.
  derived.rs   Recomputed once per iteration: clamping, active section.
  terminal.rs  RAII alternate-screen guard and the panic hook.
  external.rs  Editing and suspending: hand the terminal over, take it back.
  gate.rs      Standing the reader down so another program owns the terminal.
browser/     The file list, independent of any terminal.
  walk.rs      Streaming ignore-aware directory walk, on its own thread.
  filter.rs    Fuzzy matching, NFC-normalized on both sides.
  format.rs    Relative times for the list.
doc/         Document state, independent of any terminal.
  cache.rs     The ONLY caller of the layout engine; owns the heading tree.
  outline.rs   Headings as a tree, flattened into rows that know their subtree.
  search.rs    Hits over the plain mirror, as line-and-column ranges.
  links.rs     The document's links, and which one is stepped to.
  watch.rs     Noticing the file changed, on the directory not the file.
  view.rs      Scroll arithmetic, pure.
ui/          Draw-only widgets, each taking &App.
```

Two rules in the reader that are easy to undo by accident:

- **Pane geometry may not depend on anything only a layout can produce.** The
  contents pane asks `DocCache::heading_count`, counted from the block tree at
  parse time, rather than the outline, which does not exist until the first
  layout. Deciding from the outline made the pane appear on frame two, which
  changed the content width, re-laid out the document, and moved the reader.
- **The contents cursor and the active entry are different state.** The active
  entry follows the scroll position; the cursor is where the reader put it.
  Writing either into the other makes the pane feel broken.
- **The event thread owns standard input, so nothing may ask the terminal a
  question.** Any call that round-trips — `Terminal::clear`, which snapshots
  the cursor position, or `crossterm::cursor::position` — waits for a reply the
  event thread has already swallowed, and fails after a timeout. To force a
  full redraw after an external program, reset both ratatui buffers with two
  `swap_buffers` calls instead.
- **Reordering a list invalidates every index into it.** `Browser::extend` only
  appends; sorting happens in `refresh`, which rebuilds the match indices and
  re-finds the selected file by path in the same breath. Sorting anywhere else
  leaves the cursor pointing at a different file, with nothing to show for it.

### The reader's loop

```
RECONCILE  pane geometry -> layout cache -> derived state   (pure, no input)
DRAW       ui::draw(&App)                                   (no mutation)
RECEIVE    one event
UPDATE     update::handle(&mut App, event)                  (the only mutation)
```

Reconciling *before* drawing rather than during it is what lets the draw path
take `&App`. Two consequences to preserve:

- **`doc::cache::ensure_rendered` is the only path to a layout.** Scroll
  position, outline anchors, and search matches are all indices into
  `RenderedDoc.lines`, and every one is invalidated together by a resize or a
  theme switch. Remapping them in one place is why resizing keeps the reader's
  position instead of teleporting them. Do not call `render::layout` elsewhere.
- **Do not reach for `ListState`/`StatefulWidget`.** They mutate their offset
  during render, which would require `&mut App` in the draw path and destroy
  headless testability. Slice rows manually.

## The two invariants

Both are load-bearing. Breaking either produces subtle visual corruption rather
than a crash, so both are enforced mechanically.

**1. Every emitted line is exactly the content width.** `LineSink` is the sole
emitter and `debug_assert!`s this on every line. It is what makes the painted
column seamless, and it is why a long code line *cannot* escape its container:
nothing downstream is able to widen a line. If you add an emitter, route it
through `LineSink`; never push to `RenderedDoc.lines` directly.

**2. Escape sequences never reach width math.** `Frag.text` holds display text
only, links live in a separate field, and syntect output is converted to
`ratatui::Style` rather than to ANSI. There is deliberately no code path where
an escape byte can be counted as a column. This is the structural fix for the
bug that leaves glow's link-bearing lines ragged. Do not introduce a `Frag`
whose text contains escapes.

## Conventions

- `#![forbid(unsafe_code)]` on the library.
- Small, focused modules; one concern each.
- Inline `#[cfg(test)] mod tests` in every module.
- Prefer traits for real variation seams (`FsProbe`, `Fetcher`) so tests need
  no filesystem and CI needs no network. **No test in `cargo test` may touch
  the network.** Live checks against the real forges live in
  `tests/network.rs` behind `#[ignore]`; run them with
  `cargo test --test network -- --ignored` after changing anything in
  `source/remote.rs`, because a fake cannot notice an API changing shape.
- **UI render must be pure**: derive widgets from state, no mutation in draw.
  This matters most for the reader — see the roadmap's note on why a
  scroll-tracking table of contents tends to break it.
- Themes are data. Any new palette entry must be expressible in the TOML theme
  format, not hardcoded in layout code. Layout reads styles from `Theme`, never
  a named color.
- **Precedence is defined once**, in `config::layer::Layer::over`. Adding a
  setting means adding one field and one `or`, never a comparison at a call
  site.
- **Unknown configuration is a warning, not a failure.** A file written for a
  newer version must keep working with an older binary. Malformed TOML is still
  an error — that is a mistake, not version skew.
- A command-line switch that was not given must contribute `None`, not
  `Some(false)`, or it silently overrides the config file on every invocation.
- **`docs/KEYBINDINGS.md` is generated.** Regenerate with
  `cargo run -- keys > docs/KEYBINDINGS.md`; `tests/docs.rs` fails if it has
  drifted.
- New keys must go through the `Action` enum, never a hardcoded `KeyCode` match
  — otherwise the configurable keymap becomes a rewrite.

## Before a release

`packaging/README.md` has the sequence — now just changelog, bump, tag; the
workflow gates the rest and publishes to crates.io itself. The parts that
are easy to forget: `docs/KEYBINDINGS.md` is generated, the changelog
heading must contain the version (the release workflow refuses to release
without it), the Scoop
manifest is generated by the release workflow from the checksums rather
than kept in the repository, and the man page and completions come from the
binary rather than the repository.

## Testing notes

- The width invariant is verified over `tests/fixtures/kitchen-sink.md` at ten
  widths across both themes. That fixture exercises every construct the
  renderer handles; extend it rather than writing narrow one-off fixtures.
- Assert on the real contract, not a re-implementation of the code under test.
  An early outline test counted `#` characters in the source and disagreed with
  the parser over an edge case that the parser had right.
- `tests/keyseq.rs` types whole key sequences at a headless reader and asserts
  on `App::summary()`. Adding a mode without adding a case there is how the
  "typing `q` in a prompt quits" class of bug gets in.
- `tests/frame.rs` asserts every cell of a drawn frame carries an explicit
  background, at sizes down to 1x1. The one exception it skips is the cell a
  double-width glyph covers — ratatui's frame diff never writes there, because
  the glyph to its left already does.
- Some bugs are only reachable by running the binary: piped-stdin detection and
  broken-pipe handling were both invisible to the test suite. Run the binary
  after changing anything at the OS boundary.

  For the full-screen reader that means a pty, which `script` provides:

  ```sh
  printf 'jjq' | script -qec "stty rows 24 cols 80; \
      ./target/debug/marquee-markdown -t README.md" /dev/null
  ```

  Three things to know before writing one of these:

  - `q` closes an open overlay rather than quitting, so a script ending in `?q`
    hangs waiting for input rather than exiting.
  - Enter is `\r`. In raw mode `\n` is Ctrl+J, which crossterm correctly
    reports as a different key, so a script using `\n` types into a prompt
    forever.
  - Feed the keys with a delay between them. Sent in one burst, `esc` followed
    by a letter is indistinguishable from Alt+letter, which is exactly the
    ambiguity a real terminal has.

  Two P6 bugs were invisible to 470 passing tests and obvious on the first run:
  the file watch never fired for a relative path, and restoring the screen
  after an editor timed out on a terminal query. Neither is reachable from a
  unit test — but the second turned out to be reachable from a *process* test,
  and `scripts/handoff-check.py` is it: it runs the binary under a pty with a
  stub `$EDITOR` that records what it was sent. Its root cause was the reader
  thread never standing down, so it read the editor's keystrokes; that is what
  `src/app/gate.rs` now prevents, and the check fails without it.
