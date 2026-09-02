#!/usr/bin/env python3
"""Check that `--style system` follows the terminal, and follows it safely.

Two things have to be true at once, and neither is reachable from a unit test
because both need a real terminal that answers questions:

1. **It follows.** When the terminal is retinted, the page is repainted in the
   new colors without the reader touching anything. Here the trigger is a
   focus-in report, which is the portable one.
2. **It does not eat the keyboard doing it.** An `OSC` reply and a keystroke
   are the same bytes on the same stream. If the reply reaches crossterm
   instead of the code that asked for it, it is parsed as a handful of
   bindings — `q` among them — and the session ends on its own. So this types
   after the exchange and checks the reader is still there and still listening.

The terminal is played by this script: it answers `OSC 11`, `OSC 10`, `OSC 4`
and the device-attributes sentinel, with one palette before the focus report
and a different one after.

Usage: scripts/recolor-check.py [path-to-binary]
"""

from __future__ import annotations

import fcntl
import os
import pty
import select
import struct
import sys
import tempfile
import termios
import time

# Two palettes far enough apart that the repaint is unmistakable in the bytes:
# near-black, then near-white. The second also flips the appearance from dark
# to light, so a reader that merely re-ran the query without rebuilding the
# palette would still fail here.
BEFORE = (0x18, 0x18, 0x18)
AFTER = (0xFA, 0xF9, 0xF5)

DEVICE_ATTRIBUTES = b"\x1b[?62;c"
# A pty is born with no window size, and a reader given no room draws nothing
# at all — which looks exactly like a reader that failed to draw.
SIZE = (24, 80)
FOCUS_IN = b"\x1b[I"
TIMEOUT = 10.0


def spec(rgb: tuple[int, int, int]) -> str:
    """One color, in the `rgb:` form terminals actually answer with."""
    r, g, b = rgb
    return f"rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}"


def answer(bg: tuple[int, int, int], asked: bytes) -> bytes:
    """Reply to exactly the questions `asked` contains.

    Answering questions that were not asked would be a worse terminal than a
    real one and could hide a bug: the point of the probe is that it asks for
    the background *alone*, and a script that always sent eighteen answers
    would make an eighteen-question probe look identical to a two-question one.
    """
    # The foreground is the opposite end of whichever palette is in force, so
    # the contrast floor in `theme::system` is always cleared.
    fg = (0xD8, 0xD8, 0xD8) if sum(bg) < 384 else (0x18, 0x18, 0x18)
    out = b""
    if b"\x1b]11;?" in asked:
        out += f"\x1b]11;{spec(bg)}\x07".encode()
    if b"\x1b]10;?" in asked:
        out += f"\x1b]10;{spec(fg)}\x07".encode()
    for slot in range(16):
        if f"\x1b]4;{slot};?".encode() in asked:
            shade = (slot * 16) % 256
            out += f"\x1b]4;{slot};{spec((shade, shade, shade))}\x07".encode()
    if b"\x1b[c" in asked:
        out += DEVICE_ATTRIBUTES
    return out


def read_until(fd: int, predicate, deadline: float) -> bytes:
    """Collect output until `predicate` is happy with it, or time runs out."""
    seen = b""
    while time.monotonic() < deadline:
        ready, _, _ = select.select([fd], [], [], 0.2)
        if not ready:
            if predicate(seen):
                return seen
            continue
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            break
        if not chunk:
            break
        seen += chunk
        if predicate(seen):
            return seen
    return seen


def serve_query(fd: int, bg: tuple[int, int, int], deadline: float) -> bytes:
    """Wait for a colour question and answer it. Returns what was asked."""
    asked = read_until(fd, lambda seen: b"\x1b[c" in seen, deadline)
    if b"\x1b]11;?" not in asked:
        raise SystemExit(f"the reader never asked about the background: {asked[:200]!r}")
    os.write(fd, answer(bg, asked))
    return asked


def start(binary: str, work: str, style: str = "system"):
    """A reader on a pty of a known size, with settings of its own."""
    document = os.path.join(work, "doc.md")
    with open(document, "w", encoding="utf-8") as handle:
        handle.write("# Title\n\nBody text.\n")
    pid, fd = pty.fork()
    if pid == 0:
        # A config of its own, so the reader's real settings cannot change
        # what this checks — and no update check, which would otherwise reach
        # the network from a test.
        os.environ["MARQUEE_UPDATE_CHECK"] = "0"
        os.environ["XDG_CONFIG_HOME"] = work
        os.execv(binary, [binary, "-t", "--style", style, document])
    rows, cols = SIZE
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    return pid, fd


def stop(pid: int, fd: int) -> None:
    try:
        os.kill(pid, 9)
        os.waitpid(pid, 0)
    except (ProcessLookupError, ChildProcessError):
        pass
    os.close(fd)


