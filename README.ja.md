# gander

[English](README.md) | [简体中文](README.zh-CN.md) | **日本語** | [한국어](README.ko.md)

> Rust で書かれた、クロスプラットフォームの対話型プロセスビューア。

gander は、システム上で実行中のプロセスとそのリソース使用量を表示します。
**Linux、macOS、FreeBSD** に対応し、htop のキーバインドの多くを引き継ぎながら、
常時表示の詳細パネルを備えた、プロセス一覧中心のレイアウトを提供します。

## 特長

- **プロセス一覧を中心としたレイアウト。** デフォルトの `panels` レイアウトは、
  コンパクトなシステム情報欄、幅広いプロセステーブル、リアルタイムの詳細パネルで
  構成されます。`L` を押すと、従来の `htop` レイアウトに切り替えられます。
- **一覧を離れずに詳細を確認。** 選択したプロセスに合わせて、コマンドライン、状態、
  メモリ使用量、実行時間が更新されます。
- **応答性を保つサンプリング。** サンプリングはバックグラウンドで実行され、
  描画側はスナップショットを読み取ります。
- **スクリプトから利用できる出力。** `--json`、`-n`、`--sort`、`--filter`、
  `--limit` は TUI を起動せずに使用できます。
- **Linux 向けの詳細な指標。** PSS/EPSS、PSI、zram/zswap、GPU、遅延統計、
  コンテナ識別情報を、追加の列やメーターとして表示できます。
- **ネイティブなバックエンド。** Linux では `/proc`、macOS では
  `sysctl`/Mach/libproc、FreeBSD では `sysctl`/`libkvm` を使用します。

## 対応プラットフォーム

| 機能 | Linux | macOS | FreeBSD |
|---|:---:|:---:|:---:|
| プロセス、CPU、メモリの表示 | 対応 | 対応 | 対応 |
| ホスト全体のネットワーク転送速度 | 対応 | 対応 | 対応 |
| ホスト全体のディスク転送速度（`DiskIO`） | 対応 | - | 対応 |
| コンテナ情報の列 | 対応 | - | - |
| PSS / GPU / 遅延統計の列 | 対応 | - | - |
| CPU アフィニティの編集 | 対応 | ヒントのみ | 対応 |

その他の Unix ターゲットでもコンパイルできる場合がありますが、実際のシステム情報を
取得するバックエンドはありません。詳細は
[docs/PORTABILITY.md](docs/PORTABILITY.md) を参照してください。

## インストール

### ビルド済みバイナリ

