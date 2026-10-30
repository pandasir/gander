# nixpkgs

nixpkgs 的收录路径是 `pkgs/by-name/<前两字母>/<包名>/package.nix`（对 nixpkgs
来说这比 PR 到分类目录省事，review 也快）。本目录的 `package.nix` 是按
by-name 规范写好的草稿，直接复制到 nixpkgs 仓库对应路径提 PR。

## 提交前必须解决的一件事

`cargoHash` 现在是占位符（`lib.fakeHash`），本地没装 nix，没法预取。提 PR 前：

```bash
# 在装有 nix 的机器上，package.nix 放进 nixpkgs checkout 后
nix-build -A gander 2>&1 | grep -A2 "got:"   # 报错里会给出真实 hash
# 用真实 hash 替换 fakeHash，再跑一次 nix-build 确认通过
```

nixpkgs 的 CI（ofborg）也会跑 `nixpkgs-review`，占位符过不了。

## PR 流程（卡在你的 GitHub 账号上，流程本身不难）

1. fork nixpkgs，建分支 `gander`
2. 新增 `pkgs/by-name/ga/gander/package.nix`（用本目录草稿，填好 cargoHash）
3. `nixpkgs-fmt` 格式化；commit message 用 nixpkgs 惯例首行：
   `gander: init at 0.6.7`
4. PR 描述按模板填：指向 GitHub releases、说明三平台、链接 upstream 的
   CHANGELOG。**不要**在描述里贴宣传语，nixpkgs review 只关心包质量
5. 加 `10.rebuild-darwin: 1-10` 和 `10.rebuild-linux: 1-10` 标签的请求
   （by-name 新包影响面小，review 通常几天）

## 两个注意

- nixpkgs 对新包也有活跃度/影响力的隐性门槛，但比 homebrew-core 宽松得多，
  htop 替代品这个类别里有先例（bottom、btop 都在）。Show HN 之后提，
  描述里能有「用过的人」做背书更稳。
- 提 PR 前 search.nixos.org 确认 `gander` 这个名字没被占——crates.io 上
  `gander` 被占过（所以才发 `gander-bin`），nixpkgs 名字空间独立，但要查。
