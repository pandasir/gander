# V2EX 文案

节点：主发 **/go/linux**（或 /go/programmer），隔一天可再发 /go/macos，
换个角度写（macOS 用户痛点：装 htop 麻烦、Activity Monitor 不在终端里）。
不要同一天多节点同文。

标题：

    自己写了个 htop 替代品：Rust 的跨平台进程查看器 gander，求反馈

正文：

一直觉得 htop 的逻辑是「看所有进程」，但实际用的时候，大部分时间是在盯
「某一个进程」——找到它、确认它在干什么、看它的内存涨没涨。每次都是列表里
选中，然后另开一个 tmux 窗格跑 ps 或者进容器看，很割裂。

所以我写了 gander（顺便练 Rust）：

- 默认布局叫 panels：左边一列紧凑的系统信息（CPU / 内存 / 负载 / 网络吞吐），
  中间进程表，右边是跟着选中进程走的实时检视面板——完整命令行、状态、内存、
  运行时间，不用离开列表
- 按 L 一键切回经典的全宽 htop 布局，键位沿用 htop 的，上手没有学习成本
- 每核 CPU 表头的排法和 htop 一致：8 核以内每行一个，之后每行 2/4/8 个，
  128 核的机器表头只占 9 行
- 采样在后台线程跑，渲染只读快照，/proc 很忙的时候界面不卡
- 脚本场景可以完全脱离 TUI：gander --json -n 20 --sort mem --filter nginx
- Linux 上还能看 PSS、PSI、zram/zswap、GPU、延迟统计、容器识别，做成可选列
  或表头 meter
- 平台支持 Linux / macOS / FreeBSD，各平台读各自的接口（/proc、sysctl/Mach、
  sysctl/libkvm），FreeBSD 不需要挂 procfs
- Rust + ratatui 实现，GPL-2.0，二进制无遥测

安装：

    brew install pandasir/tap/gander     # macOS
    cargo install gander-bin             # 有 Rust 就行，命令叫 gander
    # deb / rpm 有签名仓库，release 页有各平台压缩包

仓库：https://github.com/pandasir/gander
文档站（有 vs htop / btop / bottom 的对比页）：https://pandasir.github.io/gander/

30 秒上手：装好直接跑 gander，方向键选进程，按 L 切布局，q 退出。

求反馈，特别是：1) panels 布局你觉得是加分还是多余；2) FreeBSD 用户的实际
使用情况（测试最少的就是这个后端）。bug 直接开 issue 就行，都会回。

---

评论应对：有人问和 btop/bottom 区别 → 同 Show HN 的口径，讲单进程检视 +
htop 键位，不贬低。有人吐槽 GPL → 商用自有部署不受影响，一句话带过即可。
