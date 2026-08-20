#!/usr/bin/env python3
"""Turn real terminal output into the SVGs the README shows.

Runs a program on a pseudo-terminal, interprets what it writes into a grid of
styled cells, and draws that grid as an SVG. The point is that every picture in
the README is actual output rather than a mock-up, and that anyone can
regenerate it:

    python3 scripts/screenshot.py --all --strict     # every image
    python3 scripts/screenshot.py --shot search      # just one
    python3 scripts/screenshot.py --self-test        # just the parser

Only the escape sequences the two programs actually emit are handled — cursor
positioning, SGR colour and attributes, and screen clearing. This is not a
terminal emulator and does not try to be. `--self-test` pins the sequences it
does handle, which is the cheap way to notice when that stops being true.

Two things are worth knowing before changing anything here.

**The comparison is enforced, not promised.** `--shot compare` renders one
document through glow and through this program, and the terms that make that
fair — same document, same width, no configuration on either side, no cropping
— are conditions in the code rather than claims in the caption. A caption rots
silently; an assertion does not.

**One substitution is made, and only in the picture:** the reader draws callout
and image icons with Nerd Font glyphs from the private use area, which a
browser has no font for and would show as empty boxes. They are swapped for the
nearest standard character so the image looks like what a reader with a Nerd
Font actually sees, rather than like a rendering fault.
"""

from __future__ import annotations

import os
import pty
import re
import select
import subprocess
import sys
import time
import unicodedata
from dataclasses import dataclass, field, replace

ROWS, COLS = 30, 100
# Roughly the metrics of a typical terminal monospace font at 14px.
CELL_W, CELL_H, FONT_SIZE, BASELINE = 8.4, 18.0, 14.0, 13.5
FONTS = "ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, 'DejaVu Sans Mono', monospace"


@dataclass(frozen=True)
class Style:
    fg: str | None = None
    bg: str | None = None
    bold: bool = False
    italic: bool = False
    underline: bool = False
    strike: bool = False


@dataclass(frozen=True)
class Palette:
    """What an *unset* colour means.

    A terminal cell with no colour of its own inherits the terminal's defaults,
    so the picture has to know what those defaults were. Getting this wrong is
    invisible in one program and glaring in another: a renderer that paints its
    own page background looks fine either way, while one that paints nothing
    ends up with its code blocks dissolving into a page of the same colour.
    """

    fg: str
    bg: str


SLATE = Palette(fg="#f5f4ef", bg="#262624")
PAPER = Palette(fg="#141413", bg="#faf9f5")
# glow paints no page background at all, so this is our terminal, not its
# choice. xterm 252 is the grey it paints body text with anyway. The background
# is a near-black rather than #000 deliberately: pure black would flatter
# marquee by contrast, and a comparison that needs a thumb on the scale is not
# worth publishing.
TERMINAL = Palette(fg="#d0d0d0", bg="#1e1e1e")


@dataclass
class Cell:
    char: str = " "
    style: Style = field(default_factory=Style)


# Nerd Font private-use glyphs, and what stands in for them in the picture.
PUA_SUBSTITUTES = {
    "\uf05a": "\u24d8",  # note        →  circled i
    "\uf0eb": "\u25c9",  # tip         →  fisheye
    "\uf06a": "\u2757",  # important   →  heavy exclamation
    "\uf071": "\u25b2",  # warning     →  triangle
    "\uf057": "\u2716",  # caution     →  heavy multiplication
    "\uf03e": "\u25a3",  # image       →  framed square
}


def substitute(char: str) -> str:
    """Swap a private-use glyph for something a browser can draw."""
    if char in PUA_SUBSTITUTES:
        return PUA_SUBSTITUTES[char]
    return "\ufffd" if "\ue000" <= char <= "\uf8ff" else char


def wide(char: str) -> bool:
    return unicodedata.east_asian_width(char) in ("W", "F")


