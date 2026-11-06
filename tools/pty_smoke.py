#!/usr/bin/env python3
"""Real-PTY monitoring workflow; development dependency: pyte."""
import argparse
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
import uuid

import pyte

from pty_benchmark import Session


class ScreenSession(Session):
    def __init__(self, binary, directory, rows, cols):
        self.screen = pyte.Screen(cols, rows)
        self.stream = pyte.Stream(self.screen)
        self.raw = bytearray()
        super().__init__(binary, ["-d", "300"], directory, rows, cols, config_text=None)

    def read(self, timeout):
        data = super().read(timeout)
        self.raw.extend(data)
        self.stream.feed(data.decode("utf-8", errors="replace"))
        return data

    def send(self, keys):
        os.write(self.master, keys.encode())
        self.pump(.15)

    def expect(self, text):
        assert text in "\n".join(self.screen.display), f"missing {text!r}:\n" + "\n".join(self.screen.display)

    def resize(self, rows, cols):
        self.screen.resize(lines=rows, columns=cols)
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.pump(.2)


def error_exit_restores_raw_mode(binary):
    master, slave = pty.openpty()
    original = termios.tcgetattr(master)
    read_end, write_end = os.pipe()
    os.close(read_end)
    def setup():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)
    with tempfile.TemporaryDirectory(prefix="gander-error-pty-") as directory:
        process = subprocess.Popen([str(binary)], stdin=slave, stdout=write_end, stderr=slave,
                                   env=dict(os.environ, TERM="xterm-256color", GANDER_CONFIG=directory + "/missing.toml", XDG_STATE_HOME=directory),
                                   preexec_fn=setup)
        os.close(write_end)
        try:
            deadline = time.monotonic() + 5
            while process.poll() is None and time.monotonic() < deadline:
                if select.select([master], [], [], .05)[0]:
                    try:
                        os.read(master, 65536)
                    except OSError:
                        break
            assert process.wait(timeout=1) != 0
            assert termios.tcgetattr(master) == original, "failed terminal initialization left raw mode enabled"
        finally:
            os.close(slave)
            os.close(master)
            if process.poll() is None:
                process.kill()
            process.wait(timeout=3)


def workflow(binary, rows, cols):
    token = "gander-smoke-" + uuid.uuid4().hex[:8]
    code = "import subprocess,sys,time; child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(90)',sys.argv[1]+'-child']); print(child.pid,flush=True); time.sleep(90)"
    worker = subprocess.Popen([sys.executable, "-c", code, token, "X" * 1100 + "END-COMMAND" * 3],
                              start_new_session=True, stdout=subprocess.PIPE, text=True)
    child_pid = int(worker.stdout.readline())
    session = None
    try:
        with tempfile.TemporaryDirectory(prefix="gander-smoke-pty-") as directory:
            session = ScreenSession(binary, directory, rows, cols)
            try:
                session.pump(1)
                session.expect("CPU")
                session.expect("Memory")
                session.expect("All")
                session.send("\\" + token + "\r")
                session.expect("Filter:")
                session.expect(str(worker.pid))
                session.send("\x1b[17~")
                session.expect("Sort by")
                session.send("\x1b")
                session.send("I")
                session.expect("CPU% ^")
                header_y, header = next((y, line) for y, line in enumerate(session.screen.display) if "PID" in line and "COMMAND" in line)
                header_x = header.index("CPU%")
                session.send(f"\x1b[<0;{header_x+1};{header_y+1}M\x1b[<0;{header_x+1};{header_y+1}m")
                session.expect("CPU% v")
                session.send("t\x1b[H")
                session.send("-")
                session.expect("[+]")
                assert not any(str(child_pid) in line for line in session.screen.display), "folded child is still visible"
                session.pump(.5)
                session.expect("[+]")
                session.send("+")
                session.expect(str(child_pid))
                session.send("w")
                session.expect("Command line of")
                session.send("\x1b[F")
                session.expect("END-COMMAND")
                session.resize(24, 80)
                session.expect("Command line of")
                session.send("\x1b[F")
                session.expect("END-COMMAND")
                session.send("\x1b")
                session.send("u" + str(os.getuid()) + "\r")
                session.expect("User:")
                session.send("\x1b")
                session.send("\\no-match-" + uuid.uuid4().hex + "\r")
                session.expect("No matching processes")
                session.send("\x1b")
                session.send("\\" + token + "\r")
                session.send("BV")
                session.send("L")
                session.expect("Filter:")
                session.send("\x1b[17~")
                session.expect("Sort by")
                session.send("\x1b")
                session.send("L")
                session.resize(rows, cols)
                session.send("\x1b[H")
                session.send("k")
                session.expect("Send signal")
                session.send("\x1b")
            finally:
                session.close()
            assert session.restored_termios, "normal exit did not restore termios"
            assert b"\x1b[?1006l" in session.raw, "mouse capture was not disabled"
            assert b"\x1b[?1049l" in session.raw, "alternate screen was not restored"
    finally:
        try:
            os.killpg(worker.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        worker.wait(timeout=5)
        if worker.stdout:
            worker.stdout.close()
    print(f"ok: {cols}x{rows} live workflow and terminal restoration")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    options = parser.parse_args()
    binary = options.binary.resolve()
    for rows, cols in [(32, 160), (24, 120), (24, 100), (24, 80)]:
        workflow(binary, rows, cols)
    error_exit_restores_raw_mode(binary)
    print("ok: failed terminal initialization restores raw mode")
    with tempfile.TemporaryDirectory(prefix="gander-slow-quit-") as directory:
        session = Session(binary, ["-d", "10000"], directory, 24, 100)
        session.pump(1)
        start = time.monotonic()
        session.close()
        assert time.monotonic() - start < 1, "quit waited for the ten-second sampling interval"
        assert session.restored_termios
    print("ok: ten-second sampling does not delay quit")
    with tempfile.TemporaryDirectory(prefix="gander-interrupted-quit-") as directory:
        session = ScreenSession(binary, directory, 24, 100)
        try:
            session.pump(.5)
            session.send("\x1bOQ")
            session.expect("Header")
            os.write(session.master, b"\x03")
            deadline = time.monotonic() + 2
            while session.process.poll() is None and time.monotonic() < deadline:
                try:
                    session.read(.05)
                except RuntimeError:
                    break
            assert session.process.wait(timeout=1) == 130
        finally:
            session.close()
        assert session.restored_termios
        assert not (Path(directory) / "config.toml").exists(), "interruption saved configuration"
    print("ok: Ctrl-C exits setup without saving configuration")


if __name__ == "__main__":
    main()
