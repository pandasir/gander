# gander

[English](README.md) | **简体中文**

> 用 Rust 写的跨平台交互式进程查看器 —— 对你的系统 `take a gander`（瞧一眼）。

gander 展示你的机器上正在运行什么、各自花了多少资源。它支持 **Linux 和 macOS**，
沿用 htop 的绝大部分按键（不同的两个列在[下文](#与-htop-不同的按键)），但屏幕布局与
htop 不同 —— 一个把空间花在进程列表上、而不是花在横向进度条上的布局。

## 安装

**预编译二进制**（macOS 和 Linux）在
[Releases 页面](https://github.com/pandasir/gander/releases)：

```bash
curl -LO https://github.com/pandasir/gander/releases/latest/download/gander-0.2.0-aarch64-apple-darwin.tar.gz
tar -xzf gander-0.2.0-aarch64-apple-darwin.tar.gz
./gander-0.2.0-aarch64-apple-darwin/gander
```

按平台选择对应的包（`aarch64`/`x86_64` 分别是 Apple Silicon 和 Intel 的
macOS；`x86_64-unknown-linux-musl` 是静态链接的二进制，任何发行版都能直接跑）。
每个包旁边都有对应的 `.sha256` 校验文件。

**从源码构建** — 需要 **Rust 1.89+**。不需要其他任何东西：不需要 libcurses，不需要
ncurses 开发头文件，也不需要 `rustc` 之外的 C 编译器。

这个下限不是装饰：`ratatui` 0.30 要求 1.88，`uuid` 要求 1.89，用更旧的工具链会
直接编译失败。CI 会按 `Cargo.toml` 声明的版本构建，因此这个数字不会在无人察觉的
情况下与现实脱节。

```bash
git clone https://github.com/pandasir/gander
cd gander
cargo install --path crates/bin
```

这会把 `gander` 装到 `~/.cargo/bin`。如果你用 rustup 装的 Rust，该目录已在 `PATH` 里。

不想安装、直接运行：

```bash
cargo run --release              # 启动 TUI
```

目前还没有发行版软件包；从源码构建或下载 release 二进制是当下受支持的安装方式。

## 用法

```bash
gander                                     # 交互式 TUI
gander --delay 500                         # 每 500 毫秒采样一次
gander -n 5 -d 1000                        # 输出 5 次文本采样，每秒一次
gander --json --limit 10                   # 输出一次 JSON，含 10 个进程
gander --sort PERCENT_MEM --limit 20       # 按内存排序取前 20
gander --filter "python|node"              # 只显示匹配的命令行
gander --import-htoprc ~/.config/htop/htoprc --save
```

完整参数见 `gander --help`；每个按键要么是 htop 的，要么列在
[docs/KEYS.md](docs/KEYS.md) 里。

## 与 htop 不同的按键

htop 有两个既有绑定在这里含义不同，htop 用户按下去会得到不一样的结果：

| 按键 | htop | gander |
|---|---|---|
| `I` | 反转排序方向 | 切换当前选中表头 meter 的显示模式 |
| `+` / `-` | 树状视图下展开 / 折叠子树 | 加快 / 放慢采样 |

`+`/`-` 只在树状视图中有歧义；在别处 htop 并未绑定它们，因此常见场景不受影响。
`I` 是实打实的冲突 —— 已经不再有反转排序的按键，要换顺序请用 `F6` / `<` `>` / `.`。

真正新增的按键：

| 按键 | 作用 |
|---|---|
| `L` | 切换布局：`panels` ⇄ `htop` |
| `I` | 切换当前选中表头 meter 的模式（text / bar / graph） |
| `Shift+Left` / `Shift+Right` | 移动表头 meter 光标 |

`H` **不是**新增按键 —— 它就是 htop 的"隐藏用户态线程"，且两个平台都支持，不只 Linux。

## 相比 htop 的优势

**把纵向空间留给进程列表。** htop 的 meter 是横向排列的，而它的高度随核数增长 ——
每个核占一行。gander 把系统状态放在纵向侧栏里，高度基本不变，其中每核负载用网格
显示，16 个核两行就能放下。

在本机（10 核）的 120x24 终端实测，两者都是刚启动、经真实终端模拟器抓屏：htop
显示 **11** 行进程，gander 显示 **21** 行 —— 接近两倍。核数越多差距越大，因为
只有 htop 那一侧会随核数变高。

**查看进程详情不必离开列表。** 在 htop 里，想知道某进程的命令行、确切内存占用、
或是处于睡眠还是运行状态，都要打开一个模态界面（`w`、`e`、`l`），从而丢失滚动
位置、已打的标记，以及你正在对比的上下文。gander 在列表旁常驻一个检视面板，
随选择移动而更新：折行后的完整命令行、用单词拼出的状态、以及窄列被四舍五入掉的
精确数值。模态界面依然保留，用于确实需要独立空间的功能 —— 环境变量、打开的文件、
文件锁、调用栈。

**排序结果是可见的。** `CPU%` 和 `MEM%` 列的数字背后画有比例条，比例基准是
**全表最大值**而非 100%。在机器空闲、最高进程只有 4% 时，按绝对比例画的条
在每一行都会不可见 —— 而那恰恰是你最需要判断"这些里面哪个最大"的时候。

**界面绝不卡住。** 采样在后台线程运行，渲染端读取快照，因此一次缓慢的扫描
不会冻结界面。这在 htop 最吃力的地方最关键：跨数千个 PID 读取 `smaps_rollup`。

**可以脚本化调用。** `--json`、`-n`、`--sort`、`--filter`、`--limit` 让它能用于
监控和 shell 管道，这是 htop 做不到的。

**在 macOS 上是正确的。** gander 通过 `sysctl(KERN_PROC_ALL)` 而不是
`proc_pidinfo` 读取完整进程表 —— 后者以普通用户身份只能看到约 60%。
运行状态取自 `proc_taskinfo`，因为当前 Darwin 的 `p_stat` 字段会把阻塞在
`read(2)` 里的进程报告为"可运行"。

**表头里有主机吞吐量。** htop 有按进程的 I/O 速率列，但没有主机级的网络或磁盘
meter。gander 的 `NetworkIO` 和 `DiskIO` 以迄今见过的最大速率为标尺（而非固定
上限），因此在 100 Mbit 和 10 Gbit 链路上都读得出来。

**它会告诉你*是哪个*容器。** htop 的 `CONTAINER` 列只标记"是否在容器里"；gander
显示 `docker:1f2e3d4c5b6a` 或 `kubepods:<pod-uid>`。识别同时用 cgroup 路径（给出
运行时与 id）和 PID namespace（唯一可靠的成员判定 —— 自带 cgroup namespace 的
容器会读到 `/`，只靠路径会把它当成宿主机）。

## 两种布局

gander 内置两套布局，按 `L` 切换，选择会被记住。

**`panels`（默认）** —— 纵向系统栏、带比例条的进程表、常驻检视面板：

```
  CPU    21.5%│ PID  CPU%  MEM%   RES  COMMAND             2032 iTerm2
  ████░░░░░░░░│ 2032  23.9  1.9   152M  /Applications/iTe…  /Applications/iTerm…
  10 cores    │ 24087 11.4  0.1   5.9M  gander              USER   panda
  ▂▂▂▂▃▂▁▁ ·  │ 6511   7.4  3.0   242M  Google Chrome       STATE  S sleeping
  MEM  5.5G/16G│ …                     …                    CPU    23.9%
  SWP  3.9G/5G│                                            MEM    1.9% 114M
  LOAD 2.69…  │                                            TIME   30:09.95
  UP     5h 3m│
  TASKS  552  │
  RUN      1  │
  THR   3370  │
```

每核网格是一核一格、按负载分色带，其下方的 sparkline 是聚合 CPU 的时间曲线。

**`htop`** —— 经典布局，完整支持。在配置里写 `layout = "htop"` 可设为默认。

设计理由（包括比例条为何按全表取值、窄终端上如何降级）见
[docs/LAYOUT.md](docs/LAYOUT.md)。

## 配置

TOML 格式，位于 `$XDG_CONFIG_HOME/gander/config.toml`（可用 `$GANDER_CONFIG` 覆盖）。
每个屏幕标签页各自保留自己的列与排序，表头 meter 及其显示模式也可配置。
仅在正常退出时写入，与 htop 一致。

已有的 htop 配置可以导入：

```bash
gander --import-htoprc ~/.config/htop/htoprc --save
```

搜索与过滤历史单独存放于 `$XDG_STATE_HOME/gander/{search,filter}_history`——
配置是你会复制到另一台机器的*决定*，历史只是使用这台机器的*副产品*。在
`F3` / `F4` 提示符里按 `Up` / `Down` 即可翻阅，重跑昨天的过滤只需要一个键。

### 容器列（Linux）

`F2` → Columns 在 Linux 上额外提供两列：

- **`CONTAINER`** —— 进程所属容器，格式 `运行时:id`，例如
  `docker:1f2e3d4c5b6a`、`kubepods:1a2b3c4d-...`、`lxc:web-1`。宿主进程显示 `-`。
- **`CGROUP`** —— 完整 cgroup 路径，id 不够用时查看。

识别同时使用 cgroup 路径**和** PID namespace，因为两者都不充分：路径能给出运行时
和 id，但在自带 cgroup namespace 的容器里只会读到 `/`；namespace 能说明"在容器里"
但说不出是哪个。namespace 判定为容器、路径又给不出名字时显示 `container:?`，而不是
编造一个 id。

### 表头 meter

`F2` → Header 可增删与排序；`I` 切换选中 meter 的显示模式（text / bar /
graph），每个 meter 的选择单独保存。除 htop 的整套（CPU、内存、swap、负载、
uptime、任务、时钟、日期、主机名、空白）之外，还有两个吞吐量 meter：

- **`NetworkIO`** —— 主机收发速率，已排除回环。
- **`DiskIO`** —— 来自 `/proc/diskstats` 的主机读写速率，只统计整块磁盘
  （分区与 `dm-*` 会被跳过，避免重复计数）。macOS 在没有 IOKit 的情况下没有
  主机级对应计数器，因此该 meter 会直接说明，而不是长期显示 `0 B/s`。

两者都是对累计计数器的*速率*，并以迄今见过的最大速率（而非固定上限）为标尺
——固定在 1 Gbit 的进度条在 100 Mbit 链路上会细得看不见。

### 主题

共八套，在 `F2` → Colors 里选（或配置里的 `color_scheme`）：

| | |
|---|---|
| `gander`（默认） | 深海军蓝底上的青色，紫色点缀 |
| `default` | htop 自己的配色 |
| `monochrome` | 完全无彩色 |
| `midnight` | 蓝上加蓝，低对比 |
| `classic` | 深海军蓝，单一强调色，实心蓝选中行 |
| `verdant` | 偏绿 |
| `paper` | 浅色背景，适合亮色终端 |
| `quiet` | 去饱和的灰阶 |

`paper` 在结构上是关键的那一套：每个样式都设了前景色，而多数不设背景色 —— 这在
深色主题下看不出来，在浅色主题下就是坏的。渲染器会先用主题背景铺满整帧，因此四条
绘制路径（htop、panels、setup、对话框）都自动继承。

## 平台支持

| | Linux | macOS |
|---|---|---|
| 进程列表 | `/proc` | `sysctl(KERN_PROC_ALL)` + `libproc` |
| 进程详情 | `/proc/<pid>` | Mach + `proc_pidinfo` |
| 容器列 | cgroup + PID namespace | 不可用 |
| 线程按行显示 | 支持（`H`） | 支持（`H`） |
| CPU 亲和性（`a`） | 可读可写 | 仅有调度器提示，面板会说明原因 |
| I/O 优先级（`i`） | 可读可写 | 不可用，面板会说明原因 |

两个后端都针对真实操作系统验证过。其他 unix 目标可以编译 `/proc` 后端，但没有
可用的数据源；此时 TUI 会拒绝启动，而不是显示一片空白。细节见
[docs/PORTABILITY.md](docs/PORTABILITY.md)。

## 构建与测试

```bash
cargo build --release      # target/release/gander
cargo test                 # macOS 300 个，Linux 279 个
cargo clippy --all-targets # 零警告
cargo bench -p gander-core # 扫描耗时
```

`crates/core/tests/fixtures/proc/` 下的测试样例是真实的 `/proc` 快照，
因此 Linux 读取路径可以在任何 unix 上测试。

## 设计

gander 把 htop 的*功能清单*当作需求文档，丢掉它的*代码结构*：htop 是基于 ncurses
的保留模式控件工具箱，而 gander 是在与 UI 无关的 core 之上、用 ratatui 实现的
立即模式前端。core crate 不依赖任何 UI，这正是 `--json` 几乎零成本的原因。

| | htop（C） | gander（Rust） |
|---|---|---|
| 渲染 | ncurses，保留模式 Panel | ratatui + crossterm，立即模式 |
| 控件 | `Object` 虚表 + 焦点栈 | `enum Screen` + 各屏独立的 `handle_key()` |
| 数据 | 单线程扫描 | 并行扫描（rayon）+ 后台采样线程 |
| 配置 | `htoprc`，手写解析器 | TOML，并提供 `htoprc` 导入器 |
| 平台 | Linux、BSD、macOS | Linux + macOS，共用 `PlatformSampler` trait |

更多内容：设计见 [docs/DESIGN.md](docs/DESIGN.md)，已完成与后续计划见
[docs/ROADMAP.md](docs/ROADMAP.md)。

## 许可证

GPL-2.0-or-later，与 htop 保持一致。行为、按键绑定和列语义都源自 htop；
请保持许可证兼容，并注明出处。