# SGR codes that switch a text attribute on or off.
ATTRIBUTES = {
    3: ("italic", True),
    23: ("italic", False),
    4: ("underline", True),
    24: ("underline", False),
    9: ("strike", True),
    29: ("strike", False),
}


def xterm_palette() -> list[str]:
    """The 256 colours an index refers to.

    0-15 are the usual ANSI sixteen, 16-231 a 6x6x6 cube, 232-255 a grey ramp.
    """
    base = [
        (0, 0, 0), (205, 0, 0), (0, 205, 0), (205, 205, 0),
        (0, 0, 238), (205, 0, 205), (0, 205, 205), (229, 229, 229),
        (127, 127, 127), (255, 0, 0), (0, 255, 0), (255, 255, 0),
        (92, 92, 255), (255, 0, 255), (0, 255, 255), (255, 255, 255),
    ]
    steps = [0, 95, 135, 175, 215, 255]
    cube = [(steps[r], steps[g], steps[b]) for r in range(6) for g in range(6) for b in range(6)]
    greys = [(8 + 10 * i,) * 3 for i in range(24)]
    return [f"#{r:02x}{g:02x}{b:02x}" for r, g, b in base + cube + greys]


XTERM = xterm_palette()


def extended(params: list[int], index: int) -> tuple[str | None, int]:
    """Read a 38/48 extended colour, returning it and how many params it ate.

    A `consumed` of zero means the sequence was malformed and nothing should
    change — which is why this returns a count rather than just a colour. The
    distinction matters: `38;5;39` handled sloppily falls through to its own
    tail and hits 39, *reset foreground*, so a program that colours with the
    256-colour palette comes out not merely grey but scrambled.
    """
    kind = params[index + 1] if index + 1 < len(params) else None
    if kind == 5 and index + 2 < len(params):
        return XTERM[params[index + 2] % 256], 2
    if kind == 2 and index + 4 < len(params):
        r, g, b = params[index + 2 : index + 5]
        return f"#{r:02x}{g:02x}{b:02x}", 4
    return None, 0


class Screen:
    """Just enough of a terminal to reconstruct one frame."""

    def __init__(self, rows: int, cols: int, grow: bool = False) -> None:
        self.rows, self.cols = rows, cols
        self.grow = grow
        self.grid = [[Cell() for _ in range(cols)] for _ in range(rows)]
        self.row = self.col = 0
        self.style = Style()
        # Characters that fell off the right edge. A program writing wider than
        # the pty it was given means the capture is wrong, not the program.
        self.dropped = 0

    def blank(self) -> list[Cell]:
        return [Cell() for _ in range(self.cols)]

    def reachable(self) -> bool:
        """Make the cursor's row exist, extending the grid if allowed."""
        while self.grow and self.row >= self.rows:
            self.grid.append(self.blank())
            self.rows += 1
        return self.row < self.rows

    def put(self, char: str) -> None:
        if not self.reachable():
            return
        if self.col >= self.cols:
            self.dropped += 1
            return
        self.grid[self.row][self.col] = Cell(char, self.style)
        self.col += 1
        if wide(char) and self.col < self.cols:
            # The second half of a wide glyph is covered by the first.
            self.grid[self.row][self.col] = Cell("", self.style)
            self.col += 1

    def trim(self) -> None:
        """Drop trailing rows with nothing on them."""
        while len(self.grid) > 1 and all(
            cell.char in (" ", "") and not cell.style.bg for cell in self.grid[-1]
        ):
            self.grid.pop()
        self.rows = len(self.grid)

    def sgr(self, params: list[int]) -> None:
        index = 0
        while index < len(params):
            code = params[index]
            if code == 0:
                self.style = Style()
            elif code == 1:
                self.style = replace(self.style, bold=True)
            elif code == 22:
                self.style = replace(self.style, bold=False)
            elif code in ATTRIBUTES:
                name, on = ATTRIBUTES[code]
                self.style = replace(self.style, **{name: on})
            elif code == 39:
                self.style = replace(self.style, fg=None)
            elif code == 49:
                self.style = replace(self.style, bg=None)
            elif code in (38, 48):
                colour, consumed = extended(params, index)
                if consumed:
                    key = "fg" if code == 38 else "bg"
                    self.style = replace(self.style, **{key: colour})
                    index += consumed
            elif 30 <= code <= 37:
                self.style = replace(self.style, fg=XTERM[code - 30])
            elif 90 <= code <= 97:
                self.style = replace(self.style, fg=XTERM[code - 90 + 8])
            elif 40 <= code <= 47:
                self.style = replace(self.style, bg=XTERM[code - 40])
            elif 100 <= code <= 107:
                self.style = replace(self.style, bg=XTERM[code - 100 + 8])
            index += 1


