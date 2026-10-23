# gander

[![CI](https://github.com/pandasir/gander/actions/workflows/ci.yml/badge.svg)](https://github.com/pandasir/gander/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pandasir/gander?label=release)](https://github.com/pandasir/gander/releases)
[![Downloads](https://img.shields.io/github/downloads/pandasir/gander/total?label=downloads)](https://github.com/pandasir/gander/releases)
[![crates.io](https://img.shields.io/crates/v/gander-bin?label=crates.io)](https://crates.io/crates/gander-bin)
[![License: GPL-2.0-or-later](https://img.shields.io/badge/license-GPL--2.0--or--later-blue)](LICENSE)
[![Platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20FreeBSD-lightgrey)](#platform-support)

**English** | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [한국어](README.ko.md)

> **gander** is a cross-platform interactive process viewer for Linux, macOS and
> FreeBSD, written in Rust. Think htop, with a layout that shows you one process
> instead of all of them at once.

![gander in the panels layout, with a process selected](assets/screenshot-panels.png)

```bash
brew install pandasir/tap/gander     # macOS
cargo install gander-bin             # any platform with Rust
git clone https://github.com/pandasir/gander && cd gander/packaging/aur && makepkg -si   # Arch Linux
```

![Recording: selecting a process, tree view, then L for the htop layout](assets/demo.gif)

gander shows what is running on your system and how much it costs. It runs on
Linux, macOS and FreeBSD and keeps most of htop's keybindings.

## Features

gander starts in the `panels` layout: a compact system column on the left, the
process table in the middle and a live inspector on the right. The inspector
follows the process you select, so its command line, state, memory and runtime are
there without leaving the list. Press `L` for the classic full-width `htop` layout.

The `htop` layout puts the cores on the same ladder htop does — one per line up to
eight cores, then 2, 4 and 8 per line — so a 128-core machine spends nine header
rows on CPU, not 65. `F2` → Header picks `LeftCPUs8` and friends explicitly.

Sampling runs in the background while the renderer reads a snapshot. For scripts,
`--json`, `-n`, `--sort`, `--filter` and `--limit` work without the TUI.

On Linux there is more to read: PSS/EPSS, PSI, zram/zswap, GPU, delay accounting
and container identity are available as optional columns or meters. Each backend
reads the platform's own interface: `/proc` on Linux, `sysctl`/Mach/libproc on
macOS and `sysctl`/`libkvm` on FreeBSD.

## Platform support

| Capability | Linux | macOS | FreeBSD |
|---|:---:|:---:|:---:|
| Process, CPU and memory view | Yes | Yes | Yes |
| Host network throughput | Yes | Yes | Yes |
| Host disk throughput (`DiskIO`) | Yes | - | Yes |
| Container columns | Yes | - | - |
| PSS / GPU / delay columns | Yes | - | - |
| CPU affinity editing | Yes | Hint only | Yes |

Other Unix targets may compile but have no live backend. See
[docs/PORTABILITY.md](docs/PORTABILITY.md) for details.

## How it compares

If you are shopping for an htop alternative, the short version:

| | gander | htop | btop | bottom |
|---|:---:|:---:|:---:|:---:|
| Terminal UI | Yes | Yes | Yes | Yes |
| Linux / macOS / FreeBSD | Yes | Yes | Partial | Yes |
| Reads htop's keybindings | Yes | - | No | No |
| Imports an existing `htoprc` | Yes | - | No | No |
| Keeps the selected process on screen | Yes | No | No | No |
| Layout that is not htop's | `panels` | - | Yes | Yes |
| `--json` batch mode | Yes | No | No | No |

The difference that matters in daily use is the last two rows. htop, btop and
bottom all answer "what is running" by showing you every process at once, and
then the details of the one you care about in a separate screen or not at all.
gander puts the inspector beside the table, so selecting a row answers the
question without leaving the list. `L` gives you htop's arrangement if that is
what you came for, and the keybindings stay where htop put them.

## Install

### Prebuilt binaries

Download the archive for your platform from
[GitHub Releases](https://github.com/pandasir/gander/releases).

| Platform | Target |
|---|---|
| macOS, Apple Silicon | `aarch64-apple-darwin` |
| macOS, Intel | `x86_64-apple-darwin` |
| Linux, x86_64, glibc | `x86_64-unknown-linux-gnu` |
| Linux, x86_64, static | `x86_64-unknown-linux-musl` |
| Linux, arm64, glibc | `aarch64-unknown-linux-gnu` |
| Linux, arm64, static | `aarch64-unknown-linux-musl` |
| FreeBSD | `x86_64-unknown-freebsd` |

The release also publishes `.deb` and `.rpm` packages next to the archives for
Linux x86_64 and arm64; see
[Debian/Ubuntu and Fedora/RHEL](#debianubuntu-and-fedorarhel).

```bash
tar -xzf gander-<version>-<target>.tar.gz
./gander-<version>-<target>/gander
```

To install it on your `PATH`:

```bash
mkdir -p ~/.local/bin
install -m755 gander-<version>-<target>/gander ~/.local/bin/gander
```

The musl build is static and does not depend on the host's glibc. Each archive
has a `.sha256` checksum file.

### Debian/Ubuntu and Fedora/RHEL

Both distributions have a signed repository, so `apt` and `dnf` install and
upgrade gander like any other package.

```bash
# Debian, Ubuntu, Linux Mint, …
sudo mkdir -p /usr/share/keyrings
curl -fsSL https://pandasir.github.io/gander/gander.gpg \
  | sudo gpg --dearmor -o /usr/share/keyrings/gander.gpg
echo "deb [signed-by=/usr/share/keyrings/gander.gpg] https://pandasir.github.io/gander stable main" \
  | sudo tee /etc/apt/sources.list.d/gander.list
sudo apt update && sudo apt install gander

# Fedora, RHEL, Rocky, Alma, openSUSE, …
sudo rpm --import https://pandasir.github.io/gander/RPM-GPG-KEY-gander
sudo curl -fsSL https://pandasir.github.io/gander/gander.repo \
  -o /etc/yum.repos.d/gander.repo
sudo dnf install gander
```

Upgrades then arrive with `apt upgrade` / `dnf upgrade`. The repositories are
signed and the clients verify them: `apt` checks `InRelease`, and `dnf` runs with
`gpgcheck=1` and `repo_gpgcheck=1`.

If you would rather not add a repository, the release still ships the packages as
files:

```bash
sudo apt install ./gander_<version>-1_amd64.deb      # arm64 on ARM machines
sudo dnf install ./gander-<version>-1.x86_64.rpm     # aarch64 on ARM machines
```

`SHA256SUMS` beside them covers every package.

### Package managers

| Method | Command |
|---|---|
| Debian, Ubuntu | `sudo apt install gander` |
| Fedora, RHEL, Rocky | `sudo dnf install gander` |
| Homebrew | `brew install pandasir/tap/gander` |
| crates.io | `cargo install gander-bin` |
| Arch Linux | `git clone https://github.com/pandasir/gander && cd gander/packaging/aur && makepkg -si` |

The crates.io package is named `gander-bin` because `gander` is already taken by
an unrelated crate; the installed command is still `gander`.

There is no AUR package. AUR account registration has been closed since the
[malicious-packages incident](https://archlinux.org/news/active-aur-malicious-packages-incident/)
in June 2026, and Arch's DevOps team has said there is no manual process to
create one, so the package cannot be pushed. The Arch build is the same
`PKGBUILD` that would have been published, so `makepkg -si` above builds the
identically packaged binary from source. When registration reopens, the package
will be pushed and this section will name it.

### From source

Requires **Rust 1.89+**. No ncurses or libcurses development headers are needed.

```bash
git clone https://github.com/pandasir/gander
cd gander
cargo install --locked --path crates/bin
```

Or run it without installing:

```bash
cargo run --release -p gander-bin
```

## Quick start

```bash
gander                                     # interactive TUI
gander --delay 500                         # sample every 500 ms
gander -n 5 -d 1000                        # five text samples
gander --json --limit 10                   # one JSON sample
gander --sort PERCENT_MEM --limit 20       # top 20 by memory
gander -F "python|node"                    # filter command lines
gander --import-htoprc ~/.config/htop/htoprc --save
```

Filter terms are case-insensitive fixed strings separated by `|`; they are not
regular expressions. Run `gander --help` for all options.

## Interface and keys

`panels` is the default layout and `htop` is the classic full-width one. `L`
switches between them, and gander remembers the choice.

| `panels` (default) | `htop` (`L`) | `t` tree view |
|---|---|---|
| ![panels layout](assets/screenshot-panels.png) | ![htop layout](assets/screenshot-htop.png) | ![tree view](assets/screenshot-tree.png) |

| Key | Action |
|---|---|
| `F1` / `h` / `?` | Help |
| `F2` | Setup |
| `F3` / `/` | Search |
| `F4` / `\` | Filter |
| `F5` / `t` | Tree view |
| `F6` / `<` `>` | Change sort |
| `F9` / `k` | Send signal |
| `F10` / `q` | Quit |

Keys that differ from htop:

| Key | gander |
|---|---|
| `I` | Cycle the selected header meter's mode |
| `+` / `-` | Faster / slower sampling |
| `L` | Switch layout |
| `Shift+Left` / `Shift+Right` | Move the header meter cursor |

htop's `I` inverts the sort order; in gander use `F6` / `<` `>` / `.` instead.
See [docs/KEYS.md](docs/KEYS.md) for the complete keymap.

## Configuration

Configuration is TOML at `$GANDER_CONFIG`, otherwise
`$XDG_CONFIG_HOME/gander/config.toml` (fallback:
`~/.config/gander/config.toml`). Each tab keeps its own columns and sort order;
header meters and their display modes are configurable. Settings are saved on a
clean exit. A missing file uses defaults; unreadable, malformed or invalid settings
produce an error without overwriting the file. Saves replace the file atomically.

`system_cores_max` caps the per-core grid in the `panels` system column. `0`, the
default, draws every core the column has room for; on a short terminal the grid
stops early and the label reads `64/128 cores` so a truncated grid cannot be
mistaken for a 64-core machine. `F2` -> Display cycles the same setting.

gander stores search and filter history under `$XDG_STATE_HOME/gander/`
(fallback: `~/.local/state/gander/`).

Available themes: `gander`, `default`, `monochrome`, `midnight`, `classic`,
`verdant`, `paper` and `quiet`. Choose one from `F2` -> Colors or set
`color_scheme` in the config.

## Linux-specific data

- `M_PSS`, `M_EPSS`, `M_PRIVATE`, `M_SWAP`: read on demand from `smaps_rollup`;
  unmeasured values show `-`, not `0`.
- `GPU%`, `GPU_TIME`: read from `drm-usage-stats` without a vendor SDK.
- `CPUD%`, `IOD%`, `SWPD%`: TASKSTATS delay accounting; requires
  `CAP_NET_ADMIN`.
- `CONTAINER`, `CGROUP`: cgroup path combined with PID namespace detection.
- `PSI`, `Zram`, `Zswap`, `HugePages`: optional header meters.

Use `F2` -> Columns or `F2` -> Header to enable them. See
[docs/DESIGN.md](docs/DESIGN.md) and [docs/PORTABILITY.md](docs/PORTABILITY.md)
for implementation details.

## Development

The workspace is split into four crates:

| Crate | Responsibility |
|---|---|
| `gander-core` | Platform sampling, parsing, process model and actions |
| `gander-config` | TOML settings, history and `htoprc` import |
| `gander-tui` | Rendering, layouts, input and runtime |
| `gander-bin` | CLI, batch output and the executable |

```bash
cargo build --locked
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo bench -p gander-core
```

Live macOS tests read the system process table and may need to run outside a
restricted sandbox. See [CONTRIBUTING.md](CONTRIBUTING.md) for the full
development and review workflow.

## Documentation

| Document | Contents |
|---|---|
| [assets/README.md](assets/README.md) | The screenshots and how to regenerate them |
| [docs/KEYS.md](docs/KEYS.md) | Complete keymap and batch mode |
| [docs/LAYOUT.md](docs/LAYOUT.md) | `panels` layout and terminal degradation |
| [docs/PORTABILITY.md](docs/PORTABILITY.md) | Backend matrix and platform limits |
| [docs/DESIGN.md](docs/DESIGN.md) | Architecture and data model |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Completed milestones and future work |
| [docs/SIGNING.md](docs/SIGNING.md) | The repository signing key and its rotation |
| [CHANGELOG.md](CHANGELOG.md) | Release history |

## FAQ

**Is gander an htop replacement?**
Yes and no. It reads the same `/proc` data and keeps htop's keys, so it is a drop-in
for the way you already work; `--import-htoprc` copies your htop settings over.
But the default layout is deliberately not htop's, because a full-width table with
the details on a second screen is not the only way to read a process list.

**Does it work on Windows?**
No. The three backends are `/proc` (Linux), `sysctl`/Mach/libproc (macOS) and
`sysctl`/`libkvm` (FreeBSD). There is no Windows backend, and the columns that
depend on Linux kernel interfaces (PSS, PSI, zram, delay accounting, containers)
are Linux only.

**Why is the crates.io package called `gander-bin`?**
Because `gander` on crates.io is an unrelated, long-yanked crate. The installed
command is still `gander`.

**Why does it not use ncurses?**
It renders with [ratatui](https://ratatui.rs) and [crossterm](https://github.com/crossterm-rs/crossterm),
so there is no ncurses development header to install before building.

**Can I use it in scripts?**
Yes. `gander --json`, `-n`, `--sort`, `--filter` and `--limit` run without the TUI
and print one row per process.

## Contributing

Bug reports, platform observations and focused pull requests are welcome. Start
with [CONTRIBUTING.md](CONTRIBUTING.md). Report security vulnerabilities
privately according to [SECURITY.md](SECURITY.md); do not open a public issue.

## License

GPL-2.0-or-later. gander takes its behaviour, keybindings and column semantics
from htop; keep the license compatible and credit it.
