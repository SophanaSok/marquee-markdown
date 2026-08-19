## What this changes

<!-- And why. The reasoning is the part that is hard to recover later. -->

## Checks

- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --all`
- [ ] Ran the binary, if this touches the terminal, the filesystem, or the
      network — several bugs here were invisible to the whole test suite and
      obvious on first use.
- [ ] Added a case to `tests/keyseq.rs`, if this adds a key or a mode.
- [ ] Regenerated `docs/KEYBINDINGS.md`, if this changes the default bindings.