CSI = re.compile(r"\x1b\[([0-9;:?]*)([A-Za-z])")
OSC = re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")


def feed(screen: Screen, data: str) -> None:
    data = OSC.sub("", data)
    index = 0
    while index < len(data):
        char = data[index]
        if char == "\x1b":
            match = CSI.match(data, index)
            if not match:
                index += 1
                continue
            raw, final = match.group(1), match.group(2)
            params = [int(p) if p.isdigit() else 0 for p in raw.replace(":", ";").split(";")]
            if final in "Hf":
                screen.row = (params[0] if params else 1) - 1
                screen.col = (params[1] if len(params) > 1 else 1) - 1
            elif final == "m":
                screen.sgr(params or [0])
            elif final == "J":
                screen.grid = [screen.blank() for _ in range(screen.rows)]
            index = match.end()
        elif char == "\n":
            screen.row, screen.col = screen.row + 1, 0
            index += 1
        elif char == "\r":
            screen.col = 0
            index += 1
        else:
            screen.put(char)
            index += 1


# A gap this long in the output means a whole frame has landed: a TUI writes
# one frame per flush, so silence is a frame boundary rather than a pause
# mid-paint. Waiting for it is what makes typing into the program reliable —
# by the first gap the terminal is in raw mode and no longer echoes.
IDLE = 0.25
LIMIT = 30.0  # hard cap, so a program that never exits cannot hang the script


def child_env(extra: dict[str, str] | None = None) -> dict[str, str]:
    """A neutral environment for the program under the camera.

    Allow-listed rather than filtered: a picture of a renderer should show the
    renderer, not the photographer's dotfiles. Anything that could recolour or
    rewrap the output — NO_COLOR, CLICOLOR, MARQUEE_*, GLOW_* — is absent
    because it was never copied across.
    """
    keep = ("PATH", "HOME", "LANG", "LC_ALL", "LC_CTYPE", "USER", "TMPDIR")
    env = {name: value for name, value in os.environ.items() if name in keep}
    env["TERM"] = "xterm-256color"
    env["COLORTERM"] = "truecolor"
    env.update(extra or {})
    return env


