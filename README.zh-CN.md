# gander

> 用 Rust 写的跨平台交互式进程查看器 —— 对你的系统 `take a gander`（瞧一眼）。

[English](README.md) | **简体中文**

`gander` 是 [htop](https://github.com/htop-dev/htop) *候选功能集* 的重新实现，
用立即模式 TUI（ratatui）和 Elm 风格的 state/update/view 循环构建，而不是 htop
那套保留模式的 Panel 栈。

**当前状态：M0 骨架。** 可以编译，232 个测试（macOS）/ 215 个测试（Linux）通过，
可在 **Linux 和 macOS** 上运行。
设计见 [docs/DESIGN.md](docs/DESIGN.md)，进度见 [docs/ROADMAP.md](docs/ROADMAP.md)，
各平台细节见 [docs/PORTABILITY.md](docs/PORTABILITY.md)。
（文档目前均为英文。）

## 界面

默认界面**不是** htop 的布局。它是纵向的系统栏、更精简的进程表（CPU 与内存数字
背后画有比例条），以及一个常驻的检视面板 —— 不用离开列表就能看到折行后的完整
命令行和精确数值。按 `L` 可切回经典的 htop 布局，该布局同样完整支持。
设计理由见 [docs/LAYOUT.md](docs/LAYOUT.md)。

## 为什么不照着 1:1 移植？

htop 是一套保留模式的控件工具箱（`Object` → 带焦点栈的 `Panel`），光栅化到
ncurses 窗口里。ratatui 是立即模式的：每个 tick 你重建整个帧，由它帮你 diff
单元格。把 htop 的 `Panel` / `ScreenManager` / `Meter` 虚表设计搬到 ratatui 上，
是在跟框架对着干，而且没有任何收益。

所以：**把 htop 的功能清单当作需求文档，丢掉它的代码结构。**
但保留它的*按键绑定* —— 用户迁移成本为零。

| | htop（C） | gander（Rust） |
|---|---|---|
| 渲染 | ncurses，保留模式 Panel | ratatui + crossterm，立即模式 |
| 控件 | `Object` 虚表 + 焦点栈 | `enum Screen` + 各屏独立的 `handle_key()` |
| 数据 | 单线程扫描 `/proc` | 并行扫描（rayon）+ 后台采样线程 |
| 平台 | Linux、BSD、macOS | Linux + macOS，共用 `PlatformSampler` trait |
| 配置 | `htoprc`，手写解析器 | TOML，并提供 `--import-htoprc` |
| 测试 | luajit spec、valgrind 脚本 | fixture + proptest + `TestBackend` 快照 |

## 范围

| 里程碑 | 内容 |
|---|---|
| **M0** | 进程表、排序、过滤、树状视图、kill / renice，Linux 与 macOS |
| **M1** | meter 系统（CPU / 内存 / swap / 负载 / 网络 / 磁盘 IO）、可配置列、配置持久化 |
| **M2** | 屏幕标签页、增量搜索、多选批量操作、affinity / ioprio、cgroup 列 |
| **M3** | 高价值的 Linux 深度功能：通过 `smaps_rollup` 获取 PSS/EPSS、PSI、Zram/Zswap、GPU、容器识别 |
| **M4** | BSD，复用同一个 `Platform` trait（macOS 已在 M0 完成） |

v1 明确不做：`pcp-htop` 那种动态指标引擎（运行时通过指标 API 定义列 / meter /
屏幕）。那是另一个产品。

## 与 htop 的有意差异

1. **绝不阻塞界面。** 采样在独立线程运行，渲染端读取 `Arc` 快照。htop 在
   `smaps_rollup` 读取数千个 PID 时会卡住。
2. **`--json` 输出。** core crate 不依赖任何 UI，所以输出 JSON 的批处理模式
   几乎零成本 —— 而这是 htop 在架构上无法提供的能力，便于对接监控系统。
3. **缓存不可变的进程数据。** `cmdline`、`exe`、`comm`、`cgroup`、`uid`
   对存活的进程不会变化，因此按 `(pid, starttime)` 缓存。
4. **TOML 作为配置格式 + htoprc 导入器**，而非直接用 htoprc。
5. **仅在正常退出时保存**，与 htop 一致（`htop.1`：任何信号都会丢失配置）。
   这是有意为之，不要"优化"掉。

## 对性能的预期

在这里，语言选择本身不带来任何收益 —— 这个负载是系统调用和内存分配密集型的，
不是 CPU 密集型。现实的目标：

| 场景 | 相对 htop 的目标 |
|---|---|
| 默认 1.5 秒刷新，2000 个进程 | **持平**（±1 ms 属噪声） |
| 100 毫秒刷新，10000 个进程 | **快 1.5–2.5 倍**（rayon + 缓存 + 独立线程） |
| 重算期间的输入延迟 | **明显更好**（非阻塞） |

细节与预算拆解见 [docs/DESIGN.md § 9 Performance](docs/DESIGN.md#9-performance)。

## 构建

```bash
cargo build --release      # target/release/gander
cargo test                 # macOS 232 个，Linux 215 个
cargo clippy --all-targets # 零警告
cargo bench -p gander-core # 扫描耗时
```

支持 Linux（`/proc`）和 macOS（`libproc` + `sysctl` + Mach）。两个后端都针对
真实操作系统验证过，见 [docs/PORTABILITY.md](docs/PORTABILITY.md)。其他 unix
目标可以编译 `/proc` 后端，但没有可用的数据源，TUI 会拒绝启动。
`crates/core/tests/fixtures/proc/` 下的测试样例是真实的 `/proc` 快照，
因此 Linux 读取路径可以在任何 unix 上测试。

## 试用

```bash
cargo run --release                          # TUI
cargo run --release -- --json --limit 10     # 批处理模式
cargo run --release -- -n 5 -d 1000          # 采样 5 次，每次间隔 1 秒
cargo run --release -- --import-htoprc ~/.config/htop/htoprc --save
```

按键与 htop 完全一致：[docs/KEYS.md](docs/KEYS.md)。

## 许可证

GPL-2.0-or-later，与 htop 保持一致。行为、按键绑定和列语义都源自 htop；
请保持许可证兼容，并注明出处。
