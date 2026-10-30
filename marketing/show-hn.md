# Show HN 文案

发帖方式：新帖，类型选 **Show HN**，URL 填仓库地址
`https://github.com/pandasir/gander`，下面这段贴进 Text（Text 不是必填，
但 Show HN 带 2–3 段个人陈述的转化明显更好）。

时间：周二至周四，北京时间 21:00–24:00（美东上午），避开周末。

标题（选一个，不要改引号格式）：

    Show HN: Gander – A cross-platform process viewer that focuses on one process at a time

    Show HN: Gander – An htop alternative whose layout follows the process you're on

---

Hi HN! I built gander, an interactive process viewer for the terminal —
like htop, for Linux, macOS and FreeBSD:

https://github.com/pandasir/gander

It keeps most of htop's keybindings, but the default layout is deliberately
not htop's. htop spends the whole screen on the process list; gander keeps a
live inspector beside the table that follows the process you're on — full
command line, state, memory, uptime — so you don't have to open another pane
or run `ps` to answer "what is this process actually doing?" Pressing `L`
flips back to the classic full-width htop layout, so nothing is taken away.

A couple of things I kept from htop on purpose:

- The per-core CPU meters pack the same way htop does: one row per core up to
  eight, then 2/4/8 per row, so a 128-core machine takes 9 header rows instead
  of 65.
- Sampling runs in a background thread and the renderer reads a snapshot, so
  the UI doesn't stutter when /proc gets busy.

There is also a headless mode for scripts and cron: `--json`, `-n`, `--sort`,
`--filter`, `--limit`, no TUI required. On Linux, PSS, PSI pressure,
zram/zswap, GPU and per-process latency are available as optional columns or
header meters; each platform reads its own interfaces (`/proc`,
sysctl/Mach/libproc on macOS, sysctl/libkvm on FreeBSD) — no procfs mount
needed on FreeBSD.

Install: `brew install pandasir/tap/gander` (macOS) or `cargo install
gander-bin` (any platform with Rust; the installed command is `gander`).
Signed apt/dnf repositories and per-platform archives are on the releases
page. Written in Rust with ratatui and crossterm, GPL-2.0-or-later, and there
is no telemetry anywhere — the only analytics is a cookieless page counter on
the docs site.

Feedback very welcome, especially on the panels layout — that's the part I
expect people to either love or want to turn off (it's one keypress back to
htop). Comparison pages if you want the details:
https://pandasir.github.io/gander/vs-htop.html

---

评论应对备忘：

- 问「为什么不用 btop/bottom」→ 讲差异：它们也是全屏列表，gander 的差异是
  单进程检视 + 保持 htop 键位；别贬低别人。
- 问 Windows 支持 → 如实说：没有计划，后端三平台已经够维护。
- 收到 bug → 当场记 issue，链接贴出来，让对方看到反馈被认真对待。
- 第一小时不拉票不发推，HN 对互相拉票很敏感。