def run(
    command: list[str],
    rows: int,
    cols: int,
    *,
    settle: float = 1.8,
    keys: str = "",
    oneshot: bool = False,
) -> tuple[str, str]:
    """Run `command` on a pseudo-terminal of a given size.

    Returns what it wrote to the terminal and, separately, what it wrote to
    standard error — separately because on a shared pty a stray log line lands
    in the picture.
    """
    import fcntl
    import struct
    import tempfile
    import termios

    primary, secondary = pty.openpty()
    fcntl.ioctl(secondary, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    errors = tempfile.TemporaryFile()
    process = subprocess.Popen(
        command,
        stdin=secondary,
        stdout=secondary,
        stderr=errors,
        close_fds=True,
        env=child_env(),
    )
    os.close(secondary)

    out: list[str] = []
    typed = not keys
    last = start = time.time()
    # One-shot output ends by itself; a full-screen program has to be waited out.
    deadline = None if oneshot else start + settle
    while time.time() - start < LIMIT:
        if deadline is not None and time.time() >= deadline:
            break
        ready, _, _ = select.select([primary], [], [], 0.1)
        if ready:
            try:
                chunk = os.read(primary, 65536)
            except OSError:
                break
            if not chunk:
                break
            out.append(chunk.decode("utf-8", "replace"))
            last = time.time()
        elif out and not typed and time.time() - last >= IDLE:
            os.write(primary, keys.encode())
            typed = True
            deadline = time.time() + settle

    if not oneshot:
        os.write(primary, b"q")
        time.sleep(0.3)
        process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
    os.close(primary)
    errors.seek(0)
    return "".join(out), errors.read().decode("utf-8", "replace")


@dataclass
class Shot:
    """One capture, ready to draw."""

    screen: Screen
    palette: Palette
    page: str
    caption: str = ""
    argv: str = ""


def take(
    command: list[str],
    palette: Palette,
    rows: int,
    cols: int,
    *,
    settle: float = 1.8,
    keys: str = "",
    oneshot: bool = False,
    strict: bool = False,
) -> Shot:
    """Run a command and turn what it drew into a Shot."""
    output, errors = run(command, rows, cols, settle=settle, keys=keys, oneshot=oneshot)
    if errors.strip():
        message = f"{command[0]} wrote to stderr: {errors.strip()[:200]}"
        if strict:
            raise SystemExit(f"error: {message}")
        print(f"warning: {message}", file=sys.stderr)

    screen = Screen(rows, cols, grow=oneshot)
    feed(screen, output)
    if oneshot:
        screen.trim()
    if screen.dropped:
        message = f"{screen.dropped} characters ran past column {cols}"
        if strict:
            raise SystemExit(f"error: {message} — the capture is too narrow")
        print(f"warning: {message}", file=sys.stderr)

    return Shot(screen=screen, palette=palette, page=page_colour(screen, palette))


def same_run(a: Style, b: Style) -> bool:
    """Whether two cells can share one <text> element.

    Background is drawn separately, so it does not split a run.
    """
    return (a.fg, a.bold, a.italic, a.underline, a.strike) == (
        b.fg,
        b.bold,
        b.italic,
        b.underline,
        b.strike,
    )


def attributes(style: Style) -> str:
    """The SVG attributes for a run's weight, slant and decoration."""
    out = ""
    if style.bold:
        out += ' font-weight="bold"'
    if style.italic:
        out += ' font-style="italic"'
    decoration = " ".join(
        name
        for flag, name in ((style.underline, "underline"), (style.strike, "line-through"))
        if flag
    )
    if decoration:
        out += f' text-decoration="{decoration}"'
    return out


def escape(text: str) -> str:
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def page_colour(screen: Screen, palette: Palette) -> str:
    """The colour most of the frame is painted with.

    Unset cells count as the palette default. Skipping them lets a modal *set*
    colour win the vote on a frame that is mostly bare, which then paints the
    whole page that colour and makes the thing it belongs to disappear — a
    renderer that only paints its code blocks would come out as a page of code
    block with an invisible code block on it.
    """
    counts: dict[str, int] = {}
    for row in screen.grid:
        for cell in row:
            colour = cell.style.bg or palette.bg
            counts[colour] = counts.get(colour, 0) + 1
    return max(counts, key=counts.get) if counts else palette.bg


def cells(shot: Shot, ox: float = 0.0, oy: float = 0.0) -> list[str]:
    """Draw one grid: background runs first, then text runs over them."""
    parts: list[str] = []
    screen = shot.screen

    # Backgrounds first, as runs, so the page reads as one surface.
    for r, row in enumerate(screen.grid):
        start, current = 0, None
        for c in range(screen.cols + 1):
            colour = (row[c].style.bg or shot.palette.bg) if c < screen.cols else None
            if colour != current:
                if current and current != shot.page:
                    x, w = ox + start * CELL_W, (c - start) * CELL_W
                    parts.append(
                        f'<rect x="{x:.1f}" y="{oy + r * CELL_H:.1f}" width="{w:.1f}" '
                        f'height="{CELL_H:.1f}" fill="{current}"/>'
                    )
                start, current = c, colour

    # Then text, one run per set of attributes.
    for r, row in enumerate(screen.grid):
        y = oy + r * CELL_H + BASELINE
        start, current, text = 0, None, []
        for c in range(screen.cols + 1):
            style = row[c].style if c < screen.cols else None
            if style is None or current is None or not same_run(style, current):
                parts.extend(_run(text, start, y, ox, current, shot.palette))
                start, current, text = c, style, []
            if c < screen.cols:
                text.append(substitute(row[c].char))
    return parts


def _run(
    text: list[str], start: int, y: float, ox: float, style: Style | None, palette: Palette
) -> list[str]:
    """One <text> element, pinned to the cell grid it came from.

    `textLength` is what keeps the glyphs on their columns. Without it the
    browser's font metrics decide the spacing, and a 3% disagreement is enough
    for text to drift off the background rectangles drawn under it — which is
    invisible in one picture and obvious the moment two grids sit side by side.
    """
    if style is None:
        return []
    used = 0
    for index, char in enumerate(text):
        if char not in (" ", ""):
            used = index + 1
    if not used:
        return []
    run = "".join(text[:used])
    return [
        f'<text x="{ox + start * CELL_W:.1f}" y="{y:.1f}" '
        f'fill="{style.fg or palette.fg}"{attributes(style)} '
        f'textLength="{used * CELL_W:.1f}" lengthAdjust="spacingAndGlyphs" '
        f'xml:space="preserve">{escape(run)}</text>'
    ]


def document(width: float, height: float, body: list[str]) -> str:
    return "\n".join(
        [
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0f}" '
            f'height="{height:.0f}" viewBox="0 0 {width:.1f} {height:.1f}" '
            f'font-family="{FONTS}" font-size="{FONT_SIZE}">',
            *body,
            "</svg>",
        ]
    )


