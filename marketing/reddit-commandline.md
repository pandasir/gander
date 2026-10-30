# Reddit 文案 — r/commandline

单独发帖，角度和 Show HN 区分开：更口语，先说解决了什么，再列特性。
帖前读一遍 sub 规则（r/commandline 允许项目展示，但反感纯广告腔）。

标题：

    gander: a cross-platform (Linux/macOS/FreeBSD) htop-like process viewer with a live inspector for the selected process [Rust, GPL-2.0]

正文：

I kept hitting the same workflow gap with htop: I find the process, then I
leave the table to figure out what it actually is — another pane, `ps`, or a
browser tab. So I wrote gander. Same keybindings as htop (it uses most of
them), but the default layout puts a live inspector next to the process table
that follows your selection: full command line, state, memory, uptime. One
keypress (`L`) and you're back in the classic full-width htop layout.

Works on Linux, macOS and FreeBSD — each backend reads its own interfaces
(`/proc`, sysctl/Mach/libproc, sysctl/libkvm), so FreeBSD doesn't need procfs
mounted. Written in Rust with ratatui/crossterm, GPL-2.0, no telemetry.

Things you might care about:

- htop-style per-core meters that pack 2/4/8 per row on big machines (128
  cores = 9 header rows)
- headless mode for scripts: `gander --json -n 20 --sort mem --filter nginx`
- on Linux: PSS, PSI pressure, zram/zswap, GPU and latency columns, plus
  container identification
- `F2` setup to rearrange meters/columns like htop

Install:

    brew install pandasir/tap/gander     # macOS
    cargo install gander-bin             # any platform with Rust

Repo: https://github.com/pandasir/gander · docs and live demo:
https://pandasir.github.io/gander/

Happy to take feature requests and bug reports here or on GitHub. If you're
on FreeBSD especially — that backend gets the least real-world testing and
I'd genuinely like to hear from you.

---

注意：正文里的仓库链接要写
`https://github.com/pandasir/gander`（上面留的 site 链接是给 demo 展示用的，
发帖前确认两处链接都正确，别把 repo 链接写成 github.io）。
