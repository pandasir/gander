#!/usr/bin/env python3
"""Measure a real TUI in an isolated PTY; Python standard library only."""
import argparse
import ctypes
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import pty
import re
import select
import statistics
import struct
import subprocess
import tempfile
import termios
import time

CONFIG = '''delay_ms = 1000
layout = "panels"
show_userland_threads = false
show_kernel_threads = true
active_tab = 0
[[tabs]]
name = "Benchmark"
columns = ["PID", "PERCENT_CPU", "PERCENT_MEM", "M_RESIDENT", "COMM"]
[tabs.sort]
field = "PERCENT_CPU"
desc = true
'''
ANSI = re.compile(rb"\x1b(?:\[[0-?]*[ -/]*[@-~]|[()][0-2A-Z]|[=>M])")


class DarwinTask(ctypes.Structure):
    _fields_ = [(n, ctypes.c_uint64) for n in (
        "virtual", "resident", "user", "system", "threads_user", "threads_system"
    )] + [(n, ctypes.c_int32) for n in (
        "policy", "faults", "pageins", "cow", "messages_sent", "messages_received",
        "syscalls_mach", "syscalls_unix", "csw", "threadnum", "running", "priority"
    )]


def counters(pid):
    if platform.system() == "Darwin":
        task = DarwinTask()
        lib = ctypes.CDLL("/usr/lib/libproc.dylib")
        if lib.proc_pidinfo(pid, 4, 0, ctypes.byref(task), ctypes.sizeof(task)) != ctypes.sizeof(task):
            raise OSError("cannot read process task counters")
        tb = (ctypes.c_uint32 * 2)()
        ctypes.CDLL(None).mach_timebase_info(ctypes.byref(tb))
        return (task.user + task.system) * tb[0] / tb[1] / 1e9, task.resident
    if platform.system() == "Linux":
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK"), int(fields[21]) * os.sysconf("SC_PAGE_SIZE")
    raise RuntimeError("CPU accounting currently supports Darwin and Linux")


class Session:
    def __init__(self, binary, args, directory, rows, cols, config_text=CONFIG):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.initial_termios = termios.tcgetattr(slave)
        config = Path(directory) / "config.toml"
        if config_text is not None:
            config.write_text(config_text)
        env = dict(os.environ, TERM="xterm-256color", GANDER_CONFIG=str(config),
                   XDG_STATE_HOME=directory, HTOPRC=str(Path(directory) / "htoprc"))
        def setup():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)
        try:
            self.process = subprocess.Popen([str(binary), *args], stdin=slave, stdout=slave,
                                            stderr=slave, env=env, preexec_fn=setup)
        except BaseException:
            os.close(self.master)
            raise
        finally:
            os.close(slave)
        self.bytes = 0

    def read(self, timeout):
        if not select.select([self.master], [], [], max(timeout, 0))[0]:
            return b""
        try:
            data = os.read(self.master, 65536)
        except OSError as error:
            if error.errno == errno.EIO:
                data = b""
            else:
                raise
        if not data:
            raise RuntimeError(f"TUI exited: {self.process.poll()}")
        self.bytes += len(data)
        if b"\x1b[6n" in data:
            os.write(self.master, b"\x1b[1;1R")
        return data

    def pump(self, seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            self.read(min(.1, deadline - time.monotonic()))

    def latency(self):
        token = b"latencyprobe"
        start = time.monotonic()
        os.write(self.master, b"/" + token)
        received = b""
        try:
            while time.monotonic() - start < 2:
                received += self.read(.05)
                if token in ANSI.sub(b"", received):
                    return (time.monotonic() - start) * 1000
            raise RuntimeError("search prompt did not echo the latency token")
        finally:
            os.write(self.master, b"\x1b")
            self.pump(.1)

    def close(self):
        try:
            if self.process.poll() is None:
                os.write(self.master, b"\x1b")
                self.pump(.1)
                os.write(self.master, b"q")
                deadline = time.monotonic() + 3
                while self.process.poll() is None and time.monotonic() < deadline:
                    try:
                        self.read(.05)
                    except RuntimeError:
                        break
                if self.process.poll() is None:
                    self.process.terminate()
                    try:
                        self.process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        self.process.kill()
        finally:
            if self.process.poll() is None:
                self.process.kill()
            self.process.wait()
            self.restored_termios = termios.tcgetattr(self.master) == self.initial_termios
            os.close(self.master)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--label", default="working-tree")
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--windows", type=int, default=3)
    parser.add_argument("--warmup", type=float, default=3)
    parser.add_argument("--rows", type=int, default=32)
    parser.add_argument("--cols", type=int, default=160)
    parser.add_argument("--htop", action="store_true")
    options = parser.parse_args()
    if options.seconds <= 0 or options.windows < 1:
        parser.error("positive seconds and at least one window required")
    binary = options.binary.resolve()
    args = ["-d", "10", "-s", "PERCENT_CPU"] if options.htop else ["-d", "1000"]
    results = []
    with tempfile.TemporaryDirectory(prefix="gander-pty-benchmark-") as directory:
        session = Session(binary, args, directory, options.rows, options.cols)
        try:
            session.pump(options.warmup)
            for _ in range(options.windows):
                cpu, _ = counters(session.process.pid)
                sent = session.bytes
                start = time.monotonic()
                session.pump(options.seconds)
                elapsed = time.monotonic() - start
                after, rss = counters(session.process.pid)
                results.append(dict(seconds=elapsed, cpu_percent=(after-cpu)/elapsed*100,
                                    rss_bytes=rss, terminal_bytes=session.bytes-sent,
                                    input_latency_ms=session.latency()))
        finally:
            session.close()
        if not session.restored_termios:
            raise RuntimeError("TUI did not restore its terminal attributes")
    report = dict(label=options.label, binary=str(binary), sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                  platform=platform.platform(), viewport=[options.cols, options.rows], args=args,
                  config=CONFIG if not options.htop else "fresh isolated htoprc; CPU descending; default hidden user threads",
                  process_count=len(subprocess.check_output(["ps", "-A", "-o", "pid="]).splitlines()),
                  windows=results, medians={key: statistics.median(r[key] for r in results)
                                          for key in ("cpu_percent", "rss_bytes", "terminal_bytes", "input_latency_ms")})
    options.output.parent.mkdir(parents=True, exist_ok=True)
    options.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
