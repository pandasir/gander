# gander

**English** | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [한국어](README.ko.md)

> A cross-platform interactive process viewer written in Rust.

gander shows what is running on your system and how much it costs. It supports
**Linux, macOS and FreeBSD**, keeps most htop keybindings, and provides a
process-focused layout with a persistent inspector.

## Features

- **Process-first layout.** The default `panels` layout combines a compact system
  column, a wide process table and a live inspector. Press `L` to switch to the
  classic `htop` layout.
- **Inspect without leaving the list.** Command line, state, memory and runtime
  update with the selected process.
- **Responsive sampling.** Sampling runs in the background while the renderer
  reads a snapshot.
- **Scriptable output.** `--json`, `-n`, `--sort`, `--filter` and `--limit` work
  without the TUI.
- **Linux depth.** PSS/EPSS, PSI, zram/zswap, GPU, delay accounting and container
  identity are available as optional columns or meters.
- **Native backends.** Linux uses `/proc`, macOS uses `sysctl`/Mach/libproc, and
  FreeBSD uses `sysctl`/`libkvm`.

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

`.deb` and `.rpm` packages are published beside the archives for Linux x86_64
and arm64; see [Debian/Ubuntu and Fedora/RHEL](#debianubuntu-and-fedorarhel).

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
| Arch Linux (AUR) | `yay -S gander` |

The crates.io package is named `gander-bin` because `gander` is already taken by
an unrelated crate; the installed command is still `gander`.

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

`panels` is the default layout: a system column on the left, the process table in
the center and an inspector on the right. `htop` is the classic full-width
layout. `L` switches between them and the choice is saved.

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

Search and filter history is stored under `$XDG_STATE_HOME/gander/` (fallback:
`~/.local/state/gander/`).

Available themes: `gander`, `default`, `monochrome`, `midnight`, `classic`,
`verdant`, `paper` and `quiet`. Choose one from `F2` -> Colors or set
`color_scheme` in the config.

## Linux depth

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
| [docs/KEYS.md](docs/KEYS.md) | Complete keymap and batch mode |
| [docs/LAYOUT.md](docs/LAYOUT.md) | `panels` layout and terminal degradation |
| [docs/PORTABILITY.md](docs/PORTABILITY.md) | Backend matrix and platform limits |
| [docs/DESIGN.md](docs/DESIGN.md) | Architecture and data model |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Completed milestones and future work |
| [docs/SIGNING.md](docs/SIGNING.md) | The repository signing key and its rotation |
| [CHANGELOG.md](CHANGELOG.md) | Release history |

## Contributing

Bug reports, platform observations and focused pull requests are welcome. Start
with [CONTRIBUTING.md](CONTRIBUTING.md). Report security vulnerabilities
privately according to [SECURITY.md](SECURITY.md); do not open a public issue.

## License

GPL-2.0-or-later. Behaviour, keybindings and column semantics are derived from
htop; keep the license compatible and credit it.
