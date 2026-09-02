<h1 align="center">marquee-markdown</h1>

<p align="center">
  A terminal markdown reader with the functionality of
  <a href="https://github.com/charmbracelet/glow"><code>glow</code></a>,
  rendering documents the way Claude artifacts do —<br>
  a centered reading column on a painted page, typographic headings, sealed
  code cards — with a table-of-contents panel for navigation.
</p>

<p align="center">
  <a href="https://github.com/SophanaSok/marquee-markdown/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/SophanaSok/marquee-markdown/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://crates.io/crates/marquee-markdown"><img alt="crates.io" src="https://img.shields.io/crates/v/marquee-markdown"></a>
  <img alt="Rust 1.88+" src="https://img.shields.io/badge/rust-1.88%2B-b7410e">
  <a href="LICENSE"><img alt="MIT" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <img alt="Linux, macOS, Windows" src="https://img.shields.io/badge/platform-linux%20%7C%20macos%20%7C%20windows-lightgrey">
</p>

<p align="center">
  <img src="https://raw.githubusercontent.com/SophanaSok/marquee-markdown/main/docs/demo.gif" alt="The reader in motion: the contents pane tracking the scroll, a section folding and unfolding, a search narrowing as it is typed, and the theme picker previewing against the document behind it" width="100%">
</p>

<p align="center"><sub>
  A real terminal, not a mock-up: the contents pane tracking the scroll,
  folding, search, and the theme picker previewing against the document behind
  it. Recorded with <code>vhs docs/demo.tape</code>.
</sub></p>

> **Released and in use.** Everything documented here works today; the badge
> above is the current version. See [docs/ROADMAP.md](docs/ROADMAP.md) for what
> is planned before 1.0.

## What it does

- **Reads markdown properly.** Headings become typography rather than hash
  marks, code blocks become sealed cards, tables get box drawing, and GFM
  callouts get an icon and a hue.
- **Reads the HTML in a README too.** A centered `<h1>` becomes a heading in
  the contents pane, badge images become the links they advertise, `<br>`
  breaks a line, and a `<details>` block is shown open with its `<summary>` as
  the title — instead of the tags themselves reaching the page.
- **Math is notation, not punctuation.** `$E = mc^2$` reads as a code span
  rather than as a formula wearing its dollar signs. TeX is not typeset —
  there is no glyph budget for that in a cell grid — but the delimiters go.
- **A contents pane that tracks where you are**, with folding — the thing
  `glow` has no equivalent of, and the reason this exists.
- **Ten themes**, or your own, or your terminal's own colors.
- **Search inside a document** with `/`, `n` and `N`, highlighted in place.
- **A file browser** that streams as it walks, with a fuzzy filter.
- **Reads what you point it at**: a file, a directory, standard input, a URL,
  or `github.com/owner/repo`.
- **Reloads when you save**, and `e` opens your editor at the line on screen.
- **Everything is configurable and every key is rebindable**, from one TOML
  file.
- **Themes are data**, so a new palette needs no Rust and no recompile.

<p align="center">
  <img src="https://raw.githubusercontent.com/SophanaSok/marquee-markdown/main/docs/screenshot.svg" alt="marquee-markdown reading a document, with the contents pane on the left" width="100%">
</p>

<p align="center"><sub>
  The same reader held still, where the type is sharp enough to read —
  regenerate it with <code>python3 scripts/screenshot.py</code>.
</sub></p>

## Why not glow

`glow` is good, and this keeps its flags and its keys so muscle memory carries
over. But rendering the same document through glow 3.0.0 shows what this fixes:

