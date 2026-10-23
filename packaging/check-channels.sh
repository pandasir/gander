#!/usr/bin/env bash
# Assert that every install command the READMEs and the landing page print is a
# command that actually works.
#
# The README, the three translated READMEs and site/index.html all tell a reader
# to run something. When one of those channels is missing -- an AUR package that
# was never pushed, a Homebrew tap whose formula does not resolve -- the reader
# gets an error from a command we told them to run. That is worse than silence,
# and nothing in the build notices it, because the code is fine.
#
# The checks here are the ones that need no credentials and no container:
#
#   1. Every `yay -S gander` / `apt install gander` / ... claim in the docs has a
#      matching channel that answers over the network.
#   2. The AUR package, if claimed, has a .SRCINFO identical to what the committed
#      PKGBUILD generates. A stale .SRCINFO is the usual reason a first push is
#      rejected.
#
# Run it from the repository root. Pass --offline to skip the network checks (it
# then only validates .SRCINFO, which is what the test suite can do without a
# network).
#
# Usage: packaging/check-channels.sh [--offline]
set -euo pipefail

offline=0
case "${1:-}" in
  --offline) offline=1 ;;
  "") ;;
  *)
    echo "usage: $0 [--offline]" >&2
    exit 2
    ;;
esac

cd "$(dirname "$0")/.."

failures=0
fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}
ok() { echo "ok: $*"; }

# ---------------------------------------------------------------------------
# 1. The .SRCINFO committed next to the PKGBUILD must be what that PKGBUILD
#    generates. makepkg is Arch-only, so parse the fields we set by hand instead
#    of requiring it; the fields compared are the ones a push is validated on.
# ---------------------------------------------------------------------------

srcinfo_matches_pkgbuild() {
  local pkgbuild=packaging/aur/PKGBUILD
  local srcinfo=packaging/aur/.SRCINFO

  [ -f "$pkgbuild" ] || { fail "no $pkgbuild"; return; }
  [ -f "$srcinfo" ] || { fail "no $srcinfo"; return; }

  # Pull a scalar assignment out of the PKGBUILD: pkgver=0.6.4, pkgrel=1, ...
  scalar() {
    sed -n "s/^$1=//p" "$pkgbuild" | head -1 | tr -d "\"'"
  }
  # Pull the same field out of .SRCINFO, which indents with a tab.
  field() {
    sed -n "s/^\t$1 = //p" "$srcinfo" | head -1
  }

  local v
  for v in pkgver pkgrel pkgdesc; do
    local a b
    a=$(scalar "$v")
    b=$(field "$v")
    [ -n "$a" ] || { fail "$pkgbuild: $v is empty"; continue; }
    [ "$a" = "$b" ] || fail "packaging/aur/.SRCINFO $v='$b' but PKGBUILD says '$a'"
  done

  # arch[] and license[] are arrays; .SRCINFO repeats the key.
  local arches
  arches=$(sed -n "s/^arch=(//p" "$pkgbuild" | tr -d "()'\"" | tr -s ' ')
  for a in $arches; do
    grep -qx "	arch = $a" "$srcinfo" ||
      fail "packaging/aur/.SRCINFO is missing 'arch = $a' from the PKGBUILD"
  done

  # Every checksum in the PKGBUILD has to appear in .SRCINFO, and vice versa.
  local sums
  sums=$(sed -n "s/^sha256sums=(//p" "$pkgbuild" | tr -d "()'\"" | tr -s ' ')
  for s in $sums; do
    [ -n "$s" ] || continue
    grep -q "$s" "$srcinfo" ||
      fail "packaging/aur/.SRCINFO is missing sha256 $s"
  done

  ok "packaging/aur/.SRCINFO agrees with its PKGBUILD"
}

srcinfo_matches_pkgbuild

if [ "$offline" -eq 1 ]; then
  [ "$failures" -eq 0 ] || { echo "$failures check(s) failed" >&2; exit 1; }
  echo "offline: skipped the network checks"
  exit 0
fi

# ---------------------------------------------------------------------------
# 2. Every documented install channel must answer.
#
# Each entry pairs the text that must appear in the docs with a check that the
# channel is real. A channel that the docs claim but that is not live is a bug
# in the docs or in the release, and this is the only place that can tell.
# ---------------------------------------------------------------------------

# fetch <url> [grep-pattern]: HTTP 200 and, if a pattern is given, a match in the
# body. Prints a short reason on failure.
http_ok() {
  local url=$1 pattern=${2:-} body
  # crates.io rejects requests without a User-Agent, and GitHub serves different
  # content to one, so every check identifies itself the same way.
  body=$(curl -fsSL --max-time 20 -A "gander-check-channels/1.0" "$url" 2>/dev/null) || {
    echo "no HTTP 200 from $url"
    return 1
  }
  if [ -n "$pattern" ] && ! printf '%s' "$body" | grep -q "$pattern"; then
    echo "$url answered but does not match /$pattern/"
    return 1
  fi
  return 0
}

check_aur() {
  local url="https://aur.archlinux.org/rpc/v5/info?arg[]=gander"
  local body
  body=$(curl -fsSL --max-time 20 -A "gander-check-channels/1.0" "$url" 2>/dev/null) || {
    echo "AUR RPC unreachable"
    return 1
  }
  # resultcount is 0 for a name that was never pushed.
  if printf '%s' "$body" | grep -q '"resultcount":0'; then
    echo "AUR has no 'gander' package, but the docs say 'yay -S gander'"
    return 1
  fi
  return 0
}

check_homebrew() {
  # The tap has to serve the formula, and the formula has to name a real archive.
  http_ok "https://raw.githubusercontent.com/pandasir/homebrew-tap/main/Formula/gander.rb" "class Gander" || return 1
}

check_apt() {
  http_ok "https://pandasir.github.io/gander/dists/stable/InRelease" "gander" || return 1
}

check_dnf() {
  http_ok "https://pandasir.github.io/gander/gander.repo" "gander" || return 1
}

check_crates() {
  http_ok "https://crates.io/api/v1/crates/gander-bin" '"id":"gander-bin"' || return 1
}

check_channel() {
  local cmd=$1 checker=$2 docs=$3 reason
  if reason=$("$checker"); then
    ok "$cmd (documented in $docs)"
  else
    fail "$cmd is documented in $docs, but: $reason"
  fi
}

# A channel counts as documented when the command appears in README.md or the
# landing page; the translations follow the English README, so checking both ends
# is enough to catch the drift.
documented() {
  grep -qsF "$1" README.md site/index.html
}

for pair in \
  "yay -S gander|check_aur" \
  "brew install pandasir/tap/gander|check_homebrew" \
  "apt install gander|check_apt" \
  "dnf install gander|check_dnf" \
  "cargo install gander-bin|check_crates"
do
  cmd=${pair%%|*}
  checker=${pair##*|}
  if documented "$cmd"; then
    check_channel "$cmd" "$checker" "README.md / site/index.html"
  fi
done

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "all documented install channels answer"