def single(shot: Shot) -> str:
    width, height = shot.screen.cols * CELL_W, shot.screen.rows * CELL_H
    body = [f'<rect width="100%" height="100%" fill="{shot.page}" rx="6"/>']
    body.extend(cells(shot))
    return document(width, height, body)


# The frame around a comparison. Deliberately neutral: neither program's colour.
FRAME, LABEL, MUTED = "#18181b", "#e4e4e7", "#a1a1aa"
PAD, GAP, CAPTION_H, FOOTER_H = 18.0, 18.0, 44.0, 30.0


def compare(left: Shot, right: Shot, note: str) -> str:
    """Two captures of the same document, side by side.

    Each panel keeps its own page colour, and both boxes are the height of the
    taller capture — the shorter one padded in its own colour rather than
    cropped, so neither renderer is shown having produced less than it did.
    """
    cols = max(left.screen.cols, right.screen.cols)
    rows = max(left.screen.rows, right.screen.rows)
    panel_w, panel_h = cols * CELL_W, rows * CELL_H
    width = PAD * 2 + GAP + panel_w * 2
    height = PAD + CAPTION_H + panel_h + FOOTER_H + PAD

    body = [f'<rect width="100%" height="100%" fill="{FRAME}" rx="8"/>']
    top = PAD + CAPTION_H
    # Left is glow: the thing the reader already knows, then the claim.
    for index, shot in enumerate((left, right)):
        ox = PAD + index * (panel_w + GAP)
        body.append(
            f'<text x="{ox:.1f}" y="{PAD + 14:.1f}" fill="{LABEL}" font-size="13" '
            f'font-weight="bold">{escape(shot.caption)}</text>'
        )
        body.append(
            f'<text x="{ox:.1f}" y="{PAD + 32:.1f}" fill="{MUTED}" font-size="11">'
            f"{escape(shot.argv)}</text>"
        )
        body.append(
            f'<rect x="{ox:.1f}" y="{top:.1f}" width="{panel_w:.1f}" '
            f'height="{panel_h:.1f}" fill="{shot.page}" rx="4"/>'
        )
        body.extend(cells(shot, ox=ox, oy=top))

    body.append(
        f'<text x="{PAD:.1f}" y="{top + panel_h + 20:.1f}" fill="{MUTED}" '
        f'font-size="11">{escape(note)}</text>'
    )
    return document(width, height, body)


