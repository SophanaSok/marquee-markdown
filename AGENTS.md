# AGENTS.md

Build, test, and lint commands plus the architecture of `marquee-markdown`.

## Build

```sh
cargo build
cargo build --release
```

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
```

Unit tests live inline in each module under `#[cfg(test)] mod tests`.
Cross-cutting invariants live in `tests/`.

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

## Architecture

Two halves, with a hard boundary between them.

### `src/render/` — the engine

Reusable, and **must not reference `app`, `ui`, `browser`, or `doc`**;
`tests/render_isolation.rs` fails the build if it does. This keeps the renderer
extractable into its own crate later.

```
measure.rs   The single width chokepoint. Nothing else may call unicode_width.
block.rs     Intermediate block tree: Block, BlockKind, Inline, ListItem.
parse.rs     pulldown-cmark events -> block tree. Source byte ranges, heading
             slug dedup, GFM alerts. Cached; never re-run on resize.
frag.rs      Inline content -> Frag (text + style + link + precomputed width).
wrap.rs      Span-aware line breaking. WrapMode::{Word, HardAtColumn}.
sink.rs      LineSink: the ONLY emitter of lines. Owns the width invariant.
layout/      Per-block emitters: heading, para, list, quote, rule, code, table.
highlight.rs syntect -> ratatui styles directly, surface background forced.
doc.rs       RenderedDoc: lines, per-line meta, outline, links, plain mirror.
ansi.rs      RenderedDoc -> SGR bytes + OSC 8, for the stdout path.
```

Pipeline: `source -> parse -> layout (fragment, wrap, emit) -> RenderedDoc`,
then either a ratatui buffer (reader) or ANSI bytes (`ansi.rs`).

### `src/` — the shell

```
cli/         clap derive with the full glow flag surface; pure run-mode dispatch.
source/      classify (pure, behind FsProbe) + resolve (I/O); frontmatter; kind.
theme/       Palettes, the TOML theme format, and the theme registry.
util/        Terminal detection, width rules.
oneshot.rs   Non-interactive render to stdout.
```

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
- Prefer traits for real variation seams (`FsProbe`, and `Fetcher` when the
  network lands) so tests need no filesystem and CI needs no network.
- **UI render must be pure**: derive widgets from state, no mutation in draw.
  This matters most for the reader — see the roadmap's note on why a
  scroll-tracking table of contents tends to break it.
- Themes are data. Any new palette entry must be expressible in the TOML theme
  format, not hardcoded in layout code. Layout reads styles from `Theme`, never
  a named color.
- New keys must go through the `Action` enum, never a hardcoded `KeyCode` match
  — otherwise the configurable keymap becomes a rewrite.

## Testing notes

- The width invariant is verified over `tests/fixtures/kitchen-sink.md` at ten
  widths across both themes. That fixture exercises every construct the
  renderer handles; extend it rather than writing narrow one-off fixtures.
- Assert on the real contract, not a re-implementation of the code under test.
  An early outline test counted `#` characters in the source and disagreed with
  the parser over an edge case that the parser had right.
- Some bugs are only reachable by running the binary: piped-stdin detection and
  broken-pipe handling were both invisible to the test suite. Run the binary
  after changing anything at the OS boundary.
