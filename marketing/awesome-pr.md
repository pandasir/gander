# 收录站与 awesome 列表

## 1. Terminal Trove（最对口，优先做）

TUI 工具专门收录站。到 https://terminaltrove.com/submit/ 提交，填：

- Name: gander
- Homepage / Repo: https://github.com/pandasir/gander
- Description:
      A cross-platform interactive process viewer for the terminal. Keeps
      htop's keybindings and puts a live inspector beside the process table,
      showing the selected process's command line, state, memory and uptime.
      Linux, macOS, FreeBSD. Rust, GPL-2.0.
- Tags: tui · process-viewer · system-monitor · htop · terminal

收录后它会出现在 "htop" 相关搜索里，是他们站内长期导流的入口。

## 2. rust-unofficial/awesome-rust

PR 到 Applications 分区（找个 system/monitoring 相关的小节，没有就放
Applications 末尾）。条目按他们现有格式：

    * [gander](https://github.com/pandasir/gander) — Cross-platform interactive process viewer (htop alternative) for Linux, macOS and FreeBSD. [![ci](https://github.com/pandasir/gander/actions/workflows/ci.yml/badge.svg)](https://github.com/pandasir/gander/actions/workflows/ci.yml)

PR 描述一句话即可："Adds gander, a cross-platform (Linux/macOS/FreeBSD)
interactive process viewer in Rust, to Applications." 先确认没有重复条目。

## 3. 其他可选（低优先级）

- **AlternativeTo**：建条目挂 "htop alternative"，填这个标签比填平台更重要，
  因为用户是从 htop 页面找过来的。
- **Slant / libhunt**：顺手建即可，不指望量。
- **awesome-tuis 类列表**：搜 GitHub 上 awesome-tui / awesome-tuis，看哪个
  还活跃（最近有 commit）再提，僵尸列表不值得。

## 纪律

同一周内同一个人向多个列表提 PR 没问题（每个列表独立审），但条目描述要按
各列表格式重写，不要复制粘贴带 markdown 徽章的版本到不支持的地方。
