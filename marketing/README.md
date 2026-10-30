# 宣传节奏

**2026-10-31 决定：不发帖、不发论坛**。Show HN / Reddit / V2EX / 收录站全部
暂缓，文案保留在这个目录里，哪天改主意了直接用（从 `show-hn.md` 的
「发帖前检查」开始走）。下面只剩被动渠道：包管理器的可发现性、SEO、发版节奏。

要清楚这个取舍的代价：CLI 工具的自然增长几乎全靠那几个社区帖，纯被动意味着
增长主要靠搜索和包管理器收录，以月为单位而不是以周为单位。

基线（2026-10-30）：5 stars，14 天 102 次浏览 / 5 个独立访客，Release 资产
累计约 137 次下载，crates.io 48 次。referrer 几乎全是站内——这正好是干净的
本底，渠道生效之后的变化都算数。

素材与文案都在这个目录里，可复现的演示 GIF 在 `assets/demo-social.tape`。

---

## 谁在等谁

| 事项 | 卡在谁 | 备注 |
|---|---|---|
| COPR 项目 | 你的 Fedora 账号 | `packaging/copr/README.md`，十分钟流程，纯被动渠道 |
| nixpkgs PR | 你的账号（或我来提） | 需先在有 nix 的机器上填 `packaging/nix/package.nix` 的 cargoHash |
| homebrew-core | 建议缓一缓 | 草稿在 `packaging/homebrew/core-formula.rb`，等 stars 上去再提 |
| 发帖类全部 | **暂缓** | 2026-10-31 决定；文案在 `show-hn.md`、`reddit-*.md`、`v2ex.md`、`awesome-pr.md`、`this-week-in-rust.md` |
| AUR | **不可做** | 注册无限期关闭且无人工通道，见 `packaging/README.md`，别再花时间 |
| FreeBSD ports | 等 bugmeister 回信 | 见 `docs/待办.md`，不要动 README |

## 被动渠道清单（不发帖也能做的）

- [ ] `packaging/check-channels.sh` 保持通过——README 和站点里承诺的每个
      安装命令都有活渠道在后面，这是被动流量的底线
- [ ] COPR / nixpkgs 建好后，把安装命令加进四处安装文档（README.md、
      三份翻译、`site/index.html`），再跑一次 channels 检查
- [ ] SEO 已就位（对比页 + sitemap + GoatCounter），慢变量，不动它；可以按
      搜索意图陆续加长文页（比如「htop on Linux」这类标题，站点已有
      macOS/FreeBSD 两篇的先例）
- [ ] 保持发版节奏：没有帖子的窗口里，release 是唯一的活跃维护信号；
      0.7.0 什么时候发由功能节奏定，不必等曝光
- [ ] 每周跑一次 `.github/scripts/traffic-report.sh`，结果记到本文件底部

## 度量

```bash
.github/scripts/traffic-report.sh
```

它汇总 GitHub 浏览/克隆/referrer、Release 下载和 crates.io 下载。站点侧看
GoatCounter（gander.goatcounter.com）的 referrer 报表，回答「哪个渠道带来的」。

## homebrew-core 为什么缓一缓

homebrew-core 收录有 notability 要求，没有明确数字，但太小众的公式通常会被
maintainer 以「用户面不够广」为由拒绝——现在 5 个 stars 去提大概率吃闭门羹，
而且被拒过的东西再提更难。计划是：stars 到了几百、tap 有真实安装量，再按
`packaging/homebrew/core-formula.rb`（已按 core 的源码构建规范写好）提 PR。
在那之前 `pandasir/tap` 完全够用。

## 度量记录

（每周追加一行：日期 · stars · 14 天 views/uniques · Release 下载 · crates.io 下载 · 备注）
