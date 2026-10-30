# Fedora COPR

COPR 给 gander 补的是**可发现性**，不是包本身——签名的 dnf 仓库已经在
`packaging/rpm` 的发布流程里跑着。Fedora 用户搜索 `dnf search gander` 时
COPR 不算数，但 `dnf copr enable` 是他们找第三方工具的常规路径，README 的
安装区块里值得多一行。

构建方式沿用 tap 的思路：**打包已发布的静态 musl 二进制，不在 COPR 上编译
Rust**。COPR 的构建机没有 rust 模块保障，源码构建会把维护成本转给每次
toolchain 升级；musl 压缩包链接不到宿主机任何东西，Fedora 全版本通用。

## 操作步骤（需要 Fedora Account，卡在你的账号上）

1. 登录 <https://copr.fedorainfracloud.org/>（用 FAS 账号），新建项目
   `gander`：
   - Chroots：勾最新两代 Fedora 的 x86_64 和 aarch64（去掉 EOL 的）
   - 其他默认；**Build options → 取消 "Enable internet during build"**
     不需要勾，Source0 是 rpmbuild 自己拉的
2. 项目建好后 → Packages → 创建包 `gander`，Recipe 填本目录的
   `gander.spec`（提交到仓库后用 raw 链接，或直接粘贴内容）
3. 触发一次构建，确认产物 `dnf install` 能装
4. 安装命令是：

       dnf copr enable pandasir/gander
       dnf install gander

5. 把这两行加进四处安装文档（README.md、README.zh-CN.md / ja / ko 的
   Package managers 表格，`site/index.html` 的安装区块），然后跑
   `packaging/check-channels.sh`——它会校验「有文档就有活渠道」。

## 每次发版

COPR 的包不会自己跟 release。发版后把本目录 `gander.spec` 里的 `Version:`
改一行，再到项目页对 `gander` 包点 "Rebuild"。Fedora 上验证：

    dnf download --source gander && rpmlint gander*.src.rpm