| glow | marquee-markdown |
| --- | --- |
| `## Heading` — hash marks reach the output | typography: weight, color, rhythm, a hairline rule |
| `[!NOTE]` printed literally | an icon-and-title callout in its own hue |
| Tables with no outer frame, columns stretched to the full width | box drawing with a shaded header band, columns sized to content |
| Thematic breaks as `--------` | a hairline `─` across the column |
| Long code lines wrap *out* of the block | they stay sealed inside the card |
| Raw HTML in a README printed tag by tag | interpreted: the title joins the contents pane, badges read as links |
| Link targets printed inline, interrupting the sentence | link text reads as text; `]` walks them, `enter` opens |
| 2-space margin | a centered reading column, page painted edge to edge |
| No outline, no in-document search | a scroll-tracking contents pane, and `/` `n` `N` |
| Keys hardcoded, cannot be rebound | every key resolves through a rebindable action table |

The last two rows are the ones that made this worth building. The rest are
rendering details that add up.

<p align="center">
  <img src="https://raw.githubusercontent.com/SophanaSok/marquee-markdown/main/docs/compare-glow.svg" alt="The same document rendered by glow on the left and marquee-markdown on the right" width="100%">
</p>

<p align="center"><sub>The same document, the same 80-column terminal, both at their defaults and with no
configuration on either side. glow left, marquee-markdown right.</sub></p>

## Install

### With cargo

```sh
cargo install marquee-markdown
```

Needs Rust 1.88 or newer — the code uses let-chains, which that release
stabilized for the 2024 edition. Nothing else: syntax highlighting uses a
pure-Rust regex backend on purpose, so there is no C toolchain and no system
library to find.

That installs two commands: **`marquee-markdown`** and **`mmd`**, which are the
same program under a shorter name.

### Prebuilt