SHOTS: dict[str, dict] = {
    "hero": {
        "target": "docs/screenshot.svg",
        "args": ["-t", "-s", "slate"],
        "source": "docs/demo.md",
        "palette": SLATE,
        "grid": (30, 100),
    },
    "paper": {
        "target": "docs/screenshot-paper.svg",
        "args": ["-t", "-s", "paper"],
        "source": "docs/demo.md",
        "palette": PAPER,
        "grid": (30, 100),
    },
    "search": {
        "target": "docs/screenshot-search.svg",
        "args": ["-t", "-s", "slate"],
        "source": "docs/demo.md",
        "palette": SLATE,
        "grid": (30, 100),
        "keys": "/the\r",
    },
    # No -t. The flag forces the reader and resolves the directory through
    # find_readme, so `-t .` quietly photographs README.md instead of the
    # browser. A directory with no -t is what actually opens the browser, and
    # walking the tree takes longer than a file does to render.
    "browser": {
        "target": "docs/screenshot-browser.svg",
        "args": ["-s", "slate"],
        "source": ".",
        "palette": SLATE,
        "grid": (16, 100),
        "settle": 3.0,
    },
}

# Both renderers wrap at 78 in an 80-column terminal — glow by its own
# min(term, 120) rule, marquee by the same one — so 80 is the width at which
# neither is being handed an advantage.
COMPARE_COLS = 80


def version(binary: str) -> str:
    try:
        out = subprocess.run(
            [binary, "--version"], capture_output=True, text=True, timeout=10, env=child_env()
        )
        return out.stdout.strip().splitlines()[0]
    except (OSError, IndexError, subprocess.SubprocessError):
        return binary


def shoot_compare(marquee: str, source: str, target: str, strict: bool) -> str:
    """The side-by-side. Fairness is enforced here, not promised in a caption."""
    glow = os.environ.get("GLOW_BIN", "glow")
    # --config /dev/null on both sides, because a config file on the machine
    # taking the picture would otherwise be photographed along with it. On this
    # machine glow's config sets width: 80, which would have shown up as glow
    # wrapping differently from its own defaults.
    left_argv = [glow, "--config", "/dev/null", source]
    right_argv = [marquee, "--config", "/dev/null", source]

    left = take(left_argv, TERMINAL, 1, COMPARE_COLS, oneshot=True, strict=strict)
    right = take(right_argv, SLATE, 1, COMPARE_COLS, oneshot=True, strict=strict)
    for shot, binary, argv in ((left, glow, left_argv), (right, marquee, right_argv)):
        shot.caption = version(binary)
        shot.argv = " ".join([os.path.basename(binary), *argv[1:]])

    note = (
        f"Same document, same {COMPARE_COLS}-column terminal, both at their defaults, "
        "no configuration on either side, nothing cropped. "
        "Regenerate: python3 scripts/screenshot.py --shot compare"
    )
    svg = compare(left, right, note)
    with open(target, "w", encoding="utf-8") as handle:
        handle.write(svg)
    print(f"wrote {target} ({left.screen.rows}x{COMPARE_COLS} vs {right.screen.rows}x{COMPARE_COLS})")
    return target