[GitHub Releases](https://github.com/pandasir/gander/releases) から、
プラットフォームに合ったアーカイブをダウンロードしてください。

| プラットフォーム | ターゲット |
|---|---|
| macOS、Apple Silicon | `aarch64-apple-darwin` |
| macOS、Intel | `x86_64-apple-darwin` |
| Linux、x86_64、glibc | `x86_64-unknown-linux-gnu` |
| Linux、x86_64、静的リンク | `x86_64-unknown-linux-musl` |
| Linux、arm64、glibc | `aarch64-unknown-linux-gnu` |
| Linux、arm64、静的リンク | `aarch64-unknown-linux-musl` |
| FreeBSD | `x86_64-unknown-freebsd` |

Linux x86_64 と arm64 には `.deb` と `.rpm` パッケージも用意されています。
[Debian/Ubuntu と Fedora/RHEL](#debianubuntu-と-fedorarhel) を参照してください。

```bash
tar -xzf gander-<version>-<target>.tar.gz
./gander-<version>-<target>/gander
```

`PATH` の通った場所にインストールするには、次のコマンドを実行します。

```bash
mkdir -p ~/.local/bin
install -m755 gander-<version>-<target>/gander ~/.local/bin/gander
```

musl 版は静的リンクされているため、ホストの glibc に依存しません。
各アーカイブには、対応する `.sha256` チェックサムファイルが付属しています。

### Debian/Ubuntu と Fedora/RHEL

どちらのディストリビューションにも署名済みリポジトリがあり、`apt` と `dnf` で
通常のパッケージと同じようにインストール・更新できます。

```bash
# Debian、Ubuntu、Linux Mint など
sudo mkdir -p /usr/share/keyrings
curl -fsSL https://pandasir.github.io/gander/gander.gpg \
  | sudo gpg --dearmor -o /usr/share/keyrings/gander.gpg
echo "deb [signed-by=/usr/share/keyrings/gander.gpg] https://pandasir.github.io/gander stable main" \
  | sudo tee /etc/apt/sources.list.d/gander.list
sudo apt update && sudo apt install gander

# Fedora、RHEL、Rocky、Alma、openSUSE など
sudo rpm --import https://pandasir.github.io/gander/RPM-GPG-KEY-gander
sudo dnf config-manager --add-repo https://pandasir.github.io/gander/gander.repo
sudo dnf install gander
```

以降の更新は `apt upgrade` / `dnf upgrade` で行われます。リポジトリは署名されており、
クライアントが検証します（`apt` は `InRelease`、`dnf` は `gpgcheck=1` と
`repo_gpgcheck=1`）。

リポジトリを追加したくない場合は、リリースからパッケージを直接ダウンロードできます。

```bash
sudo apt install ./gander_<version>-1_amd64.deb      # ARM マシンでは arm64
sudo dnf install ./gander-<version>-1.x86_64.rpm     # ARM マシンでは aarch64
```

隣の `SHA256SUMS` がすべてのパッケージを網羅します。

### パッケージマネージャー

| 方法 | コマンド |
|---|---|
| Debian、Ubuntu | `sudo apt install gander` |
| Fedora、RHEL、Rocky | `sudo dnf install gander` |
| Homebrew | `brew install pandasir/tap/gander` |
| crates.io | `cargo install gander-bin` |
| Arch Linux（AUR） | `yay -S gander` |

crates.io では、`gander` という名前が別の無関係な crate に使用されているため、
パッケージ名は `gander-bin` です。インストール後のコマンド名は `gander` のままです。

### ソースからビルド

**Rust 1.89 以降**が必要です。ncurses や libcurses の開発用ヘッダーは不要です。

```bash
git clone https://github.com/pandasir/gander
cd gander
cargo install --locked --path crates/bin
```

インストールせずに実行することもできます。

```bash
cargo run --release -p gander-bin
```

## クイックスタート

```bash
gander                                     # 対話型 TUI
gander --delay 500                         # 500 ミリ秒ごとにサンプリング
gander -n 5 -d 1000                        # テキスト形式で 5 回出力
gander --json --limit 10                   # JSON 形式で 1 回出力
gander --sort PERCENT_MEM --limit 20       # メモリ使用率の上位 20 件
gander -F "python|node"                    # コマンドラインで絞り込み
gander --import-htoprc ~/.config/htop/htoprc --save
```

フィルターは大文字と小文字を区別しない固定文字列で、`|` で区切ります。
正規表現ではありません。すべてのオプションは `gander --help` で確認できます。

## 画面とキー操作

デフォルトの `panels` レイアウトでは、左側にシステム情報、中央にプロセステーブル、
右側に詳細パネルを表示します。`htop` は従来の全幅レイアウトです。
`L` で切り替えられ、選択したレイアウトは保存されます。

| キー | 操作 |
|---|---|
| `F1` / `h` / `?` | ヘルプ |
| `F2` | 設定 |
| `F3` / `/` | 検索 |
| `F4` / `\` | フィルター |
| `F5` / `t` | ツリー表示 |
| `F6` / `<` `>` | ソート列の変更 |
| `F9` / `k` | シグナルの送信 |
| `F10` / `q` | 終了 |

htop と動作が異なるキー:

| キー | gander での動作 |
|---|---|
| `I` | 選択中のヘッダーメーターの表示モードを切り替え |
| `+` / `-` | サンプリングを速く / 遅くする |
| `L` | レイアウトの切り替え |
| `Shift+Left` / `Shift+Right` | ヘッダーメーターのカーソルを移動 |

htop の `I` はソート順を反転しますが、gander では代わりに
`F6` / `<` `>` / `.` を使用してください。
すべてのキーバインドは [docs/KEYS.md](docs/KEYS.md) を参照してください。

## 設定

設定ファイルは TOML 形式です。パスは `$GANDER_CONFIG` を優先し、未設定の場合は
`$XDG_CONFIG_HOME/gander/config.toml`、さらに未設定の場合は
`~/.config/gander/config.toml` を使用します。各タブはそれぞれの列とソート順を保持し、
ヘッダーメーターとその表示モードも設定できます。設定は正常終了時に保存されます。
ファイルが存在しない場合はデフォルト設定を使用します。読み取り不能、構文エラー、
無効な設定がある場合は、ファイルを上書きせずにエラーを報告します。
保存時はファイルをアトミックに置き換えます。

検索とフィルターの履歴は `$XDG_STATE_HOME/gander/` に保存されます。
未設定の場合は `~/.local/state/gander/` を使用します。

利用可能なテーマは `gander`、`default`、`monochrome`、`midnight`、`classic`、
`verdant`、`paper`、`quiet` です。`F2` -> Colors から選択するか、
設定ファイルの `color_scheme` で指定してください。

## Linux 向けの詳細機能

- `M_PSS`、`M_EPSS`、`M_PRIVATE`、`M_SWAP`: 必要に応じて `smaps_rollup` から
  読み取ります。未測定の値は `0` ではなく `-` と表示されます。
- `GPU%`、`GPU_TIME`: ベンダーの SDK を使わず、`drm-usage-stats` から読み取ります。
- `CPUD%`、`IOD%`、`SWPD%`: TASKSTATS による遅延統計です。
  `CAP_NET_ADMIN` が必要です。
- `CONTAINER`、`CGROUP`: cgroup のパスと PID 名前空間の検出を組み合わせます。
- `PSI`、`Zram`、`Zswap`、`HugePages`: 追加可能なヘッダーメーターです。

`F2` -> Columns または `F2` -> Header で有効にできます。実装の詳細は
[docs/DESIGN.md](docs/DESIGN.md) と [docs/PORTABILITY.md](docs/PORTABILITY.md)
を参照してください。

## 開発

ワークスペースは 4 つの crate で構成されています。

| Crate | 役割 |
|---|---|
| `gander-core` | プラットフォームごとのサンプリング、解析、プロセスモデルと操作 |
| `gander-config` | TOML 設定、履歴、`htoprc` のインポート |
| `gander-tui` | 描画、レイアウト、入力、ランタイム |
| `gander-bin` | CLI、バッチ出力、実行ファイル |

```bash
cargo build --locked
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo bench -p gander-core
```

macOS の実システムを使うテストはシステムのプロセステーブルを読み取るため、
制限されたサンドボックスの外で実行する必要がある場合があります。
開発とレビューの手順は [CONTRIBUTING.md](CONTRIBUTING.md) を参照してください。

## ドキュメント

| ドキュメント | 内容 |
|---|---|
| [docs/KEYS.md](docs/KEYS.md) | 全キーバインドとバッチモード |
| [docs/LAYOUT.md](docs/LAYOUT.md) | `panels` レイアウトと狭い端末での表示調整 |
| [docs/PORTABILITY.md](docs/PORTABILITY.md) | バックエンドの対応表とプラットフォームの制限 |
| [docs/DESIGN.md](docs/DESIGN.md) | アーキテクチャとデータモデル |
| [docs/ROADMAP.md](docs/ROADMAP.md) | 完了したマイルストーンと今後の計画 |
| [CHANGELOG.md](CHANGELOG.md) | リリース履歴 |

## コントリビュート

バグ報告、各プラットフォームでの動作報告、変更範囲を絞ったプルリクエストを
歓迎します。まず [CONTRIBUTING.md](CONTRIBUTING.md) をお読みください。
セキュリティ上の脆弱性は [SECURITY.md](SECURITY.md) に従って非公開で報告し、
公開の issue は作成しないでください。

## ライセンス

GPL-2.0-or-later。動作、キーバインド、列の意味は htop に由来します。
ライセンスの互換性を維持し、出典を明記してください。