def follows_the_terminal(binary: str) -> None:
    """The whole feature: retint the terminal, and the page follows."""
    with tempfile.TemporaryDirectory() as work:
        pid, fd = start(binary, work)
        deadline = time.monotonic() + TIMEOUT
        try:
            # 1. The question asked before the screen is taken.
            first = serve_query(fd, BEFORE, deadline)
            if b"\x1b]4;15;?" not in first:
                raise SystemExit("the first query should ask for the whole palette")

            # The first frame, painted in the first palette.
            painted = read_until(fd, lambda seen: b"48;2;24;24;24" in seen, deadline)
            if b"48;2;24;24;24" not in painted:
                raise SystemExit(
                    "the page was never painted in the terminal's own background"
                )

            # 2. Focus comes back — the portable trigger.
            os.write(fd, FOCUS_IN)

            # 3. The probe: the background alone, not the whole palette. This
            #    is the efficiency guard, and it is worth failing over.
            probe = read_until(fd, lambda seen: b"\x1b[c" in seen, deadline)
            if b"\x1b]11;?" not in probe:
                raise SystemExit(f"no probe followed the focus report: {probe[:200]!r}")
            if b"\x1b]4;0;?" in probe:
                raise SystemExit(
                    "the probe asked for the whole palette; it should ask for "
                    "the background alone until it knows something changed"
                )
            os.write(fd, answer(AFTER, probe))

            # 4. The background moved, so now the full read is paid for.
            full = read_until(fd, lambda seen: b"\x1b]4;15;?" in seen, deadline)
            if b"\x1b]4;15;?" not in full:
                raise SystemExit(
                    "a changed background did not lead to a full palette read"
                )
            os.write(fd, answer(AFTER, full))

            # 5. Repainted in the new colors, with nobody having pressed a key.
            repainted = read_until(
                fd, lambda seen: b"48;2;250;249;245" in seen, deadline
            )
            if b"48;2;250;249;245" not in repainted:
                raise SystemExit("the page was not repainted in the new background")

            # 6. And the keyboard still works. If any reply leaked into
            #    crossterm the reader has already acted on it — often by
            #    quitting — so reaching here at all is most of the check;
            #    `q` proves it is still listening rather than merely alive.
            os.write(fd, b"q")
            _, status = os.waitpid(pid, 0)
            if not os.WIFEXITED(status) or os.WEXITSTATUS(status) != 0:
                raise SystemExit(f"the reader did not quit cleanly: {status}")
        finally:
            stop(pid, fd)


def a_burst_costs_one_question(binary: str) -> None:
    """Focus bouncing in and out is one theme switch, not five.

    The rate limit is what keeps this free for a reader who alt-tabs all day,
    and it is invisible from inside the process: only a terminal can see how
    many times it was asked.
    """
    with tempfile.TemporaryDirectory() as work:
        pid, fd = start(binary, work)
        deadline = time.monotonic() + TIMEOUT
        try:
            serve_query(fd, BEFORE, deadline)
            read_until(fd, lambda seen: b"48;2;24;24;24" in seen, deadline)

            # Five focus reports in a row, answering nothing. The first is
            # allowed to ask; the rest are inside the cooldown.
            for _ in range(5):
                os.write(fd, FOCUS_IN)
                time.sleep(0.02)
            # Long enough for every one of them to have been acted on, and
            # still short of the cooldown.
            asked = read_until(fd, lambda seen: False, time.monotonic() + 0.3)
            probes = asked.count(b"\x1b]11;?")
            if probes > 1:
                raise SystemExit(
                    f"a burst of {5} focus reports asked the terminal {probes} times; "
                    "the cooldown should have collapsed them into one"
                )
        finally:
            stop(pid, fd)


def a_signal_is_a_trigger(binary: str) -> None:
    """SIGUSR1, which is what a desktop's theme hook sends."""
    import signal

    with tempfile.TemporaryDirectory() as work:
        pid, fd = start(binary, work)
        deadline = time.monotonic() + TIMEOUT
        try:
            serve_query(fd, BEFORE, deadline)
            read_until(fd, lambda seen: b"48;2;24;24;24" in seen, deadline)
            os.kill(pid, signal.SIGUSR1)
            probe = read_until(fd, lambda seen: b"\x1b[c" in seen, deadline)
            if b"\x1b]11;?" not in probe:
                raise SystemExit(
                    f"SIGUSR1 did not make the reader ask: {probe[:200]!r}"
                )
        finally:
            stop(pid, fd)


def a_silent_terminal_is_never_asked_twice(binary: str) -> None:
    """The guard that keeps this free under `screen` and behind a pipe.

    A terminal that answered nothing the first time is not asked again, ever.
    Without this, every focus regain spends the timeout finding out the same
    thing.
    """
    with tempfile.TemporaryDirectory() as work:
        pid, fd = start(binary, work)
        deadline = time.monotonic() + TIMEOUT
        try:
            # Asked once, and answered with silence — as `screen` does.
            read_until(fd, lambda seen: b"\x1b[c" in seen, deadline)
            read_until(fd, lambda seen: b"\x1b[?1049h" in seen, deadline)
            # Now trigger every way there is.
            import signal

            os.write(fd, FOCUS_IN)
            os.kill(pid, signal.SIGUSR1)
            os.write(fd, b"R")
            after = read_until(fd, lambda seen: False, time.monotonic() + 0.6)
            if b"\x1b]11;?" in after:
                raise SystemExit(
                    "a terminal that never answered was asked again; it will "
                    "pay the timeout on every trigger for the rest of the session"
                )
        finally:
            stop(pid, fd)


def main() -> int:
    binary = sys.argv[1] if len(sys.argv) > 1 else "target/debug/mmd"
    binary = os.path.abspath(binary)
    if not os.access(binary, os.X_OK):
        raise SystemExit(f"not executable: {binary}")

    for check in (
        follows_the_terminal,
        a_burst_costs_one_question,
        a_signal_is_a_trigger,
        a_silent_terminal_is_never_asked_twice,
    ):
        check(binary)
        print(f"recolor: {check.__name__.replace('_', ' ')}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
