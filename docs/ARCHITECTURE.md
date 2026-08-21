# Architecture

A map of the code and the reasoning behind its shape. If you are about to
change something and want to know why it is the way it is, this is the file.

## The shape of the thing

Two halves with a hard boundary between them.

```
                     ┌──────────────────────────────────────┐
   markdown ────────▶│  render/   parse → layout → lines    │──▶ ratatui buffer
                     │            (no terminal, no state)   │──▶ ANSI + OSC 8
                     └──────────────────────────────────────┘
                                      ▲
                     ┌────────────────┴─────────────────────┐
                     │  the shell: source, doc, browser,    │
                     │  config, app, ui                     │
                     └──────────────────────────────────────┘
```

`render/` is a library that turns markdown into a fixed-width buffer of styled
lines. It knows nothing about terminals, scroll positions, or key presses. The
shell is everything else: finding a document, holding a reading position,
drawing, and reacting to input.

`tests/layering.rs` fails the build if that boundary is crossed. The point is
not tidiness — it is that the renderer stays usable on its own, and that
extracting it into its own crate would be a move rather than an excavation.

## The render pipeline

```
&str
 │
 ├─ parse.rs     pulldown-cmark events → Vec<Block>, with source byte ranges,
 │               deduplicated heading slugs, and GFM alerts.
 │               Width-independent, so it is cached and never re-run on resize.
 │
 ├─ html.rs      raw HTML → blocks and inlines, or a decision to show it as
 │               markup. Runs inside parse, not layout: an HTML heading has to
 │               reach heading_count, which pane geometry is decided from.
 │
 ├─ frag.rs      inline content → Vec<Frag>. A Frag is display text, a style, an
 │               optional link index, and a precomputed width.
 │
 ├─ wrap.rs      Vec<Frag> → wrapped lines, breaking at fragment boundaries.
 │
 ├─ layout/      one emitter per block kind: heading, para, list, quote, rule,
 │               code, table. All of them push through LineSink.
 │
 ├─ sink.rs      LineSink: the only thing that emits a line. Pads every line to
 │               exactly the content width, records anchors, links, and the
 │               plain-text mirror.
 │
 └─ RenderedDoc  lines, per-line metadata, the outline, interned links, the
                 plain mirror, and the width it was built at.
```

Raw HTML is a parse-time decision, which is why `ParseOptions` exists
alongside `LayoutOptions` rather than joining it. The two invalidate different
things: a layout option changes only the lines, and a resize re-runs that many
times a second; a parse option changes the tree. In practice the mode is fixed
at startup, so nothing in the reader has to handle it changing — but wiring it
to a key later would need a re-parse, not just `ensure_rendered`.

`Document` is the parsed half held on its own, so a resize re-runs only the
layout. It is opaque: the block tree behind it is the renderer's working
representation and changes as the pipeline does, and keeping it behind a type
is what lets the promised API stay small. That split is the stability boundary
— `render::{render, Document, RenderedDoc, LayoutOptions, ansi, tui, overlay,
measure}` and `theme` are stable from 1.0; the pipeline modules are
`#[doc(hidden)]` and free to change.

Two serializers take it from there: `tui.rs` writes into a ratatui buffer for
the reader, `ansi.rs` writes SGR bytes and real OSC 8 hyperlinks for standard
output. One layout engine, two destinations — which is why `marquee-markdown
file.md` and `marquee-markdown -t file.md` cannot disagree about what the
document looks like.

`overlay.rs` restyles column ranges on the way to a buffer. Search highlighting
and the selected link go through it, so neither re-lays anything out.

## The two invariants

Both are load-bearing. Breaking either produces subtle visual corruption rather
than a crash, so both are enforced mechanically rather than by review.

**1. Every emitted line is exactly the content width.** `LineSink` is the sole
emitter and asserts it on every line. This is what makes the painted page
seamless — gutters meet code-card fills with no join — and it is why a long
line inside a code block *cannot* escape its container: nothing downstream is
able to widen a line. `glow` gets this wrong, and the difference is visible.

**2. Escape sequences never reach width math.** `Frag.text` holds display text
only. Links live in a separate field and are turned into OSC 8 at serialization
time from recorded column ranges; syntax colors become `ratatui::Style` rather
than ANSI. There is deliberately no code path in which an escape byte could be
counted as a column. This is the structural fix for the bug that leaves glow's
link-bearing lines ragged.

`measure.rs` is the single width chokepoint. Nothing else calls
`unicode-width`, because the whole design rests on all the width arithmetic
agreeing with itself, and it only does if there is one of it. Widths are
measured over grapheme clusters, not chars: a combining sequence or an emoji
family is one cluster with one width, and splitting inside one tears it apart.

## The shell

```
cli/         clap derive; the full glow flag surface; pure run-mode dispatch.
             `run.rs` is the whole program, so the `marquee-markdown` and
             `mmd` binaries are stubs rather than two copies.
config/      A file, the environment, and the command line, resolved into one
             set of settings. `Layer::over` is the ONE definition of precedence.
source/      What an argument means (classify, pure, behind FsProbe) and how to
             get it (resolve; local files, and remote behind the Fetcher trait).
theme/       Two Claude palettes, and the TOML theme format they are instances
             of. Built-ins, user themes, and the one derived from the
             terminal's own colors all load through the same constructor;
             `system.rs` is a pure function of what the terminal answered, so
             asking it stays somebody else's job (`util/osc.rs`).
doc/         Document state with no terminal in it: the layout cache, the
             heading tree, search, links, the scroll position, the file watch.
browser/     The file list with no terminal in it: the walk, the filter, the
             selection.
app/         State, input, and the loop.
ui/          Draw-only widgets, each taking &App.
oneshot.rs   The non-interactive path, and the pager.
```

