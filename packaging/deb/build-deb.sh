#!/usr/bin/env bash
# Build a Debian binary package from an already-built gander binary.
#
# The binary must be the *static* musl build. Nothing in the package links
# against the host, so the control file declares no dependencies and the package
# installs on any architecture-matched Debian, Ubuntu or derivative regardless of
# the host's glibc version. `packaging/check-packages.sh` asserts that.
#
# Usage: packaging/deb/build-deb.sh <binary> <version> <arch> <outdir> [<repo-root>]
#   arch: amd64 (x86_64) or arm64 (aarch64)
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

case "$arch" in
  amd64 | arm64) ;;
  *)
    echo "unsupported Debian architecture: $arch (want amd64 or arm64)" >&2
    exit 2
    ;;
esac

# The version lands in the control file verbatim; refuse anything that is not a
# plain upstream version rather than letting a stray character into the archive.
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

maintainer="gander contributors <gander-maintainers@users.noreply.github.com>"
# The Debian revision: packaging-only changes to the same upstream version bump
# this, exactly like the Homebrew formula's `revision`.
revision=1
deb_version="${version}-${revision}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
stage="$work/root"
docdir="$stage/usr/share/doc/gander"

install -Dm755 "$binary" "$stage/usr/bin/gander"
install -Dm644 "$root/LICENSE" "$docdir/copyright"
install -Dm644 "$root/README.md" "$docdir/README.md"

# Debian policy wants documentation compressed. `-n` keeps the gzip header free
# of a timestamp so the same input always produces the same package.
gzip -9n "$docdir/README.md"

cat >"$docdir/changelog" <<EOF
gander (${deb_version}) unstable; urgency=medium

  * Release ${version}. See the release notes for v${version}:
    https://github.com/pandasir/gander/releases/tag/v${version}

 -- ${maintainer}  $(date -u '+%a, %d %b %Y %H:%M:%S +0000')
EOF
gzip -9n "$docdir/changelog"

# Installed-Size counts the installed tree only, so measure before DEBIAN/ is
# added. `du -sk` is 1024-byte blocks on both GNU and BSD.
installed_size=$(du -sk "$stage" | cut -f1)

cat >"$work/control" <<EOF
Package: gander
Version: ${deb_version}
Architecture: ${arch}
Maintainer: ${maintainer}
Installed-Size: ${installed_size}
Section: utils
Priority: optional
Homepage: https://github.com/pandasir/gander
Description: cross-platform interactive process viewer
 gander is an interactive process viewer for Linux, macOS and FreeBSD in the
 spirit of htop: a process table with sorting, filtering, tree view, per-process
 signals, CPU affinity and I/O priority, plus header meters and a batch mode.
 .
 This package contains the statically linked binary, so it has no runtime
 dependencies and does not depend on the host's glibc.
EOF

install -Dm644 "$work/control" "$stage/DEBIAN/control"

mkdir -p "$outdir"
package="$outdir/gander_${deb_version}_${arch}.deb"
# --root-owner-group makes the archive reproducible without fakeroot: every entry
# is owned by root, whoever ran the build.
dpkg-deb --root-owner-group -Zxz --build "$stage" "$package" >/dev/null
echo "$package"
