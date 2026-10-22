# gander

[English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | **한국어**

> Rust로 작성된 크로스 플랫폼 대화형 프로세스 뷰어.

gander는 시스템에서 실행 중인 프로세스와 각 프로세스의 리소스 사용량을 보여 줍니다.
**Linux, macOS, FreeBSD**를 지원하며, 대부분의 htop 키 바인딩을 유지하면서
상세 정보 패널을 항상 표시하는 프로세스 목록 중심의 레이아웃을 제공합니다.

## 주요 기능

- **프로세스 목록 중심의 레이아웃.** 기본 `panels` 레이아웃은 간결한 시스템 정보 영역,
  넓은 프로세스 표, 실시간 상세 정보 패널로 구성됩니다. `L`을 누르면 기존 `htop`
  레이아웃으로 전환할 수 있습니다.
- **목록을 벗어나지 않고 상세 정보 확인.** 선택한 프로세스에 따라 명령줄, 상태,
  메모리 사용량, 실행 시간이 갱신됩니다.
- **반응성을 유지하는 샘플링.** 샘플링은 백그라운드에서 실행되며,
  렌더링 측에서는 스냅샷을 읽습니다.
- **스크립트에서 활용할 수 있는 출력.** `--json`, `-n`, `--sort`, `--filter`,
  `--limit`은 TUI 없이 사용할 수 있습니다.
- **Linux 전용 상세 지표.** PSS/EPSS, PSI, zram/zswap, GPU, 지연 통계,
  컨테이너 식별 정보를 선택적 열이나 미터로 표시할 수 있습니다.
- **네이티브 백엔드.** Linux에서는 `/proc`, macOS에서는 `sysctl`/Mach/libproc,
  FreeBSD에서는 `sysctl`/`libkvm`을 사용합니다.

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

Linux x86_64와 arm64에는 `.deb` 및 `.rpm` 패키지도 제공됩니다.
[Debian/Ubuntu 및 Fedora/RHEL](#debianubuntu-및-fedorarhel)을 참고하세요.

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

릴리스에는 x86_64와 arm64용 `.deb` 및 `.rpm` 패키지도 포함됩니다.
정적 musl 바이너리로 만들어지므로 런타임 의존성을 선언하지 않으며,
아키텍처가 일치하는 어떤 배포판(Debian 12, Ubuntu 24.04, Rocky 9, CentOS 7 등)에도
설치할 수 있습니다.

```bash
# Debian, Ubuntu, Linux Mint 등
sudo apt install ./gander_<version>-1_amd64.deb      # ARM 머신에서는 arm64

# Fedora, RHEL, Rocky, Alma, openSUSE 등
sudo dnf install ./gander-<version>-1.x86_64.rpm     # ARM 머신에서는 aarch64
```

옆에 있는 `SHA256SUMS`가 모든 패키지를 포함합니다. 이것은 **일회성 설치**이며
저장소가 아닙니다. 아직 `apt install gander`나 `apt upgrade` 업그레이드 경로는
없습니다. 새 버전으로 옮기려면 새 패키지를 내려받아 같은 방식으로 설치하세요.

### 패키지 관리자

| 방법 | 명령 |
|---|---|
| Debian, Ubuntu | `sudo apt install ./gander_<version>-1_amd64.deb` |
| Fedora, RHEL, Rocky | `sudo dnf install ./gander-<version>-1.x86_64.rpm` |
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

기본 `panels` 레이아웃은 왼쪽에 시스템 정보, 가운데에 프로세스 표,
오른쪽에 상세 정보 패널을 표시합니다. `htop`은 기존의 전체 너비 레이아웃입니다.
`L`로 전환할 수 있으며 선택한 레이아웃은 저장됩니다.

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

검색 및 필터 기록은 `$XDG_STATE_HOME/gander/`에 저장됩니다.
설정되지 않은 경우 `~/.local/state/gander/`을 사용합니다.

사용 가능한 테마는 `gander`, `default`, `monochrome`, `midnight`, `classic`,
`verdant`, `paper`, `quiet`입니다. `F2` -> Colors에서 선택하거나
설정 파일의 `color_scheme`으로 지정할 수 있습니다.

## Linux 전용 상세 기능

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
| [docs/KEYS.md](docs/KEYS.md) | 전체 키 바인딩과 배치 모드 |
| [docs/LAYOUT.md](docs/LAYOUT.md) | `panels` 레이아웃과 좁은 터미널에서의 표시 조정 |
| [docs/PORTABILITY.md](docs/PORTABILITY.md) | 백엔드 지원 표와 플랫폼 제한 |
| [docs/DESIGN.md](docs/DESIGN.md) | 아키텍처와 데이터 모델 |
| [docs/ROADMAP.md](docs/ROADMAP.md) | 완료된 마일스톤과 향후 계획 |
| [CHANGELOG.md](CHANGELOG.md) | 릴리스 이력 |

## 기여

버그 보고, 플랫폼별 동작 보고, 범위를 좁힌 풀 리퀘스트를 환영합니다.
먼저 [CONTRIBUTING.md](CONTRIBUTING.md)를 읽어 주세요.
보안 취약점은 [SECURITY.md](SECURITY.md)에 따라 비공개로 보고하고,
공개 이슈를 생성하지 마세요.

## 라이선스

GPL-2.0-or-later. 동작, 키 바인딩, 열의 의미는 htop에서 가져왔습니다.
라이선스 호환성을 유지하고 출처를 명시해 주세요.
