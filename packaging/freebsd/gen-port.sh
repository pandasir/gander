#!/bin/sh
#
# Generate the FreeBSD port's dependency list, and verify the port builds.
#
# Run under FreeBSD, from the root of a ports tree:
#
#     sh packaging/freebsd/gen-port.sh /usr/ports
#
# It writes ${PORTDIR} (a new sysutils/gander) and then builds the port in a
# clean environment. Everything here is what `freebsd-ports` requires of a Rust
# port; none of it can be produced from Linux, because `make cargo-crates` reads
# the real Cargo.lock through the ports framework and `make makesum` fetches every
# crate to hash it.
#
# Why a script rather than a committed distinfo: the dependency list is 200
# crates, and it changes whenever anything in Cargo.lock changes. A stale
# distinfo is not a cosmetic problem — it fails the build with a checksum
# mismatch that names one crate and hides the rest.
set -eu

portsdir=${1:?usage: gen-port.sh <ports-tree>}
portdir="${portsdir}/sysutils/gander"
here=$(cd "$(dirname "$0")/../.." && pwd)

# The ports tree may be given as a relative path (the CI workflow clones it into
# the workspace so the action syncs it back). make(1) needs to be run from inside
# the port directory, so resolve it once and work with that.

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "${here}/Cargo.toml" | head -1)
[ -n "$version" ] || { echo "cannot read the version from Cargo.toml" >&2; exit 1; }

echo "==> writing ${portdir} for gander ${version}"
mkdir -p "${portdir}"

cat > "${portdir}/Makefile" <<EOF
PORTNAME=	gander
DISTVERSIONPREFIX=	v
DISTVERSION=	${version}
CATEGORIES=	sysutils

MAINTAINER=	839027631@qq.com
COMMENT=	Interactive process viewer for the terminal
WWW=		https://github.com/pandasir/gander

LICENSE=	GPLv2+
LICENSE_FILE=	\${WRKSRC}/LICENSE

USES=		cargo
USE_GITHUB=	yes
GH_ACCOUNT=	pandasir

# This is a Cargo workspace: the top-level Cargo.toml is a virtual manifest, and
# `cargo install` refuses it with "found a virtual manifest at ... instead of a
# package manifest". Point the install at the crate that produces the binary.
#
# The framework's variable is CARGO_INSTALL_PATH — it becomes `cargo install
# --path ...` — not CARGO_INSTALL_PACKAGES, which does not exist in
# Mk/Uses/cargo.mk. Getting this wrong compiles all 200 crates successfully and
# then fails at stage with a message about a virtual manifest.
CARGO_INSTALL_PATH=	crates/bin

PLIST_FILES=	bin/gander

.include <bsd.port.mk>
EOF

cat > "${portdir}/pkg-descr" <<'EOF'
gander is an interactive process viewer for the terminal, in the tradition of
top(1) and htop(1): it shows the processes on the machine and what they cost, and
it runs on Linux, macOS and FreeBSD from one codebase.

It differs from htop in layout rather than in data. Where htop devotes a second
screen to one process and makes you leave the list to read it, gander keeps the
list and puts a live inspector beside it, so the details of whatever the cursor is
on are always visible. The keybindings are htop's, and an existing htoprc can be
imported, so the transition is mostly habit rather than relearning.

Features:

  - Process table with tree mode, filtering and search, sortable on any column
  - Inspector panel showing the selected process's memory, threads, open files,
    affinity and I/O priority, without leaving the list
  - Meters for CPU, memory, swap and load, each with its own display mode
  - htoprc import, and a TOML config of its own
  - Batch mode (--json) for scripting, with no TUI attached

The FreeBSD backend reads sysctl, libkvm and the kinfo_proc structures directly,
which is also what makes it work without procfs mounted.
EOF

# `make cargo-crates` prints the block to stdout; it does not edit the Makefile.
# (Mk/Uses/cargo.mk: `@${_CARGO_AWK} .../cargo-crates.awk ${CARGO_CARGOLOCK}`.)
# The block has to be inserted here, and it must go *after* the last variable and
# before any target, which for this Makefile means immediately before the
# `.include <bsd.port.mk>` line.
#
# `make makesum` then fetches every crate and writes distinfo. Neither step has an
# equivalent outside FreeBSD.
echo "==> generating CARGO_CRATES"
# Resolve to an absolute path before changing directory: the CI run passes a
# relative ports tree (it clones into the workspace so the action syncs it back),
# and `cd relative/dir` followed by `relative/dir/Makefile` resolves against the
# new directory, not the original one. That is what broke the second run — the
# crates were found (199 of them) and then written to a path that did not exist.
portdir=$(cd "${portdir}" && pwd)
cd "${portdir}"
if ! make cargo-crates > /tmp/cargo-crates.txt 2>/tmp/cargo-crates.err; then
  echo "make cargo-crates failed:" >&2
  cat /tmp/cargo-crates.err >&2
  exit 1
fi

# cargo-crates prints banner lines (===> ...) on stderr and the block on stdout.
# Check the result is what we asked for, rather than inserting an empty block and
# calling it done.
crates=$(sed '/^\.include /d' /tmp/cargo-crates.txt)
if ! printf '%s\n' "$crates" | grep -q '^CARGO_CRATES='; then
  echo "make cargo-crates produced no CARGO_CRATES assignment" >&2
  echo "--- stdout was ---" >&2
  head -20 /tmp/cargo-crates.txt >&2
  echo "--- stderr was ---" >&2
  head -20 /tmp/cargo-crates.err >&2
  exit 1
fi

count=$(printf '%s\n' "$crates" | grep -o ' \\$' | wc -l | tr -d ' ')
echo "    ${count} crates"

# Insert the block before the include, so a rebuild of this script does not
# duplicate it in a Makefile that already has one.
#
# awk cannot take multi-line text as a -v assignment (it fails with "newline in
# string"), so the block goes through a file that awk reads line by line.
printf '%s\n' "$crates" > /tmp/cargo-crates-block.txt
awk '
  /^\.include <bsd\.port\.mk>/ && !done {
    while ((getline line < "/tmp/cargo-crates-block.txt") > 0) print line
    close("/tmp/cargo-crates-block.txt")
    print ""
    done = 1
  }
  { print }
' "${portdir}/Makefile" > "${portdir}/Makefile.new" &&
  mv "${portdir}/Makefile.new" "${portdir}/Makefile"

grep -q '^CARGO_CRATES=' "${portdir}/Makefile" || {
  echo "failed to insert CARGO_CRATES into the Makefile" >&2
  exit 1
}

echo "==> generating distinfo"
make makesum

echo "==> checking the port with portlint"
if command -v portlint >/dev/null 2>&1; then
  portlint -A -C . || echo "portlint reported warnings (not fatal here)"
else
  echo "portlint not installed; skipping (pkg install ports-mgmt/portlint)"
fi

echo "==> building, staging and packaging"
make clean
make stage
make stage-qa
make package

echo "==> installed files"
make -V PLIST_FILES

echo "done: ${portdir}"
