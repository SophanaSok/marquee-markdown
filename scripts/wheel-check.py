#!/usr/bin/env python3
"""Check what the reader actually asks a terminal for, and that a wheel works.

Two things here are invisible to `cargo test`. The unit tests can only check
what `setup` writes into a `Vec`; what the *process* puts on a terminal on the
way up is a different claim, and it is the one that matters — a mode asked for
in the wrong order, or through a path that never runs, fails nowhere else. And
the wheel is only reported at all because `?1000h` reports buttons 4 to 7: no
test that does not parse a real byte stream can hold that.

So: start the reader on a pty, read the modes it set, then post it mouse
reports the way a terminal would and watch what it does with them.

Usage: scripts/wheel-check.py [path-to-binary]
"""

from __future__ import annotations

import fcntl
import os
import pty
import select
import signal
import struct
import shutil
import sys
import tempfile
import termios
import time

# Deep enough that no first frame can be showing it, so finding it on screen
# can only mean the document moved.
DEPTH = 60
MARKER = "SCROLLED-THIS-FAR"

# One tick is three lines, and a paragraph here is two (its text and the blank
# line after it), so this is comfortably past the marker on a 24-row terminal
# without relying on exactly where the layout put it.
TICKS = 60

# SGR mouse reports, as a terminal in `?1006h` sends them. The button number
# is the low two bits plus the wheel's 64: 65 is button 5, wheel down. 35 is
# button 3 with the motion bit, which is what a pointer crossing a cell looks
# like — the report this program asks not to be sent and drops if it is.
WHEEL_DOWN = b"\x1b[<65;5;5M"
POINTER_MOVED = b"\x1b[<35;5;5M"

# Enough to separate a frame apiece from nothing at all, spaced the way a hand
# moving a pointer across a window really spaces them.
REPORTS = 200

# Nothing here should take more than a few seconds, and this runs unattended
# in CI. A check that hangs is worse than one that fails.
WATCHDOG = 120

STAGE = "starting up"


def at(stage: str) -> None:
    global STAGE
    STAGE = stage


def watchdog(_signum, _frame):
    raise SystemExit(f"timed out after {WATCHDOG}s while {STAGE}")


def read_until(master: int, needle: bytes, timeout: float) -> bytes:
    """Collect output until `needle` shows up, or time runs out."""
    seen = b""
    deadline = time.time() + timeout
    while time.time() < deadline and needle not in seen:
        if select.select([master], [], [], 0.05)[0]:
            try:
                chunk = os.read(master, 65536)
            except OSError:
                break
            if not chunk:
                break
            seen += chunk
    return seen


def drain(master: int, quiet: float = 0.25) -> bytes:
    """Everything the reader has to say until it stops saying anything."""
    seen = b""
    while select.select([master], [], [], quiet)[0]:
        try:
            chunk = os.read(master, 65536)
        except OSError:
            break
        if not chunk:
            break
        seen += chunk
    return seen


def spawn(binary: str, document: str, args: list[str]):
    """Start the reader on a pty of its own. Returns (child pid, master fd)."""
    master, slave = pty.openpty()
    # A pty starts out with no size at all, and a reader given no rows draws
    # nothing.
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    child = os.fork()
    if child == 0:
        # Nothing in here may return: a fork that falls back into the caller's
        # code leaves two processes running this script.
        try:
            os.setsid()
            if hasattr(termios, "TIOCSCTTY"):
                try:
                    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
                except OSError:
                    pass
            for fd in (0, 1, 2):
                os.dup2(slave, fd)
            os.close(master)
            os.close(slave)
            os.environ.update(TERM="xterm-256color", NO_COLOR="1")
            os.execv(binary, [binary, "-t", *args, document])
        except BaseException:
            os._exit(127)
        os._exit(127)
    os.close(slave)
    return child, master


def stop(child: int, master: int) -> None:
    try:
        os.kill(child, 9)
        os.waitpid(child, 0)
    except OSError:
        pass
    os.close(master)


def cpu_seconds(pid: int) -> float | None:
    """User plus system time, or None where /proc does not exist."""
    try:
        with open(f"/proc/{pid}/stat") as handle:
            fields = handle.read().rsplit(") ", 1)[1].split()
    except (OSError, IndexError):
        return None
    ticks = os.sysconf("SC_CLK_TCK")
    return (int(fields[11]) + int(fields[12])) / ticks


