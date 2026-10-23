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

# Only the binary crate is built and installed. The workspace's other crates are
# libraries this one links against, so they arrive through the same Cargo.lock and
# do not need their own port.
CARGO_INSTALL_PACKAGES=	gander-bin

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

# `make cargo-crates` appends the CARGO_CRATES block by reading Cargo.lock through
# the ports framework; `make makesum` fetches every crate and hashes it. Neither
# step has an equivalent outside FreeBSD.
echo "==> generating CARGO_CRATES"
cd "${portdir}"
make cargo-crates > /tmp/cargo-crates.txt 2>/tmp/cargo-crates.err || {
  echo "make cargo-crates failed:" >&2
  cat /tmp/cargo-crates.err >&2
  exit 1
}

# The output is now also written into the Makefile by the framework; check it
# actually landed there rather than trusting the exit status.
if ! grep -q '^CARGO_CRATES=' Makefile; then
  echo "make cargo-crates did not add a CARGO_CRATES block to the Makefile" >&2
  echo "--- it printed instead: ---" >&2
  head -5 /tmp/cargo-crates.txt >&2
  exit 1
fi

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
