#!/usr/bin/env bash
# Build an RPM package from an already-built gander binary.
#
# Same contract as packaging/deb/build-deb.sh: the binary must be the static musl
# build, so the package declares no runtime dependencies and installs on any
# architecture-matched RPM distribution.
#
# Usage: packaging/rpm/build-rpm.sh <binary> <version> <arch> <outdir> [<repo-root>]
#   arch: x86_64 or aarch64
#
# Prints the path of the package it wrote.
set -euo pipefail

usage() {
  echo "usage: $0 <binary> <version> <arch> <outdir> [<repo-root>]" >&2
  exit 2
}

[ $# -ge 4 ] || usage
binary=$1
version=$2
arch=$3
outdir=$4
root=${5:-$(cd "$(dirname "$0")/../.." && pwd)}
here=$(cd "$(dirname "$0")" && pwd)

case "$arch" in
  x86_64 | aarch64) ;;
  *)
    echo "unsupported RPM architecture: $arch (want x86_64 or aarch64)" >&2
    exit 2
    ;;
esac

if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "version must be X.Y.Z, got '$version'" >&2
  exit 2
fi

[ -f "$binary" ] || {
  echo "no binary at $binary" >&2
  exit 2
}
for file in LICENSE README.md; do
  [ -f "$root/$file" ] || {
    echo "missing $root/$file" >&2
    exit 2
  }
done

# Fedora names rpmbuild's _topdir as $HOME/rpmbuild; openSUSE does not read it.
# An explicit --define keeps both working, and `mktemp` keeps runs independent.
topdir=$(mktemp -d)
trap 'rm -rf "$topdir"' EXIT
mkdir -p "$topdir"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}

# The spec is a template: substitute the two values that cannot come from the
# command line, and leave every other %-sequence for rpmbuild to expand.
sed -e "s/@VERSION@/${version}/g" -e "s/@ARCH@/${arch}/g" \
  "$here/gander.spec" >"$topdir/SPECS/gander.spec"
install -m755 "$binary" "$topdir/SOURCES/gander"
install -m644 "$root/LICENSE" "$topdir/SOURCES/LICENSE"
install -m644 "$root/README.md" "$topdir/SOURCES/README.md"

rpmbuild --define "_topdir $topdir" \
  --define "_binary_payload w2.xzdio" \
  -bb "$topdir/SPECS/gander.spec"

mkdir -p "$outdir"
# rpmbuild writes exactly one package into RPMS/<arch>/; take it by glob rather
# than by guessing rpmbuild's filename, which varies with the distro's macro
# defaults (e.g. a `.cmake` or `1.el9` suffix).
package=$(find "$topdir/RPMS/$arch" -maxdepth 1 -name "gander-$version-*.rpm" -print -quit)
cp "$package" "$outdir/"
echo "$outdir/$(basename "$package")"
