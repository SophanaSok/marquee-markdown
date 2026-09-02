# The README's animated demo, recorded with VHS.
#
#   https://github.com/charmbracelet/vhs
#
#   cargo build --release
#   vhs docs/demo.tape          # writes docs/demo.gif
#
# What this records, in order, is the case for the project over `glow`: the
# contents pane tracking the scroll, folding, search narrowing as you type,
# and the theme picker previewing against your own document. Rendering
# details photograph fine as a still — `scripts/screenshot.py` already does
# that better than a GIF can. Only the moving parts belong here.
#
# `--config /dev/null` for the same reason the comparison shot uses it: so no
# local configuration is recorded, and the demo is what a reader gets on a
# first run.

Output docs/demo.gif

Require mmd

Set Shell bash
Set FontSize 15
Set Width 1300
Set Height 760
Set Padding 12
Set Framerate 24
Set PlaybackSpeed 1.0
Set TypingSpeed 55ms
Set LoopOffset 5%

# Start on a clean prompt, with the release build on PATH.
Hide
Type "export PATH=$PWD/target/release:$PATH"  Enter
Type "clear"  Enter
Show

Type "mmd --config /dev/null -t docs/demo.md"
Sleep 600ms
Enter
Sleep 2s

# The contents pane tracks the scroll — the thing glow has no equivalent of.
Down@120ms 14
Sleep 1500ms
Down@120ms 12
Sleep 2s

# Folding: `tab` moves focus to the pane, `h` collapses a subtree.
Tab
Sleep 900ms
Down@200ms 2
Sleep 700ms
Type "h"
Sleep 1600ms
Type "l"
Sleep 1200ms
Tab
Sleep 700ms

# Search narrows as you type, and highlights in place without re-laying out.
#
# `enter` is not decoration: a prompt binds almost nothing, so every printable
# key reaches the text being typed. Without it the `n`s below land in the query
# — `cardnn`, the word this recorded before anyone watched it back — and the
# demo ends on "no matches". And the query has to be a word the document says
# more than once, or `n` walks a single hit and appears to do nothing.
Type "/"
Sleep 500ms
Type@140ms "the"
Sleep 1500ms
Enter
Sleep 800ms
Type "n"
Sleep 900ms
Type "n"
Sleep 1400ms
Escape
Sleep 900ms

# The theme picker previews against the document behind it.
Type "s"
Sleep 1200ms
Down@350ms 3
Sleep 1600ms
Down@350ms 2
Sleep 1800ms
Escape
Sleep 1200ms

Type "q"
Sleep 1500ms
