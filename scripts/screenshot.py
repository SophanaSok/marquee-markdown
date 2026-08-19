#!/usr/bin/env python3
"""Turn a real frame of the reader into an SVG for the README.

Runs the binary in a pseudo-terminal, interprets what it writes into a grid of
styled cells, and draws that grid as an SVG. The point is that the picture in
the README is the actual output rather than a mock-up, and that anyone can
regenerate it:

    python3 scripts/screenshot.py docs/demo.md docs/screenshot.svg

Only the escape sequences ratatui actually emits are handled — cursor
positioning, SGR colours, and screen clearing. This is not a terminal emulator
and does not try to be.

One substitution is made, and only in the picture: the reader draws callout and
image icons with Nerd Font glyphs from the private use area, which a browser
has no font for and would show as empty boxes. They are swapped for the nearest
standard character so the image looks like what a reader with a Nerd Font
actually sees, rather than like a rendering fault.
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


class Screen:
    """Just enough of a terminal to reconstruct one frame."""

    def __init__(self, rows: int, cols: int) -> None:
        self.rows, self.cols = rows, cols
        self.grid = [[Cell() for _ in range(cols)] for _ in range(rows)]
        self.row = self.col = 0
        self.style = Style()

    def put(self, char: str) -> None:
        if self.row >= self.rows or self.col >= self.cols:
            return
        self.grid[self.row][self.col] = Cell(char, self.style)
        self.col += 1
        if wide(char) and self.col < self.cols:
            # The second half of a wide glyph is covered by the first.
            self.grid[self.row][self.col] = Cell("", self.style)
            self.col += 1

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
            elif code == 39:
                self.style = replace(self.style, fg=None)
            elif code == 49:
                self.style = replace(self.style, bg=None)
            elif code in (38, 48) and params[index + 1 : index + 2] == [2]:
                r, g, b = params[index + 2 : index + 5]
                colour = f"#{r:02x}{g:02x}{b:02x}"
                key = "fg" if code == 38 else "bg"
                self.style = replace(self.style, **{key: colour})
                index += 4
            index += 1


CSI = re.compile(r"\x1b\[([0-9;?]*)([A-Za-z])")
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
            params = [int(p) for p in raw.split(";") if p.isdigit()]
            if final in "Hf":
                screen.row = (params[0] if params else 1) - 1
                screen.col = (params[1] if len(params) > 1 else 1) - 1
            elif final == "m":
                screen.sgr(params or [0])
            elif final == "J":
                screen.grid = [[Cell() for _ in range(screen.cols)] for _ in range(screen.rows)]
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


def capture(command: list[str], settle: float = 1.8) -> str:
    """Run `command` on a pseudo-terminal of a fixed size and collect output."""
    import fcntl
    import struct
    import termios

    primary, secondary = pty.openpty()
    fcntl.ioctl(secondary, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
    process = subprocess.Popen(
        command, stdin=secondary, stdout=secondary, stderr=secondary, close_fds=True
    )
    os.close(secondary)

    out, deadline = [], time.time() + settle
    while time.time() < deadline:
        ready, _, _ = select.select([primary], [], [], 0.1)
        if ready:
            try:
                chunk = os.read(primary, 65536)
            except OSError:
                break
            if not chunk:
                break
            out.append(chunk.decode("utf-8", "replace"))
    os.write(primary, b"q")
    time.sleep(0.3)
    process.terminate()
    process.wait(timeout=5)
    os.close(primary)
    return "".join(out)


def escape(text: str) -> str:
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def to_svg(screen: Screen, page: str) -> str:
    width, height = screen.cols * CELL_W, screen.rows * CELL_H
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0f}" height="{height:.0f}" '
        f'viewBox="0 0 {width:.1f} {height:.1f}" font-family="{FONTS}" font-size="{FONT_SIZE}">',
        f'<rect width="100%" height="100%" fill="{page}" rx="6"/>',
    ]

    # Backgrounds first, as runs, so the page reads as one surface.
    for r, row in enumerate(screen.grid):
        start, current = 0, None
        for c in range(screen.cols + 1):
            colour = row[c].style.bg if c < screen.cols else None
            if colour != current:
                if current and current != page:
                    x, w = start * CELL_W, (c - start) * CELL_W
                    parts.append(
                        f'<rect x="{x:.1f}" y="{r * CELL_H:.1f}" width="{w:.1f}" '
                        f'height="{CELL_H:.1f}" fill="{current}"/>'
                    )
                start, current = c, colour

    # Then text, one run per (colour, weight).
    for r, row in enumerate(screen.grid):
        y = r * CELL_H + BASELINE
        start, current, text = 0, None, []
        for c in range(screen.cols + 1):
            cell = row[c] if c < screen.cols else Cell()
            key = (cell.style.fg, cell.style.bold) if c < screen.cols else None
            if key != current:
                run = "".join(text).rstrip()
                if run:
                    fg, bold = current
                    weight = ' font-weight="bold"' if bold else ""
                    parts.append(
                        f'<text x="{start * CELL_W:.1f}" y="{y:.1f}" '
                        f'fill="{fg or "#f5f4ef"}"{weight} '
                        f'xml:space="preserve">{escape(run)}</text>'
                    )
                start, current, text = c, key, []
            text.append(substitute(cell.char))
    parts.append("</svg>")
    return "\n".join(parts)


def main() -> int:
    source = sys.argv[1] if len(sys.argv) > 1 else "docs/demo.md"
    target = sys.argv[2] if len(sys.argv) > 2 else "docs/screenshot.svg"
    binary = os.environ.get("MARQUEE_BIN", "target/release/marquee-markdown")

    screen = Screen(ROWS, COLS)
    feed(screen, capture([binary, "-t", "-s", "slate", source]))

    # The page colour is whatever most of the frame is painted with.
    counts: dict[str, int] = {}
    for row in screen.grid:
        for cell in row:
            if cell.style.bg:
                counts[cell.style.bg] = counts.get(cell.style.bg, 0) + 1
    page = max(counts, key=counts.get) if counts else "#262624"

    with open(target, "w", encoding="utf-8") as handle:
        handle.write(to_svg(screen, page))
    print(f"wrote {target} ({ROWS}x{COLS}, page {page})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
