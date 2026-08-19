# Contributing

Thank you for looking. This document is the short version of what the code
already enforces; `AGENTS.md` has the longer reasoning behind the design.

## Getting set up

Rust 1.88 or newer. Nothing else — syntax highlighting uses a pure-Rust regex
backend on purpose, so there is no C toolchain and no system library to find.

```sh
cargo check --all-targets   # fast: use this while working
cargo test                  # unit, integration, and doctests
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

`cargo test` links a large dev-dependency tree and can take a few minutes from
cold. `cargo check --all-targets` surfaces compile errors in seconds; reach for
it first.

## What CI checks

Format, clippy with warnings denied, the full test suite on Linux, macOS and
Windows, `cargo doc` with warnings denied, a build at the minimum supported
Rust version, and `cargo package`. All of it runs locally with the commands
above.

**No test may touch the network.** Live checks against the real forges live in
`tests/network.rs` behind `#[ignore]`; run them deliberately with
`cargo test --test network -- --ignored` after changing anything in
`src/source/remote.rs`, because a fake cannot notice an API changing shape.

## Things the code will not let you do, and why

Several rules here are enforced by tests rather than by review, because each
one failed once and was expensive to find:

- **`src/render/` may not reference the application shell**, and `src/doc/`,
  `src/browser/` and `src/source/` may not reference the screen.
  `tests/layering.rs` fails the build otherwise. This is what keeps the
  renderer usable as a library.
- **Widgets take `&App`, never `&mut App`.** Anything computed for the first
  time during a draw makes the frame irreproducible and the headless tests
  meaningless. Pane geometry and derived state are settled in `reconcile`,
  before drawing.
- **Every emitted line is exactly the content width.** `LineSink` is the only
  emitter and asserts it. Route new output through it.
- **Escape sequences never reach width math.** Links live in their own field
  and syntax colors become `ratatui::Style`, so no code path can count an
  escape byte as a column.
- **Keys go through the `Action` enum**, never a `KeyCode` match, or the
  configurable keymap stops working.
- **`docs/KEYBINDINGS.md` is generated.** Regenerate with
  `cargo run -- keys > docs/KEYBINDINGS.md`.

## Adding a feature

- Put unit tests inline in the module, in `#[cfg(test)] mod tests`. Test the
  contract, not a re-implementation of the code under test.
- If it adds a key or a mode, add a case to `tests/keyseq.rs`. It drives whole
  key sequences headlessly and is the cheapest coverage in the project for the
  bug class that matters most in a modal interface — a key doing the wrong
  thing because the wrong mode was in force.
- If it draws, `tests/frame.rs` will check every cell is painted at seven
  terminal sizes down to 1×1.
- **Run the binary.** Several bugs in this project were invisible to hundreds
  of passing tests and obvious within seconds of using it: piped-stdin
  detection, broken pipes, a file watch that never fired for a relative path, a
  terminal query that could never be answered. Anything touching the OS
  boundary needs a real terminal, and `AGENTS.md` explains how to drive one
  from a script.

## Themes

A theme is a TOML file, not code. If you want a new palette, you do not need to
write Rust — see the theming section of the README. If a change to the reader
needs a color that the theme format cannot express, the format is what should
change.

## Commits and pull requests

Explain *why*, not just what. The commit log is the main record of the
reasoning behind the design, and several of its entries have already saved
re-deriving a decision.

Small pull requests are easier to take. If you are planning something large,
open an issue first so the approach can be agreed before you spend the time.

## Conduct

By taking part you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
