# Reddit 文案 — r/rust

两条路，按当天情况选一条：

## 路线 A：weekly 帖评论（稳妥，无被删风险）

在每周的 "What's everyone working on this week?" 置顶帖下评论：

This week I've been polishing gander, a cross-platform interactive process
viewer (htop alternative) for Linux, macOS and FreeBSD — Rust + ratatui +
crossterm, GPL-2.0.

The twist vs htop: the default layout keeps a live inspector next to the
process table that follows the selected process (full command line, state,
memory, uptime), and `L` switches to the classic full-width htop layout.
Sampling runs on a background thread and the renderer reads snapshots, and
there's a headless `--json` mode for scripts. The Linux backend can surface
PSS, PSI, zram, GPU and latency stats as columns or header meters.

Repo: https://github.com/pandasir/gander · docs and htop/btop/bottom
comparisons: https://pandasir.github.io/gander/

Feedback very welcome — especially from FreeBSD users, that backend gets the
least real-world exercise.

## 路线 B：独立帖（等有里程碑再发，比如 0.7.0）

标题：

    gander: a cross-platform htop-like process viewer in Rust (Linux/macOS/FreeBSD), now with a headless JSON mode

正文直接复用 `reddit-commandline.md` 的正文，但开头加一句技术向的：
"Technically it's ratatui + crossterm over three per-platform backends
(/proc, sysctl/Mach/libproc, sysctl/libkvm); sampling runs on a background
thread so the render loop only reads snapshots."