def self_test() -> int:
    """Check the escape-sequence parser against sequences both programs emit."""

    def styled(sequence: str) -> Style:
        screen = Screen(1, 4)
        feed(screen, sequence + "x")
        return screen.grid[0][0].style

    cases = [
        # 256-colour, the one that used to fall through to 39 and reset the
        # foreground instead of setting it.
        ("\x1b[38;5;39;1m", Style(fg="#00afff", bold=True)),
        ("\x1b[38;5;203;48;5;236mtext", Style(fg="#ff5f5f", bg="#303030")),
        ("\x1b[38;5;252m", Style(fg="#d0d0d0")),
        # Truecolor, which must not regress: marquee sets both in one sequence.
        ("\x1b[38;2;245;244;239;48;2;38;38;36m", Style(fg="#f5f4ef", bg="#262624")),
        # 16-colour.
        ("\x1b[31m", Style(fg="#cd0000")),
        ("\x1b[94m", Style(fg="#5c5cff")),
        # Attributes.
        ("\x1b[3m", Style(italic=True)),
        ("\x1b[4;9m", Style(underline=True, strike=True)),
        ("\x1b[3m\x1b[23m", Style()),
        # Malformed: consume nothing rather than falling through to the tail.
        ("\x1b[38;5m", Style()),
        ("\x1b[38m", Style()),
        # A reset really does reset.
        ("\x1b[38;5;39m\x1b[39m", Style()),
        ("\x1b[1;38;5;39m\x1b[0m", Style()),
    ]
    failures = 0
    for sequence, expected in cases:
        actual = styled(sequence)
        if actual != expected:
            failures += 1
            print(f"FAIL {sequence!r}\n  expected {expected}\n  actual   {actual}")
    print(f"{len(cases) - failures}/{len(cases)} parser cases pass")
    return 1 if failures else 0


def shoot(name: str, marquee: str, strict: bool) -> str:
    """Take one of the named shots."""
    spec = SHOTS[name]
    rows, cols = spec["grid"]
    argv = [marquee, "--config", "/dev/null", *spec["args"], spec["source"]]
    shot = take(
        argv,
        spec["palette"],
        rows,
        cols,
        settle=spec.get("settle", 1.8),
        keys=spec.get("keys", ""),
        strict=strict,
    )
    target = spec["target"]
    with open(target, "w", encoding="utf-8") as handle:
        handle.write(single(shot))
    print(f"wrote {target} ({rows}x{cols}, page {shot.page})")
    return target


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("source", nargs="?", help="markdown source (default docs/demo.md)")
    parser.add_argument("target", nargs="?", help="where to write the SVG")
    parser.add_argument(
        "--shot",
        help=f"a named shot: {', '.join(SHOTS)}, compare",
    )
    parser.add_argument("--all", action="store_true", help="take every named shot")
    parser.add_argument(
        "--strict",
        action="store_true",
        help="fail rather than warn on stderr output or overflow",
    )
    parser.add_argument("--self-test", action="store_true", help="check the parser and exit")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    marquee = os.environ.get("MARQUEE_BIN", "target/release/marquee-markdown")

    if args.all:
        for name in SHOTS:
            shoot(name, marquee, args.strict)
        shoot_compare(marquee, "docs/compare.md", "docs/compare-glow.svg", args.strict)
        return 0

    if args.shot == "compare":
        shoot_compare(marquee, args.source or "docs/compare.md", args.target or "docs/compare-glow.svg", args.strict)
        return 0

    if args.shot:
        if args.shot not in SHOTS:
            parser.error(f"unknown shot {args.shot!r}; try one of {', '.join(SHOTS)}, compare")
        return 0 if shoot(args.shot, marquee, args.strict) else 1

    # The original invocation, kept working: two positionals, or neither.
    spec = dict(SHOTS["hero"])
    spec["source"] = args.source or spec["source"]
    spec["target"] = args.target or spec["target"]
    SHOTS["hero"] = spec
    shoot("hero", marquee, args.strict)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
