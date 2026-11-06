# TUI Performance Checks

`tools/pty_benchmark.py` runs a release binary in a private pseudoterminal with an
explicit configuration and isolated state. It requires Python 3 and its standard
library, not Python packages or runtime dependencies in gander. It never changes
the caller's terminal mode or personal configuration. The child receives its own
controlling terminal and is stopped and reaped even when measurement fails.

Before implementation, preserve a release build outside the working build tree:

```sh
cargo build --release -p gander-bin
mkdir -p /tmp/gander-monitoring-baseline-6fb015b
cp target/release/gander /tmp/gander-monitoring-baseline-6fb015b/gander
python3 tools/pty_benchmark.py /tmp/gander-monitoring-baseline-6fb015b/gander \
  --label 6fb015b --output /tmp/gander-monitoring-baseline-6fb015b/results.json
```

After implementation, rebuild and repeat with the same viewport, workload, thread
visibility, and explicit columns/sort configuration:

```sh
python3 tools/pty_benchmark.py target/release/gander \
  --label changed --output /tmp/gander-monitoring-changed.json
```

Defaults are 160x32 cells, a three-second warm-up, and three 30-second windows at
one-second sampling. Each window records OS-accounted process CPU, RSS, terminal
output bytes, and search-input echo latency. Latency probes run outside the CPU
measurement window. Darwin uses libproc counters with the Mach timebase applied;
Linux uses `/proc/<pid>/stat`. Displayed process CPU in another monitor is not a
measurement of this tool's overhead. Record workload changes/process counts and
report variability rather than treating a single sample as a universal result.

Optional htop comparison uses a fresh isolated htoprc, CPU-descending sort, default
hidden user threads, and one-second sampling:

```sh
python3 tools/pty_benchmark.py /opt/homebrew/bin/htop --htop \
  --label htop-3.4.1 --output /tmp/htop-benchmark.json
```

Its classic layout is not equivalent to gander's panels composition; interpret
that comparison separately from the matched before/after gander measurements.
For harness smoke checks use `--seconds 1 --windows 1 --warmup 1`. Formal
acceptance must use at least three 30-second windows. All helper processes belong
to the harness; never send test actions to unrelated running applications.

## Reference Result

The `improve-process-monitoring-experience` change measured the original `6fb015b`
release and the changed release on macOS ARM64 using the default protocol above.
Three-window medians were:

| Metric | Original | Changed |
|---|---:|---:|
| OS-accounted CPU | 2.2412% | 0.8685% |
| Terminal bytes per 30 seconds | 118505 | 56567 |
| Search echo latency | 34.4029 ms | 1.4353 ms |
| RSS | 8.95 MiB | 11.14 MiB |

CPU decreased 61.25% and terminal output decreased 52.27%; RSS increased in this
run, so these results are not a memory-reduction claim. Background process counts
were 463 and 515, not a frozen workload. The change's `baseline.json`, `after.json`,
and `verification.md` retain all windows, settings, hashes, and limitations.

## Live Interaction Smoke Test

The workflow test uses the same private PTY plus `pyte` as a development-only
terminal emulator. Install it in a disposable virtual environment, not in the
application's runtime environment:

```sh
python3 -m venv /tmp/gander-pty-tests
/tmp/gander-pty-tests/bin/pip install pyte
/tmp/gander-pty-tests/bin/python tools/pty_smoke.py target/release/gander
```

It exercises fresh tabs, filtering and empty states, sorting and mouse header
clicks, tree folding across samples, owner selection, full-command scrolling and
resizing, sidebar toggles, both layouts, and signal-picker cancellation at
160x32, 120x24, 100x24, and 80x24. All process families it creates are terminated
and reaped. Normal exit must restore termios, mouse capture, and the alternate
screen. A closed-output-pipe initialization failure must also restore raw mode,
and quitting at a ten-second sampling delay must not wait for that interval.
Ctrl-C from setup must return 130 without writing configuration.
