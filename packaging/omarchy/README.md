# Omarchy

`--style system` follows the terminal on its own — regaining focus is a trigger
and needs nothing installed. This directory is for the case where that is not
enough, and for the one trigger with no race in it.

| File | What it is for |
| --- | --- |
| `theme-set.d/reload-marquee` | An Omarchy theme hook that signals every running reader once the retint has finished |

```sh
install -Dm755 packaging/omarchy/theme-set.d/reload-marquee \
  ~/.config/omarchy/hooks/theme-set.d/reload-marquee
```

The hook spells the long binary `marquee-markdow`, which is not a typo:
`pkill -x` matches the kernel's process name and that is capped at 15
characters, so the full spelling matches nothing. `tests/docs.rs` fails if it
ever gains the sixteenth letter back.

`omarchy-theme-set` calls its hooks *after* rewriting the terminal
configurations and reloading the terminals, so a hook that fires here is
asking about colors that have already changed. The other triggers have to
tolerate arriving early, which is what the probe-and-compare in
`src/app/recolor.rs` is for.

The configuration-only alternative, with no file to install:

```toml
[theme]
watch = ["~/.local/state/omarchy/current/theme"]
```

That watches the state Omarchy writes when a theme is applied. It is a hint
about where this machine keeps its theme rather than something compiled in —
no desktop's directory layout is in the Rust, because there is no version of
that list that stays right.
