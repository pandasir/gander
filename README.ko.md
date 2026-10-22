# gander

[![CI](https://github.com/pandasir/gander/actions/workflows/ci.yml/badge.svg)](https://github.com/pandasir/gander/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pandasir/gander?label=release)](https://github.com/pandasir/gander/releases)
[![Downloads](https://img.shields.io/github/downloads/pandasir/gander/total?label=downloads)](https://github.com/pandasir/gander/releases)
[![crates.io](https://img.shields.io/crates/v/gander-bin?label=crates.io)](https://crates.io/crates/gander-bin)
[![License: GPL-2.0-or-later](https://img.shields.io/badge/license-GPL--2.0--or--later-blue)](LICENSE)
[![Platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20FreeBSD-lightgrey)](#플랫폼-지원)

[English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | **한국어**

> **gander**는 Rust로 작성된 Linux·macOS·FreeBSD용 대화형 프로세스 뷰어입니다.
> htop과 같은 부류지만, 레이아웃의 주인공은 "모든 프로세스"가 아니라
> "고른 프로세스 하나"입니다.

![gander의 panels 레이아웃에서 프로세스를 선택한 모습](assets/screenshot-panels.png)

```bash
brew install pandasir/tap/gander     # macOS
cargo install gander-bin             # Rust가 있는 모든 플랫폼
yay -S gander                        # Arch Linux
```

![녹화: 프로세스 선택, 트리 보기, 그리고 L로 htop 레이아웃 전환](assets/demo.gif)

gander는 시스템에서 실행 중인 프로세스와 각 프로세스의 리소스 사용량을 보여 줍니다.
Linux, macOS, FreeBSD에서 실행되며 대부분의 htop 키 바인딩을 그대로 씁니다.

## 주요 기능

gander는 기본으로 `panels` 레이아웃을 사용합니다. 왼쪽에 간결한 시스템 정보 영역,
가운데에 프로세스 표, 오른쪽에 실시간 상세 정보 패널을 둡니다. 상세 정보 패널은 선택한
프로세스를 따라가므로 명령줄, 상태, 메모리 사용량, 실행 시간을 목록에서 벗어나지 않고
볼 수 있습니다. `L`을 누르면 기존 전체 너비 `htop` 레이아웃으로 전환됩니다.

샘플링은 백그라운드에서 실행되고 렌더링 측은 스냅샷을 읽습니다. 스크립트에서는
`--json`, `-n`, `--sort`, `--filter`, `--limit`을 TUI 없이 사용할 수 있습니다.

Linux에서는 볼 수 있는 항목이 더 많습니다. PSS/EPSS, PSI, zram/zswap, GPU, 지연 통계,
컨테이너 식별 정보를 선택적 열이나 미터로 표시할 수 있습니다. 각 백엔드는 플랫폼
자체의 인터페이스를 읽습니다. Linux는 `/proc`, macOS는 `sysctl`/Mach/libproc,
FreeBSD는 `sysctl`/`libkvm`입니다.

## 플랫폼 지원

| 기능 | Linux | macOS | FreeBSD |
|---|:---:|:---:|:---:|
| 프로세스, CPU, 메모리 보기 | 지원 | 지원 | 지원 |
| 호스트 전체 네트워크 전송 속도 | 지원 | 지원 | 지원 |
| 호스트 전체 디스크 전송 속도 (`DiskIO`) | 지원 | - | 지원 |
| 컨테이너 정보 열 | 지원 | - | - |
| PSS / GPU / 지연 통계 열 | 지원 | - | - |
| CPU 선호도 편집 | 지원 | 힌트만 제공 | 지원 |

다른 Unix 타깃에서도 컴파일될 수 있지만, 실제 시스템 정보를 수집하는 백엔드는 없습니다.
자세한 내용은 [docs/PORTABILITY.md](docs/PORTABILITY.md)를 참고하세요.

## 다른 도구와 비교

htop 대체제를 찾고 있다면, 짧게 정리하면 이렇습니다.

| | gander | htop | btop | bottom |
|---|:---:|:---:|:---:|:---:|
| 터미널 UI | 있음 | 있음 | 있음 | 있음 |
| Linux / macOS / FreeBSD | 모두 | 모두 | 일부 | 모두 |
| htop 키 바인딩 계승 | 함 | - | 안 함 | 안 함 |
| 기존 `htoprc` 가져오기 | 가능 | - | 불가 | 불가 |
| 선택한 프로세스가 화면에 남음 | 남음 | 안 남음 | 안 남음 | 안 남음 |
| htop이 아닌 레이아웃 | `panels` | - | 있음 | 있음 |
| `--json` 배치 모드 | 있음 | 없음 | 없음 | 없음 |

매일 쓰면서 차이가 나는 건 마지막 두 줄입니다. htop, btop, bottom은 "지금 뭐가
돌고 있나"를 모든 프로세스를 펼쳐서 답하고, 궁금한 프로세스의 세부 정보는 다른
화면에 있거나 아예 없습니다. gander는 인스펙터를 표 옆에 두기 때문에 행을
고르는 순간 답이 거기에 있고, 목록을 떠날 필요가 없습니다. htop의 배치가
목적이었다면 `L`로 전환하면 되고, 키도 htop 자리에 그대로 있습니다.

## 설치

### 미리 빌드된 바이너리

[GitHub Releases](https://github.com/pandasir/gander/releases)에서
사용 중인 플랫폼에 맞는 압축 파일을 다운로드하세요.

| 플랫폼 | 타깃 |
|---|---|
| macOS, Apple Silicon | `aarch64-apple-darwin` |
| macOS, Intel | `x86_64-apple-darwin` |
| Linux, x86_64, glibc | `x86_64-unknown-linux-gnu` |
| Linux, x86_64, 정적 링크 | `x86_64-unknown-linux-musl` |
| Linux, arm64, glibc | `aarch64-unknown-linux-gnu` |
| Linux, arm64, 정적 링크 | `aarch64-unknown-linux-musl` |
| FreeBSD | `x86_64-unknown-freebsd` |

릴리스는 Linux x86_64와 arm64용 `.deb` 및 `.rpm` 패키지도 압축 파일 옆에 함께
제공합니다. [Debian/Ubuntu 및 Fedora/RHEL](#debianubuntu-및-fedorarhel)을
참고하세요.

```bash
tar -xzf gander-<version>-<target>.tar.gz
./gander-<version>-<target>/gander
```

`PATH`에 포함된 위치에 설치하려면 다음 명령을 실행하세요.

```bash
mkdir -p ~/.local/bin
install -m755 gander-<version>-<target>/gander ~/.local/bin/gander
```

musl 빌드는 정적으로 링크되어 호스트의 glibc에 의존하지 않습니다.
각 압축 파일에는 해당하는 `.sha256` 체크섬 파일이 함께 제공됩니다.

### Debian/Ubuntu 및 Fedora/RHEL

두 배포판 모두 서명된 저장소가 있어 `apt`와 `dnf`로 일반 패키지처럼 설치하고
업그레이드할 수 있습니다.

```bash
# Debian, Ubuntu, Linux Mint 등
sudo mkdir -p /usr/share/keyrings
curl -fsSL https://pandasir.github.io/gander/gander.gpg \
  | sudo gpg --dearmor -o /usr/share/keyrings/gander.gpg
echo "deb [signed-by=/usr/share/keyrings/gander.gpg] https://pandasir.github.io/gander stable main" \
  | sudo tee /etc/apt/sources.list.d/gander.list
sudo apt update && sudo apt install gander

# Fedora, RHEL, Rocky, Alma, openSUSE 등
sudo rpm --import https://pandasir.github.io/gander/RPM-GPG-KEY-gander
sudo curl -fsSL https://pandasir.github.io/gander/gander.repo \
  -o /etc/yum.repos.d/gander.repo
sudo dnf install gander
```

이후 업그레이드는 `apt upgrade` / `dnf upgrade`로 이루어집니다. 저장소는 서명되어
있고 클라이언트가 이를 검증합니다(`apt`는 `InRelease`, `dnf`는 `gpgcheck=1`과
`repo_gpgcheck=1`).

저장소를 추가하고 싶지 않다면 릴리스에서 패키지를 직접 받을 수도 있습니다.

```bash
sudo apt install ./gander_<version>-1_amd64.deb      # ARM 머신에서는 arm64
sudo dnf install ./gander-<version>-1.x86_64.rpm     # ARM 머신에서는 aarch64
```

옆에 있는 `SHA256SUMS`가 모든 패키지를 포함합니다.

### 패키지 관리자

| 방법 | 명령 |
|---|---|
| Debian, Ubuntu | `sudo apt install gander` |
| Fedora, RHEL, Rocky | `sudo dnf install gander` |
| Homebrew | `brew install pandasir/tap/gander` |
| crates.io | `cargo install gander-bin` |
| Arch Linux (AUR) | `yay -S gander` |

crates.io에서는 `gander`라는 이름을 관련 없는 다른 crate가 이미 사용하고 있어
패키지 이름이 `gander-bin`입니다. 설치 후 실행 명령은 그대로 `gander`입니다.

### 소스에서 빌드

**Rust 1.89 이상**이 필요합니다. ncurses나 libcurses 개발용 헤더는 필요하지 않습니다.

```bash
git clone https://github.com/pandasir/gander
cd gander
cargo install --locked --path crates/bin
```

설치하지 않고 바로 실행할 수도 있습니다.

```bash
cargo run --release -p gander-bin
```

## 빠른 시작

```bash
gander                                     # 대화형 TUI
gander --delay 500                         # 500밀리초마다 샘플링
gander -n 5 -d 1000                        # 텍스트 샘플 5회 출력
gander --json --limit 10                   # JSON 샘플 1회 출력
gander --sort PERCENT_MEM --limit 20       # 메모리 사용률 상위 20개
gander -F "python|node"                    # 명령줄로 필터링
gander --import-htoprc ~/.config/htop/htoprc --save
```

필터는 대소문자를 구분하지 않는 고정 문자열이며 `|`로 구분합니다.
정규 표현식이 아닙니다. 전체 옵션은 `gander --help`에서 확인할 수 있습니다.

## 화면과 키 조작

`panels`가 기본 레이아웃이고 `htop`이 기존 전체 너비 레이아웃입니다.
`L`로 전환할 수 있으며 gander는 선택을 기억합니다.

| `panels`(기본) | `htop`(`L`) | `t` 트리 보기 |
|---|---|---|
| ![panels 레이아웃](assets/screenshot-panels.png) | ![htop 레이아웃](assets/screenshot-htop.png) | ![트리 보기](assets/screenshot-tree.png) |

| 키 | 동작 |
|---|---|
| `F1` / `h` / `?` | 도움말 |
| `F2` | 설정 |
| `F3` / `/` | 검색 |
| `F4` / `\` | 필터 |
| `F5` / `t` | 트리 보기 |
| `F6` / `<` `>` | 정렬 열 변경 |
| `F9` / `k` | 시그널 보내기 |
| `F10` / `q` | 종료 |

htop과 동작이 다른 키:

| 키 | gander에서의 동작 |
|---|---|
| `I` | 선택한 헤더 미터의 표시 모드 전환 |
| `+` / `-` | 샘플링 속도 높이기 / 낮추기 |
| `L` | 레이아웃 전환 |
| `Shift+Left` / `Shift+Right` | 헤더 미터 커서 이동 |

htop의 `I`는 정렬 순서를 반전합니다. gander에서는 대신 `F6` / `<` `>` / `.`을
사용하세요. 전체 키 바인딩은 [docs/KEYS.md](docs/KEYS.md)를 참고하세요.

## 설정

설정 파일은 TOML 형식입니다. 경로는 `$GANDER_CONFIG`를 우선하며,
설정되지 않은 경우 `$XDG_CONFIG_HOME/gander/config.toml`을 사용합니다.
이 값도 설정되지 않았다면 `~/.config/gander/config.toml`을 사용합니다.
각 탭은 자체 열 구성과 정렬 순서를 유지하며, 헤더 미터와 표시 모드도 설정할 수 있습니다.
설정은 정상 종료 시 저장됩니다. 파일이 없으면 기본값을 사용합니다.
파일을 읽을 수 없거나 형식 또는 설정값이 잘못된 경우, 원본 파일을 덮어쓰지 않고
오류를 알립니다. 저장 시에는 파일을 원자적으로 교체합니다.

검색 및 필터 기록은 gander가 `$XDG_STATE_HOME/gander/`에 저장합니다.
설정되지 않은 경우 `~/.local/state/gander/`을 사용합니다.

사용 가능한 테마는 `gander`, `default`, `monochrome`, `midnight`, `classic`,
`verdant`, `paper`, `quiet`입니다. `F2` -> Colors에서 선택하거나
설정 파일의 `color_scheme`으로 지정할 수 있습니다.

## Linux 전용 데이터

- `M_PSS`, `M_EPSS`, `M_PRIVATE`, `M_SWAP`: 필요할 때 `smaps_rollup`에서 읽습니다.
  측정하지 않은 값은 `0`이 아닌 `-`로 표시됩니다.
- `GPU%`, `GPU_TIME`: 제조사 SDK 없이 `drm-usage-stats`에서 읽습니다.
- `CPUD%`, `IOD%`, `SWPD%`: TASKSTATS 기반 지연 통계이며 `CAP_NET_ADMIN`이 필요합니다.
- `CONTAINER`, `CGROUP`: cgroup 경로와 PID 네임스페이스 감지를 함께 사용합니다.
- `PSI`, `Zram`, `Zswap`, `HugePages`: 선택적으로 추가할 수 있는 헤더 미터입니다.

`F2` -> Columns 또는 `F2` -> Header에서 활성화할 수 있습니다. 구현 세부 사항은
[docs/DESIGN.md](docs/DESIGN.md)와 [docs/PORTABILITY.md](docs/PORTABILITY.md)를
참고하세요.

## 개발

워크스페이스는 4개의 crate로 구성됩니다.

| Crate | 역할 |
|---|---|
| `gander-core` | 플랫폼별 샘플링, 파싱, 프로세스 모델과 작업 |
| `gander-config` | TOML 설정, 기록, `htoprc` 가져오기 |
| `gander-tui` | 렌더링, 레이아웃, 입력, 런타임 |
| `gander-bin` | CLI, 배치 출력, 실행 파일 |

```bash
cargo build --locked
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo bench -p gander-core
```

macOS의 실제 시스템을 사용하는 테스트는 시스템 프로세스 테이블을 읽으므로,
제한된 샌드박스 밖에서 실행해야 할 수 있습니다.
전체 개발 및 검토 절차는 [CONTRIBUTING.md](CONTRIBUTING.md)를 참고하세요.

## 문서

| 문서 | 내용 |
|---|---|
| [assets/README.md](assets/README.md) | 스크린샷과 재생성 방법 |
| [docs/KEYS.md](docs/KEYS.md) | 전체 키 바인딩과 배치 모드 |
| [docs/LAYOUT.md](docs/LAYOUT.md) | `panels` 레이아웃과 좁은 터미널에서의 표시 조정 |
| [docs/PORTABILITY.md](docs/PORTABILITY.md) | 백엔드 지원 표와 플랫폼 제한 |
| [docs/DESIGN.md](docs/DESIGN.md) | 아키텍처와 데이터 모델 |
| [docs/ROADMAP.md](docs/ROADMAP.md) | 완료된 마일스톤과 향후 계획 |
| [CHANGELOG.md](CHANGELOG.md) | 릴리스 이력 |

## 자주 묻는 질문

**gander가 htop을 대신할 수 있나요?**
됩니다, 그리고 안 됩니다. 같은 `/proc`을 읽고 키도 htop 자리에 있으므로 지금
쓰는 방식은 그대로 통합니다. `--import-htoprc`로 htop 설정을 그대로 가져올 수도
있습니다. 다만 기본 레이아웃은 의도적으로 htop과 다릅니다. "전체 너비 표 +
별도 화면의 세부 정보"가 프로세스 목록을 읽는 유일한 방법은 아니기 때문입니다.

**Windows에서 되나요?**
안 됩니다. 백엔드는 `/proc`(Linux), `sysctl`/Mach/libproc(macOS),
`sysctl`/`libkvm`(FreeBSD) 세 가지이고 Windows용은 없습니다. Linux 커널
인터페이스에 기대는 열(PSS, PSI, zram, 지연 어카운팅, 컨테이너)도 Linux
전용입니다.

**crates.io 패키지 이름이 `gander-bin`인 이유는?**
crates.io의 `gander`는 무관한, 오래전에 yank된 크레이트이기 때문입니다.
설치되는 명령 이름은 그대로 `gander`입니다.

**ncurses를 쓰지 않는 이유는?**
렌더링은 [ratatui](https://ratatui.rs)와
[crossterm](https://github.com/crossterm-rs/crossterm)로 하므로, 빌드 전에
ncurses 개발 헤더를 설치할 필요가 없습니다.

**스크립트에서 쓸 수 있나요?**
됩니다. `gander --json`, `-n`, `--sort`, `--filter`, `--limit`은 TUI를 띄우지
않고 프로세스마다 한 줄을 출력합니다.

## 기여

버그 보고, 플랫폼별 동작 보고, 범위를 좁힌 풀 리퀘스트를 환영합니다.
먼저 [CONTRIBUTING.md](CONTRIBUTING.md)를 읽어 주세요.
보안 취약점은 [SECURITY.md](SECURITY.md)에 따라 비공개로 보고하고,
공개 이슈를 생성하지 마세요.

## 라이선스

GPL-2.0-or-later. gander의 동작, 키 바인딩, 열의 의미는 htop에서 가져왔습니다.
라이선스 호환성을 유지하고 출처를 명시해 주세요.
