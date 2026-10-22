#!/usr/bin/env bash
# Assert the two properties the .deb and .rpm packages are sold on.
#
#   1. The payload binary links against nothing. A dynamically linked binary
#      would make the package's empty dependency list a lie and would pin a glibc
#      floor the package does not declare.
#   2. The packages themselves declare no runtime dependencies.
#
# Run it on a built package, in an environment that has the tooling
# (`dpkg-deb`/`rpm`, plus `readelf`). It exits non-zero with a reason otherwise.
#
# Usage: packaging/check-packages.sh <package>...
set -euo pipefail

[ $# -ge 1 ] || {
  echo "usage: $0 <package>..." >&2
  exit 2
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0

fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

# Print the NEEDED entries of every ELF file under a directory. Empty output
# means "statically linked".
needed_entries() {
  local dir=$1
  find "$dir" -type f -perm -u+x -print0 |
    while IFS= read -r -d '' file; do
      # Only ELF files matter; readelf errors out on anything else.
      case $(head -c 4 "$file" | od -An -tx1 | tr -d ' \n') in
        7f454c46) readelf -d "$file" 2>/dev/null | grep -E 'NEEDED|Shared library:' || true ;;
      esac
    done
}

check_deb() {
  local package=$1
  local name arch version

  name=$(dpkg-deb --field "$package" Package)
  arch=$(dpkg-deb --field "$package" Architecture)
  version=$(dpkg-deb --field "$package" Version)

  [ "$name" = "gander" ] || fail "$package: package name is '$name', want 'gander'"
  case "$arch" in
    amd64 | arm64) ;;
    *) fail "$package: architecture '$arch' is not amd64 or arm64" ;;
  esac
  [[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+-1$ ]] ||
    fail "$package: version '$version' is not X.Y.Z-1"

  local depends
  depends=$(dpkg-deb --field "$package" Depends || true)
  [ -z "$depends" ] || fail "$package: declares Depends='$depends', want none"

  local dir="$work/deb"
  mkdir -p "$dir"
  dpkg-deb --extract "$package" "$dir"
  local needed
  needed=$(needed_entries "$dir")
  [ -z "$needed" ] || fail "$package: payload is dynamically linked: $needed"

  # The binary must be executable and named the way users invoke it.
  [ -x "$dir/usr/bin/gander" ] || fail "$package: no executable /usr/bin/gander"
}

check_rpm() {
  local package=$1
  local name arch version

  name=$(rpm -qp --qf '%{NAME}' "$package")
  arch=$(rpm -qp --qf '%{ARCH}' "$package")
  version=$(rpm -qp --qf '%{VERSION}' "$package")

  [ "$name" = "gander" ] || fail "$package: package name is '$name', want 'gander'"
  case "$arch" in
    x86_64 | aarch64) ;;
    *) fail "$package: architecture '$arch' is not x86_64 or aarch64" ;;
  esac
  [[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] ||
    fail "$package: version '$version' is not X.Y.Z"

  local requires
  requires=$(rpm -qp --requires "$package" | grep -v '^rpmlib(' || true)
  [ -z "$requires" ] || fail "$package: requires $requires, want none"

  local dir="$work/rpm"
  mkdir -p "$dir"
  # rpm2cpio|cpio avoids needing the package installed, and works on any host
  # that has the two tools (the Ubuntu runner installs rpm for this).
  (cd "$dir" && rpm2cpio "$package" | cpio -idm --quiet)
  local needed
  needed=$(needed_entries "$dir")
  [ -z "$needed" ] || fail "$package: payload is dynamically linked: $needed"

  [ -x "$dir/usr/bin/gander" ] || fail "$package: no executable /usr/bin/gander"
}

for package in "$@"; do
  before=$failures
  case "$package" in
    *.deb) check_deb "$package" ;;
    *.rpm) check_rpm "$package" ;;
    *) fail "$package: not a .deb or .rpm" ;;
  esac
  # Only claim success for a package that passed every check; a partial pass is
  # worse than a plain failure because it reads as verified.
  if [ "$failures" -eq "$before" ]; then
    echo "ok: $package (no runtime dependencies, statically linked payload)"
  fi
done

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed" >&2
  exit 1
fi
