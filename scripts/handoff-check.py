#!/usr/bin/env python3
"""Check that handing the terminal to an editor really hands it over.

The reader watches standard input on a thread of its own. While an editor has
the terminal, that thread must not read a byte of it: two processes blocking on
one tty split the keystrokes between them, and the reader wins the race often
enough that an editor opened with `e` loses whole words and stalls waiting for
escape sequences whose tail it never receives.

That is not visible to a unit test — it needs two processes and a real
terminal — so it is checked here. The editor is a stub that records exactly
what it was sent, and the text typed into it contains a `q`: if the reader is
still listening it takes that as the quit binding and the session ends, which
is the same defect seen from the other side.

Usage: scripts/handoff-check.py [path-to-binary]
"""

from __future__ import annotations

import fcntl
import os
import signal
import pty
import select
import struct
import termios
import shutil
import subprocess
import sys
import tempfile
import time

# Every character has to arrive, and the `q` has to arrive *here* rather than
# being taken by the reader as a binding.
TYPED = "The quick brown fox"
READY = "EDITOR-READY"
SENTINEL = "\x04"

STUB_EDITOR = r"""#!/usr/bin/env python3
import os, select, sys, termios, time, tty
log = os.environ["HANDOFF_LOG"]
# Standard input, which is the descriptor the reader and this program are
# being asked not to fight over.
tty_fd = 0
saved = termios.tcgetattr(tty_fd)
tty.setraw(tty_fd)
try:
    os.write(tty_fd, b"EDITOR-READY")
    seen = b""
    # Bounded, and not merely as a courtesy: nothing reaps this process, so a
    # sentinel that never arrives would leave it holding the terminal for as
    # long as the machine stays up.
    deadline = time.time() + 30
    while not seen.endswith(b"\x04") and time.time() < deadline:
        if not select.select([tty_fd], [], [], 0.5)[0]:
            continue
        chunk = os.read(tty_fd, 1)
        if not chunk:
            break
        seen += chunk
finally:
    termios.tcsetattr(tty_fd, termios.TCSADRAIN, saved)
with open(log, "wb") as handle:
    handle.write(seen)
sys.exit(0)
"""


def run_reader(slave, master, binary, document, editor, log):
    """Become the reader, in the child half of the fork. Never returns."""
    os.setsid()
    # Claim the pty as the controlling terminal, so the editor this launches
    # sees what a real one would and can open /dev/tty. Best-effort: not
    # every platform exposes the call from Python, and the stub reads standard
    # input, which is the descriptor actually being contested.
    if hasattr(termios, "TIOCSCTTY"):
        try:
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
        except OSError:
            pass
    for fd in (0, 1, 2):
        os.dup2(slave, fd)
    os.close(master)
    os.close(slave)
    os.environ.update(TERM="xterm-256color", EDITOR=editor, HANDOFF_LOG=log, NO_COLOR="1")
    os.execv(binary, [binary, "-t", document])


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


# Nothing here should take more than a few seconds, and this runs unattended
# in CI. A check that hangs is worse than one that fails: it holds a machine
# until something else gives up on it.
WATCHDOG = 120


# Where the check had got to, for the watchdog to name. It runs unattended on
# machines nobody is watching, so a timeout has to say more than that one
# happened.
STAGE = "starting up"


def at(stage: str) -> None:
    global STAGE
    STAGE = stage


def watchdog(_signum, _frame):
    raise SystemExit(f"timed out after {WATCHDOG}s while {STAGE}")


def main() -> int:
    if hasattr(signal, "SIGALRM"):
        signal.signal(signal.SIGALRM, watchdog)
        signal.alarm(WATCHDOG)
    binary = sys.argv[1] if len(sys.argv) > 1 else "target/debug/marquee-markdown"
    if not os.path.exists(binary):
        print(f"no binary at {binary}; run `cargo build` first", file=sys.stderr)
        return 2

    work = tempfile.mkdtemp(prefix="handoff-")
    try:
        document = os.path.join(work, "note.md")
        with open(document, "w") as handle:
            handle.write("# Title\n\nSome body text.\n")
        editor = os.path.join(work, "stub-editor")
        with open(editor, "w") as handle:
            handle.write(STUB_EDITOR)
        os.chmod(editor, 0o755)
        log = os.path.join(work, "received")

        master, slave = pty.openpty()
        # A pty starts out with no size at all, and a reader given no rows
        # draws nothing.
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        child = os.fork()
        if child == 0:
            # Everything from here to `execv` runs in the child, and nothing
            # in it may return: a fork that falls back into the caller's code
            # leaves two processes running this script, the second one holding
            # a stdout that whoever ran us is still waiting on.
            try:
                run_reader(slave, master, binary, document, editor, log)
            except BaseException:
                os._exit(127)
            os._exit(127)
        os.close(slave)

        try:
            # Let the reader draw its first frame, then ask it to edit.
            at("waiting for the reader to draw its first frame")
            read_until(master, b"Title", 5.0)
            os.write(master, b"e")
            at("waiting for the editor to start")
            seen = read_until(master, READY.encode(), 5.0)
            if READY.encode() not in seen:
                print("FAIL: the editor never started", file=sys.stderr)
                if os.environ.get("HANDOFF_DEBUG"):
                    print(repr(seen[:800]), file=sys.stderr)
                return 1

            # Type into the editor exactly as a person would.
            at("typing into the editor")
            for char in TYPED:
                os.write(master, char.encode())
                time.sleep(0.012)
            os.write(master, SENTINEL.encode())

            at("waiting for the editor to record what it was sent")
            deadline = time.time() + 5.0
            while time.time() < deadline and not os.path.exists(log):
                time.sleep(0.02)
            if not os.path.exists(log):
                print("FAIL: the editor never recorded anything", file=sys.stderr)
                return 1

            with open(log, "rb") as handle:
                got = handle.read().rstrip(SENTINEL.encode()).decode(
                    "utf-8", "replace"
                )

            failures = []
            if got != TYPED:
                failures.append(f"the editor was sent {got!r}, not {TYPED!r}")

            # If the reader took the `q` it would have quit; give it a moment
            # to have done so, then check it is still there.
            time.sleep(0.5)
            alive = os.waitpid(child, os.WNOHANG) == (0, 0)
            if not alive:
                failures.append("the reader quit: it read the editor's keystrokes")

            for failure in failures:
                print(f"FAIL: {failure}", file=sys.stderr)
            if failures:
                return 1
            print(f"ok: the editor received {got!r} and the reader stayed out of it")
            return 0
        finally:
            at("cleaning up")
            try:
                os.kill(child, 9)
                os.waitpid(child, 0)
            except OSError:
                pass
            os.close(master)
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