Each [GitHub release](https://github.com/SophanaSok/marquee-markdown/releases)
carries binaries for Linux, macOS (Intel and Apple Silicon) and Windows, plus
`.deb` and `.rpm` packages, with man pages and shell completions in the
archives and SHA-256 checksums alongside.

```sh
# Debian and derivatives
sudo dpkg -i marquee-markdown_*_amd64.deb

# Fedora and derivatives
sudo rpm -i marquee-markdown-*.x86_64.rpm
```

With [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall), one
command fetches the right archive and puts both binaries on your path — and
the same command upgrades them later:

```sh
cargo binstall marquee-markdown
```

### Homebrew

```sh
brew install SophanaSok/marquee/marquee-markdown
```

### Scoop

```powershell
scoop bucket add marquee https://github.com/SophanaSok/scoop-marquee
scoop install marquee-markdown
```

### Nix

Submitted to nixpkgs and in review
([NixOS/nixpkgs#558998](https://github.com/NixOS/nixpkgs/pull/558998)), so
`nix-shell -p marquee-markdown` is not a thing yet. Until it is, the derivation
builds from a checkout:

```sh
nix-build -E 'with import <nixpkgs> {}; callPackage ./packaging/nix/default.nix {}'
```

### Arch Linux

Not available, and not for want of a package. Both PKGBUILDs are written and
kept current in [`packaging/aur/`](packaging/aur/) — they build under `makepkg`
and pass `namcap` — but **the AUR has been closed to new accounts since 15 June
2026**, after a supply-chain attack that hijacked more than a thousand packages
to ship credential stealers. Package adoption is disabled too, and there is no
announced date for either reopening.

So there is nowhere to publish them to. Until that changes, `cargo install
marquee-markdown` or the [prebuilt archive](#prebuilt) above are the ways in on
Arch; both PKGBUILDs are ready to push the day it reopens.

### From source

```sh
git clone https://github.com/SophanaSok/marquee-markdown
cd marquee-markdown
cargo install --path .
```

### Fonts

Any monospace font works — the callout and image icons default to standard
Unicode glyphs (ⓘ ✦ ‼ ⚠ ✖ ▣). If you use a [Nerd
Font](https://www.nerdfonts.com/), an `[icons]` block in a theme file swaps in
its glyphs; see [Themes](#themes).

### Staying up to date

The reader checks crates.io for a newer release at most once a day, from a
detached background thread that never delays startup, rendering, or exit.
When one exists, the last line on the way out says so and names the upgrade
command. It stays quiet in scripts and builds: the notice only appears when
standard error is a terminal, and never when `CI` is set. Turn it off for
good with `update-check = false` in the configuration file, or
`MARQUEE_UPDATE_CHECK=0` in the environment.

## Contents

[Usage](#usage) · [Reading](#reading) · [Browsing](#browsing) ·
[Editing, reloading, copying](#editing-reloading-and-copying) ·
[Remote documents](#remote-documents) · [Configuration](#configuration) ·
[Themes](#themes) · [As a library](#using-the-renderer-as-a-library) ·
[Contributing](#contributing)

## Usage

Everything below works with `mmd` too — it is the same program under a shorter
name, which is the one you will actually type.

```sh
mmd                                 # browse the markdown here
mmd README.md                       # render a file
mmd -t README.md                    # ...in the full-screen reader

marquee-markdown                    # browse the markdown here
marquee-markdown README.md          # render a file
marquee-markdown docs/              # render a directory's README
marquee-markdown -                  # read standard input
cat notes.md | marquee-markdown     # same
marquee-markdown src/main.rs        # source files render as highlighted code

marquee-markdown github.com/charmbracelet/glow   # a repository's README
marquee-markdown gitlab://gitlab-org/gitlab      # the same, spelled as a scheme
marquee-markdown https://example.com/doc.md      # any URL

marquee-markdown -t doc.md          # full-screen reader
marquee-markdown -w 80 doc.md       # fixed width (0 disables wrapping)
marquee-markdown -s paper doc.md    # light theme
marquee-markdown -s system doc.md   # your terminal's own colors
marquee-markdown -l doc.md          # line numbers

marquee-markdown -p doc.md          # through your pager
marquee-markdown themes             # list available themes
marquee-markdown config             # the settings in force
marquee-markdown keys               # every key binding
marquee-markdown man                # man page to stdout
marquee-markdown completion fish    # shell completions
```

Standard input is read when nothing else is named, or when `-` asks for it. A
named source always wins — unlike glow, where a redirected stdin silently
replaces the file you asked for, which is fine when you typed the pipe and
wrong in a cron job.

Output degrades on its own: piping or redirecting drops color, gutters, and
hyperlinks, so `marquee-markdown doc.md > out.txt` contains just the text, and
closing the pipe early (`… | head`) stops quietly rather than erroring.
`NO_COLOR` is honored, `TERM=dumb` gets plain text, and `CLICOLOR_FORCE=1`
(or `FORCE_COLOR=1`) forces color back on for a pipe, so
`marquee-markdown doc.md | less -R` keeps its color.

Every flag `glow` takes is accepted and means the same thing:
`-a -l -m -n -p -s -t -w`.

## Reading

`-t` opens the full-screen reader: the document in a centered column, a
table-of-contents pane beside it, and a status bar that says where you are.
Keys follow glow, so muscle memory carries over; `?` shows the list, rendered
from the keymap that is actually in force rather than from a fixed page, so it
stays honest once keys become rebindable.

The wheel moves whichever pane has the keys — the document, the contents pane,
the file list, the key reference — three steps a tick, on every terminal. This is
the one place the reader does not follow glow, where the wheel is opt-in: a
terminal nobody claimed the wheel from answers it by manufacturing arrow keys,
multiplied by its own scroll factor, and those arrive as ordinary keystrokes —
so a stray touchpad brush yanks the document away from someone reading it with
the keyboard. Claiming the wheel is what stops that. Selecting text with the
mouse needs `shift` held while the reader is open, as it does in `less
--mouse`; `--no-mouse`, or `mouse = false` in the configuration file, hands the
wheel back.

A hint line above the status bar names the handful of keys worth knowing —
scroll, search, contents, help, quit — so the reader is learnable without
having to guess that `?` exists. It is rendered from the keymap in force, like
the reference itself, so it never advertises a key you rebound, and it says
what the pane you are in can do: the contents pane offers folding, a prompt
offers the way out of it. As the terminal narrows it drops hints from the end
rather than wrapping, and a terminal too narrow for even the first one spends
the row on the document instead.

It is on by default, because the keys are the part of a full-screen reader that
nothing else announces, and the row it costs is one row. `H` takes it back for
the session; `hints = false` under `[ui]` takes it back for good.

Keys are written here the way a configuration file will spell them.

| Key | |
| --- | --- |
| `j` `k` · `down` `up` | line down, line up |
| `d` `u` · `ctrl+d` `ctrl+u` | half page down, half page up |
| `f` `b` · `space` `pgdn` `pgup` | page down, page up |
| `g` `G` · `home` `end` | top, bottom |
| `h` `l` · `left` `right` | scroll sideways (only with `-w 0`) |
| `/` | search |
| `n` `N` | next hit, previous hit |
| `]` `[` | next link, previous link |
| `enter` | follow the selected link |
| `y` | copy the selected link |
| `c` | copy the document |
| `e` | edit, at the line on screen |
| `r` | reload from disk |
| `R` | re-read the terminal's colors |
| `t` | show / hide the contents pane |
| `tab` | move focus between the panes |
| `T` | switch light / dark |
| `s` | choose a theme |
| `?` | key reference |
| `H` | show / hide the hint line |
| `ctrl+z` | suspend to the shell (unix) |
| `esc` | close what is open |
| `q` · `ctrl+c` | quit |

The contents pane takes the same movement keys, pointed at itself. `h` and `l`
fold and unfold there, which is what they mean in any tree; in the document
they still scroll sideways.

| Key | |
| --- | --- |
| `j` `k` · `down` `up` | next entry, previous entry |
| `g` `G` · `home` `end` | first entry, last entry |
| `h` `l` · `left` `right` | fold, unfold |
| `enter` | go to the entry |
| `tab` · `esc` | back to the document |
| `H` | show / hide the hint line |

The pane highlights two different things and they are deliberately not the
same: the **active** entry is the section the document is scrolled to, and it
follows you as you read, while the **cursor** is where you left it. Scrolling
never drags the cursor away mid-keystroke. The pane hides itself on a narrow
terminal, and on a document with fewer than two headings, where it would cost
a quarter of the screen to say nothing.

<p align="center">
  <img src="https://raw.githubusercontent.com/SophanaSok/marquee-markdown/main/docs/screenshot-search.svg" alt="Searching a document: matches highlighted in place, the current one accented, the count in the status bar" width="100%">
</p>

<p align="center"><sub><code>/</code> narrows as you type; <code>n</code> and <code>N</code> walk the matches.</sub></p>

## Browsing

Run `marquee-markdown` with no argument, or point it at a directory, and it
lists the markdown under it — most recently edited first, with hidden and
git-ignored files left out unless `-a` says otherwise. The walk streams, so the
first screenful is there immediately and a large tree fills in behind it.

| Key | |
| --- | --- |
| `j` `k` · `down` `up` | next file, previous file |
| `f` `d` · `l` `right` `pgdn` | next page |
| `b` `u` · `h` `left` `pgup` | previous page |
| `g` `G` · `home` `end` | first file, last file |
| `enter` | read this file |
| `/` | filter the list |
| `r` | rescan the directory |
| `R` | re-read the terminal's colors |
| `.` | show / hide hidden and ignored files |
| `T` `s` | switch light / dark, choose a theme |
| `H` | show / hide the hint line |
| `esc` | clear the filter |
| `q` · `ctrl+c` | quit |

`esc` in a document goes back to the list, so the browser is where a reading
session lives rather than somewhere you pass through once. The list is a
snapshot; `r` walks the directory again — keeping your filter and, when the
file still exists, your place — and `.` widens or narrows the walk to hidden
and git-ignored files on the spot.

Two of those keys are inconsistent with the document: `f`/`d` page a whole
screen here and half a screen when reading, and `h`/`l` page here but scroll
sideways there. That is glow's behavior, reproduced deliberately — anyone with
the muscle memory would find a silent correction more surprising than the
quirk, and rebindable keys are the fix glow cannot offer.

The filter is fuzzy and runs as you type: `rdmp` finds `docs/ROADMAP.md`. Both
the query and the file names are normalized first, so a name written with
combining marks still matches one typed with precomposed characters — a file
you can see should never be a file you cannot find.

Search runs over the rendered text, so a hit is already a place on the page and
highlighting costs nothing. The matches narrow as you type — the count in the
status bar is the feedback — and `enter` commits the query and jumps to the
first hit; abandoning the prompt with `esc` brings the previous highlight
back. A phrase broken across a soft wrap matches, with the highlight split
across both lines; markers and gutter bars are decoration, not text, and never
match. A lowercase query ignores case; a query with any capital in it does
not. `esc` clears the highlight.

<p align="center">
  <img src="https://raw.githubusercontent.com/SophanaSok/marquee-markdown/main/docs/screenshot-browser.svg" alt="The file browser listing markdown files found under the current directory, newest first" width="100%">
</p>

<p align="center"><sub>Run <code>mmd</code> with a directory, or with nothing at all, to browse.</sub></p>

## Editing, reloading, and copying

The open document is watched, so saving it in another window re-renders it
where you are — the *section* you were reading, not the line number, which an
edit above you would have moved. `r` reloads by hand if a filesystem does not
report changes.

`e` opens the document in `$VISUAL`, `$EDITOR`, or `vi`, at the line on screen,
and reloads when the editor exits. Line arguments are spelled the way each
editor wants them; an editor that is not recognized is handed the path alone
rather than a flag it would take for a second filename.

A link to a heading in the same document (`[see below](#section)`) scrolls
there rather than handing the fragment to your browser — the contents pane
already knows where every heading is. Links out are resolved against wherever
the document came from, including root-relative ones and `..`.

`c` copies the markdown as written, not as rendered — what you want to paste
elsewhere is the source. `y` copies the address of the selected link. Both go
through the terminal (OSC 52) before the system clipboard, so copying works
over SSH: the text lands on the machine you are sitting at rather than on the
server. In tmux this needs `set -g set-clipboard on`.

Resizing the terminal or switching theme re-lays out the document and keeps
your place: the position is carried by source offset, not by line number, so a
narrower column does not teleport you. Search hits are re-found at the same
time, so the highlight never points at a stale line.

## Remote documents

A repository shorthand resolves through the forge's API to the raw README, so
what you get is the file rather than a screenful of the page around it:

```sh
marquee-markdown github.com/charmbracelet/glow
marquee-markdown github://charmbracelet/glow      # same thing
marquee-markdown gitlab://gitlab-org/gitlab
marquee-markdown https://example.com/notes/guide.md
```

The extension in a URL is trusted ahead of the `Content-Type` header, because
plenty of servers hand out markdown as `text/plain` or even `text/html`, and a
`.md` in the path is a stronger statement of intent than a header nobody
thought about. A URL with no extension served as HTML is shown as highlighted
markup rather than run through the markdown renderer, which would only produce
noise.

Fetched documents are capped at 8 MiB and time out after 20 seconds, so a
mistyped URL cannot leave you with an unresponsive terminal.

## Configuration

Nothing needs configuring, but everything can be. The file lives at
`~/.config/marquee-markdown/config.toml`; `--config` or `MARQUEE_CONFIG` names
a different one.

```toml
[general]
style = "paper"            # paper | slate | system | a name | a path
width = 80                 # 0 disables wrapping
line-numbers = false
mouse = true               # the reader takes the wheel; shift to select text
all = false                # list hidden and ignored files when browsing
preserve-new-lines = false
update-check = true        # mention a newer release on the way out
terminal-query = true      # let `--style system` ask the terminal its colors

[theme]
# Paths whose change means the terminal may have been retinted, for
# `--style system`. Only needed when your desktop retints a window that
# never loses focus — regaining focus is already a trigger. `~` expands.
watch = ["~/.local/state/omarchy/current/theme"]

[render]
html = "render"            # render | hide | literal

[ui]
contents = true            # start with the contents pane showing
hints = true               # start with the hint line above the status bar

[keys.document]
"ctrl+n" = "line-down"     # rebind
"q" = "none"               # or take a key away
```

Settings resolve in one order, everywhere: **command line, then environment,
then file, then defaults**. A flag that was not given contributes nothing, so
`mouse = true` in your config is not undone by every invocation that omits
`-m`.

Environment variables are the setting name in `MARQUEE_` form, with
`[general]` left out: `MARQUEE_STYLE`, `MARQUEE_WIDTH`, `MARQUEE_LINE_NUMBERS`,
`MARQUEE_MOUSE`, `MARQUEE_ALL`, `MARQUEE_PRESERVE_NEW_LINES`,
`MARQUEE_UPDATE_CHECK`, `MARQUEE_TERMINAL_QUERY`, `MARQUEE_RENDER_HTML`,
`MARQUEE_UI_CONTENTS`, and `MARQUEE_UI_HINTS`.

A setting this version does not recognize is reported and ignored rather than
refused, so a file written for a newer version still works with an older
binary. The same goes for a key or an action it does not know: one typo costs
one key, not the keymap.

Two commands make all of this inspectable:

```sh
marquee-markdown config    # the settings in force, as a file that would produce them
marquee-markdown keys      # every binding, as markdown
```

`config` output round-trips: save it as your configuration file and you get the
same settings back. Every action name it prints is listed in
[docs/KEYBINDINGS.md](docs/KEYBINDINGS.md), which is generated from the
keymap rather than written by hand.

## Themes

Two palettes are the reader's own — `paper` (light) and `slate` (dark), and
`slate` is the default. `--style auto` is accepted too, because `glow` spells
it that way and its flags carry over here; it is an alias for `slate` rather
than an adaptive choice, despite the name.

Eight ports of established colorschemes ship alongside them:

```sh
mmd -s catppuccin-mocha doc.md
```

| light | dark |
| --- | --- |
| `catppuccin-latte` | `catppuccin-mocha` |
| `solarized-light` | `solarized-dark` |
| | `dracula` |
| | `gruvbox-dark` |
| | `nord` |
| | `tokyo-night` |

These are TOML files in [`themes/`](themes), not Rust — the same kind of file
you would write yourself, parsed by the same code. A theme of your own in
`~/.config/marquee-markdown/themes/` with the same name wins, so retuning one
for your terminal is a copy and an edit. [`docs/THEMES.md`](docs/THEMES.md)
has the schema, the seven syntax themes a palette can pair with, and what a
theme PR needs.

The adaptive one is `--style system`, which builds the whole palette out of
the colors your terminal is already using, so a document reads in your own
colorscheme rather than in Claude's:

```sh
marquee-markdown -s system doc.md
```

It asks the terminal directly — `OSC 10`, `OSC 11` and `OSC 4`, the same
questions any terminal program asks — and takes the page and the text
verbatim. The rest is derived: cards and borders step off the page, and
headings, links and callouts come from the ANSI slots, each held to a contrast
floor so a light scheme's yellow does not become an unreadable heading. On
Solarized Light that floor is what picks the red over the yellow; without it
the heading would sit at a ratio of 2.0 on its own page.

Only `system` asks. Every other style — the default included — sends the
terminal nothing at all. Anything that will not answer
falls back to a shipped palette, and costs nothing to try:

| where | `system` gets | asked for |
| --- | --- | --- |
| a terminal that answers | its own colors | nothing measurable |
| **tmux** | falls back — tmux answers the device query and nothing else | nothing measurable |
| `screen`, or a reply slower than 100 ms | falls back | 100 ms, once |
| Windows | falls back — the replies arrive there by another road | nothing |

`terminal-query = false` stops it being asked at all, and nothing but
`-s system` ever asked in the first place.

#### Following your terminal while you read

`system` keeps up. Change your terminal's colorscheme — or your desktop's
theme, which changes your terminal's — and the page is repainted in the new
palette without your touching anything.

Four things can prompt it, and you need none of them set up for the common
case:

| trigger | needs | notices |
| --- | --- | --- |
| **coming back to the window** | nothing | a theme changed while you were elsewhere, which is nearly always |
| **`R`** | nothing | whenever you ask |
| **a watched path** | one line of config | a theme changed while this window kept focus |
| **`SIGUSR1`** | a hook that sends it | the same, exactly when the retint has finished |

Coming back to the window is the one that does the work. A theme is almost
always changed from somewhere else — a picker, a hotkey, another window — so
regaining focus is both the moment the answer has settled and a moment you
were going to have anyway.

If your desktop retints a terminal that never loses focus, name the file it
touches:

```toml
[theme]
watch = ["~/.local/state/omarchy/current/theme"]
```

For an exact trigger with no race in it, have your desktop's theme hook send
the signal instead — it runs after the terminals have been retinted, so the
colors are already the new ones when the question is asked:

```sh
# ~/.config/omarchy/hooks/theme-set.d/reload-marquee
pkill -USR1 -x marquee-markdow   # not a typo: `pkill -x` caps names at 15
pkill -USR1 -x mmd
```

`packaging/omarchy/` has that hook ready to copy.

**It costs nothing while nothing changes**, which matters because focus is
regained far more often than a theme is switched:

- A terminal that answered nothing when first asked is never asked again — so
  `screen`, a dumb terminal and every Windows console pay nothing at all,
  forever, rather than a timeout per trigger.
- A trigger asks for the background alone — two escape sequences, not
  nineteen. Only a background that actually moved pays for the full palette.
- Triggers that arrive together are one question. A theme switch usually
  arrives twice, as a watched path *and* a regained focus.

Following stops the moment you choose a palette by hand: `T` and the theme
picker both mean *this one*, not *keep following*. Choosing `system` in the
picker starts it again. A theme file follows the same rule — edit
`~/.config/marquee-markdown/themes/mine.toml` while reading with `-s mine`,
and `R` shows you the change.

Adding one needs no Rust and no recompile:

```sh
mkdir -p ~/.config/marquee-markdown/themes
$EDITOR ~/.config/marquee-markdown/themes/mine.toml
marquee-markdown -s mine doc.md
# or, without installing it:
marquee-markdown -s ./mine.toml doc.md
```

```toml
name = "mine"
appearance = "dark"       # light | dark
syntax = "base16-eighties.dark"

[palette]
bg = "#262624"            # page, painted edge to edge
surface = "#1f1e1d"       # code cards, inline chips, table headers
fg = "#f5f4ef"
muted = "#87867f"
accent = "#d97757"
accent_soft = "#d4a27f"
border = "#3d3d3a"

[palette.alerts]
note = "#7ea3c4"
tip = "#85b085"
important = "#b094c0"
warning = "#d9a441"
caution = "#dd7a6d"

[icons]                   # optional — these are the defaults, any font draws them
note = "ⓘ"
tip = "✦"
important = "‼"
warning = "⚠"
caution = "✖"
image = "▣"               # placeholder in front of an image's alt text
```

Icons are part of the theme because glyph choice is a font question. With a
[Nerd Font](https://www.nerdfonts.com/), this block swaps in its icons:

```toml
[icons]
note = ""
tip = ""
important = ""
warning = ""
caution = ""
image = ""
```

`marquee-markdown themes` lists what is available and where each came from —
built-ins, your own files, and `system`.

In the reader, `s` opens the same list to pick from. The document behind it
redraws as you move, so you are choosing by looking at your own text rather
than at a swatch, and `enter` keeps what you are looking at — writing it to
your configuration file so the next run starts there. `esc` puts back the
theme you opened with. Only the `style` line is rewritten: comments, key
order, and every other setting in the file survive.

`T` still flips between light and dark, which is the faster gesture when you
only ever use two.

<p align="center">
  <img src="https://raw.githubusercontent.com/SophanaSok/marquee-markdown/main/docs/screenshot-paper.svg" alt="The same document in the paper theme, dark text on a light page" width="100%">
</p>

<p align="center"><sub>The same document in <code>paper</code>. Themes are data — a new palette needs no recompile.</sub></p>

## Using the renderer as a library

`src/render/` is a standalone engine with no dependency on the application
shell — a test enforces that — so it can be used to build other frontends.

```rust
use marquee_markdown::render::{self, Document, LayoutOptions};
use marquee_markdown::theme::{Theme, ThemeVariant};

let options = LayoutOptions {
    width: 80,
    code_line_numbers: false,
    preserve_new_lines: false,
};
let theme = Theme::new(ThemeVariant::Slate);

// One call, when you only need the document once.
let doc = render::render("# Title\n\nSome prose.", &theme, options);
assert_eq!(doc.outline[0].text, "Title");
assert!(doc.lines.iter().all(|line| line.width() == 80));

// Or parse once and lay out repeatedly — what a reader that resizes wants.
// Parsing is the expensive half and does not depend on the width.
let parsed = Document::parse("# Title\n\nSome prose.");
for width in [40, 80, 120] {
    let doc = parsed.layout(&theme, LayoutOptions { width, ..options });
    assert_eq!(doc.width, width);
}
```

A `RenderedDoc` is a buffer of `ratatui` lines, each exactly the content width,
plus the outline, the links with their column ranges, per-line source offsets,
and a plain-text mirror for searching. Two serializers take it from there:
`render::tui` writes it into a `ratatui` buffer, and `render::ansi` writes SGR
bytes with real OSC 8 hyperlinks.

### What is stable

The promised API is deliberately small, and from 1.0 it follows semantic
versioning:

- `render::{render, render_with, Document, RenderedDoc, LineMeta, LineKind, Anchor}`
- `render::{LayoutOptions, ParseOptions, HtmlMode}`
- `render::{ansi, tui, overlay, measure}`
- all of `theme`

The pipeline behind them — parsing, fragmentation, wrapping, the block tree,
the per-block emitters — stays public so the binary and the tests can reach it,
and because it is worth reading, but it is marked `#[doc(hidden)]` and may
change in any release. `Document` is opaque for exactly this reason: it is the
part of the pipeline a consumer genuinely needs, without freezing the shape of
what is behind it.

If you find yourself reaching for a hidden module, please open an issue — it
means the stable surface is missing something.

## Contributing

Pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) is the short
version; [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains how the code is
shaped and why, including the invariants that several tests exist to enforce.

- [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md) — every binding, generated from
  the keymap
- [docs/ROADMAP.md](docs/ROADMAP.md) — what is built and what is left
- [AGENTS.md](AGENTS.md) — the same ground as the architecture document, in
  working-notes form
- [CHANGELOG.md](CHANGELOG.md)

Requires Rust 1.88. Nothing else: syntax highlighting uses a pure-Rust regex
backend on purpose, so there is no C toolchain and no system library to find.

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all

# render a file without installing, to look at the output
cargo run --example preview -- tests/fixtures/kitchen-sink.md 80 slate
```

## License

MIT. See [LICENSE](LICENSE).