def modes_check(binary: str, document: str, failures: list[str]) -> None:
    """What the reader asks the terminal for, on the way up."""
    for args, present, absent in [
        (
            [],
            # The wheel, in the encoding that can express a column past 223.
            [b"\x1b[?1000h", b"\x1b[?1006h"],
            # Button-event and any-event tracking, which crossterm's
            # `EnableMouseCapture` bundles in and nothing here reads.
            [b"\x1b[?1002h", b"\x1b[?1003h"],
        ),
        (
            ["--no-mouse"],
            # Tracking another program may have left on, cleared rather than
            # inherited.
            [b"\x1b[?1003l", b"\x1b[?1000l"],
            [b"\x1b[?1000h"],
        ),
    ]:
        at(f"reading the modes set by {' '.join(args) or 'a default run'}")
        child, master = spawn(binary, document, args)
        try:
            seen = read_until(master, MARKER.encode()[:8], 5.0) + drain(master, 0.15)
        finally:
            stop(child, master)
        how = " ".join(args) or "by default"
        for mode in present:
            if mode not in seen:
                failures.append(f"{how}: {mode.decode()!r} was never sent")
        for mode in absent:
            if mode in seen:
                failures.append(f"{how}: {mode.decode()!r} was sent")


def wheel_check(binary: str, document: str, failures: list[str]) -> None:
    """A wheel report scrolls the document.

    This is what proves `?1000h` and `?1006h` are enough on their own — the
    claim the whole mode set rests on, and one that cannot be made from
    anything short of real bytes.
    """
    at("waiting for the reader's first frame")
    child, master = spawn(binary, document, [])
    try:
        first = read_until(master, b"line 1", 5.0) + drain(master, 0.15)
        if MARKER.encode() in first:
            failures.append(
                f"{MARKER} was already on screen, so scrolling to it proves nothing"
            )
        at("posting wheel reports")
        for _ in range(TICKS):
            os.write(master, WHEEL_DOWN)
            time.sleep(0.004)
        if MARKER.encode() not in drain(master, 0.5):
            failures.append("a wheel report did not scroll the document")
    finally:
        stop(child, master)


def motion_check(binary: str, document: str, failures: list[str]) -> None:
    """Pointer movement costs nothing.

    The obvious assertion — that nothing comes back — is worthless: the frame
    a motion report provokes is identical to the one before it, so the diff is
    empty and not a byte reaches the terminal either way. That is exactly why
    this went unnoticed. What it costs is measurable only as time on a CPU.

    The reports have to be spaced out to cost anything. The loop drains
    everything waiting before it draws, so five hundred posted in one go are
    one frame however they are handled — and a pointer crossing a window does
    not arrive in one go. Sent a few milliseconds apart, the way one really
    does, each is its own wakeup and its own frame.
    """
    at("measuring what pointer movement costs")
    child, master = spawn(binary, document, [])
    try:
        read_until(master, b"line 1", 5.0)
        drain(master, 0.2)
        before = cpu_seconds(child)
        if before is None:
            print("skipped the motion measurement: no /proc on this platform")
            return
        for _ in range(REPORTS):
            os.write(master, POINTER_MOVED)
            time.sleep(0.005)
        drain(master, 0.5)
        spent = cpu_seconds(child) - before
        # A frame apiece lands an order of magnitude above this; events
        # dropped before the loop sees them land two below it.
        if spent > 0.15:
            failures.append(
                f"{REPORTS} pointer reports cost {spent:.3f}s of CPU: they are being drawn"
            )
        else:
            print(f"ok: {REPORTS} pointer reports cost {spent:.3f}s of CPU")
    finally:
        stop(child, master)


def main() -> int:
    if hasattr(signal, "SIGALRM"):
        signal.signal(signal.SIGALRM, watchdog)
        signal.alarm(WATCHDOG)
    binary = sys.argv[1] if len(sys.argv) > 1 else "target/debug/marquee-markdown"
    if not os.path.exists(binary):
        print(f"no binary at {binary}; run `cargo build` first", file=sys.stderr)
        return 2

    work = tempfile.mkdtemp(prefix="wheel-")
    try:
        document = os.path.join(work, "long.md")
        with open(document, "w") as handle:
            handle.write("# Title\n\n")
            for line in range(1, DEPTH * 2):
                handle.write(f"{MARKER}\n\n" if line == DEPTH else f"line {line}\n\n")

        failures: list[str] = []
        modes_check(binary, document, failures)
        wheel_check(binary, document, failures)
        motion_check(binary, document, failures)

        for failure in failures:
            print(f"FAIL: {failure}", file=sys.stderr)
        if failures:
            return 1
        print("ok: the reader asks for a wheel, is scrolled by one, and ignores the rest")
        return 0
    finally:
        at("cleaning up")
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