### The loop

One iteration, in order:

```
RECONCILE   pane geometry → layout cache → derived state    (pure, no input)
DRAW        ui::draw(&App)                                  (no mutation)
RECEIVE     one event, then drain everything else waiting
UPDATE      update::handle(&mut App, event)                 (the only mutation)
```

Reconciling *before* drawing rather than during it is what lets every widget
take `&App`. A scroll-tracking table of contents naturally wants to compute
pane sizes inside the draw call, and ratatui's `StatefulWidget` is *designed*
to mutate its offset during render — taking `&mut App` into the draw path would
make frames irreproducible and the headless tests meaningless. So rows are
sliced by hand, and `tests/layering.rs` rejects a widget that takes `&mut App`.

Draining after each blocking receive is why dragging a window edge costs one
re-layout per batch rather than one per event.

### State that is derived, never stored

- **The input mode** comes from what is open and what has focus. A mode stored
  alongside the state it describes is how a closed prompt ends up still
  swallowing keys.
- **The active outline entry** comes from the scroll position. It is never
  written into the contents cursor, which is where the *reader* put it.
  Collapsing those two is the single easiest way to make a contents pane feel
  broken: scrolling would drag the selection away mid-keystroke.
- **Pane geometry** comes from the terminal size and the state.

### Everything that points into `lines`

Scroll position, outline anchors, search hits, and link spans are all indices
into `RenderedDoc.lines`. A resize, a theme switch, a reload, or a `-w` change
invalidates every one of them at once.

`doc::cache::ensure_rendered` is therefore the only caller of the layout
engine. It snapshots where the reader is, re-lays out, remaps everything
together, and bumps a revision counter. Search hits and links re-derive
themselves when the revision changes. Nothing else may assign to `lines`.

The reading position is carried across by *source byte offset*, not line
number, so a narrower column does not teleport the reader. A reload is the
exception: an edit moves the text itself, so the *section* being read is
remembered instead — inserting a paragraph at the top would otherwise drop the
reader a section back.

### Input

A key never reaches any logic. It is resolved to an `Action` by the keymap
first, and only actions are matched on. That indirection is what makes the
configuration file's `[keys.*]` a data swap rather than a rewrite, and it is
why the help overlay can be generated from the bindings actually in force
instead of written out by hand.

A prompt binds almost nothing on purpose: any printable key it has not bound is
text. That is what keeps `q` typed into a search box from quitting the reader.

## Seams

Three traits exist so that tests need no filesystem and CI needs no network:

- `FsProbe` — classification asks whether a path is a directory.
- `Fetcher` — everything remote, with `FakeFetcher` beside it *in the library*
  rather than behind `#[cfg(test)]`, so integration tests can use it too.
- `EventSource` — the loop's input, with `ScriptedEvents` for headless runs.

Live checks against the real forges exist in `tests/network.rs` behind
`#[ignore]`, because a fake cannot notice an API changing shape.

## Things that bit, and what stops them recurring

Each of these cost a debugging session. They are here so the next one does not.

- **A background thread owning standard input means nothing may ask the
  terminal a question — except while that thread is standing down.**
  `Terminal::clear` snapshots the cursor position first, a round trip the event
  thread swallows the reply to, so it times out and fails. Forcing a redraw
  after an external program means resetting both ratatui buffers instead. The
  one window where a question is legal is inside `external::run`, which parks
  the reader through `app::gate` before the terminal changes hands; that is
  what lets the leftovers of the other program's session be read and thrown
  away. Moving the buffer reset inside that window as an optimization would
  put it back on the wrong side of the rule.
- **Two processes reading one terminal split the keystrokes between them.**
  The reader used to block in `event::read` for its whole life, including while
  an editor had the terminal — so the editor lost characters, and stalled
  waiting out timeouts on questions whose replies the reader had eaten. A
  thread parked in a blocking read cannot be asked to stop, which is why the
  reader waits with a timeout and parks at a gate instead. The handshake is the
  load-bearing part: `gate::pause` does not return until the reader has
  acknowledged from the top of its loop, the only point at which it is provably
  not inside a read. `scripts/handoff-check.py` reproduces the original defect.
- **Every terminal mode the reader depends on has to be asked for.** Bracketed
  paste was not, so `Event::Paste` never fired and a pasted newline was an
  ordinary Enter: it submitted the search prompt and left the rest of the paste
  being dispatched as bindings, where `q` quits. What the terminal is set to on
  the way in is whatever the last program left behind, and an editor launched
  with `e` leaves its own settings, not ours. `terminal::setup` and
  `terminal::teardown` are mirrors and a test holds them to it.
- **Pane geometry may not depend on anything only a layout can produce.** The
  contents pane first decided whether to show itself from the outline, which
  does not exist until the first layout — so it appeared on frame two, changed
  the content width, re-laid out, and moved the reader a line. It asks the
  heading count now, which is counted from the block tree at parse time.
- **Reordering a list invalidates every index into it.** The browser sorts by
  modification time, so a file arriving mid-scan inserts itself above the
  cursor. Sorting inside `extend` left the cursor pointing at a different
  document, silently. Sorting happens only in `refresh`, which rebuilds the
  indices and re-finds the selection by path in the same breath.
- **A watch matched on the whole path never fires for a relative argument.**
  The argument is relative and the events come back absolute.
- **Some bugs are only reachable by running the binary.** Piped-stdin
  detection, broken pipes, the watch, the terminal query — all invisible to
  hundreds of passing tests. The CI smoke job and a real terminal are the only
  things that find them.

## Where to start reading

- `src/render/sink.rs` if you want to understand the layout invariant.
- `src/app/mod.rs` for the loop, which is short.
- `tests/keyseq.rs` for what the reader is supposed to do, stated as key
  sequences and their outcomes.
