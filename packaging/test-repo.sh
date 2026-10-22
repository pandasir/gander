#!/usr/bin/env bash
# Exercise the repository scripts without the signing key.
#
# The release workflow is the only place the real key exists, and it only runs on
# a tag. A typo in publish-apt-repo.sh would therefore be found at the worst
# possible moment, and a repository that fails to build is a repository users
# cannot install from.
#
# This test builds a throwaway key, so it proves the mechanics -- index layout,
# signatures, pruned versions, the placeholder guards -- and nothing about the
# real key.
#
# Usage: packaging/test-repo.sh <deb> <rpm>
set -euo pipefail

usage() {
  echo "usage: $0 <deb> <rpm>" >&2
  exit 2
}

[ $# -eq 2 ] || usage
deb=$1
rpm=$2

[ -f "$deb" ] && [ -f "$rpm" ] || {
  echo "need one .deb and one .rpm to publish" >&2
  exit 2
}

for tool in gpg rpmsign createrepo_c apt-ftparchive dpkg-deb gzip; do
  command -v "$tool" >/dev/null || {
    echo "$tool is required; see the packages job in .github/workflows/ci.yml" >&2
    exit 2
  }
done

here=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

export GNUPGHOME="$work/gnupg"
mkdir -p "$GNUPGHOME"
chmod 700 "$GNUPGHOME"
gpg --batch --quiet --pinentry-mode loopback --passphrase '' \
  --quick-generate-key "gander test <test@example.com>" rsa2048 sign never 2>/dev/null

export GANDER_GPG_PRIVATE_KEY
GANDER_GPG_PRIVATE_KEY=$(gpg --armor --export-secret-keys)
export GANDER_GPG_KEY_ID
GANDER_GPG_KEY_ID=$(gpg --list-keys --with-colons | awk -F: '/^fpr/{print $10; exit}')

tree="$work/tree"
mkdir -p "$tree" "$work/packages"
cp "$deb" "$rpm" "$work/packages/"

"$here/.github/scripts/gpg-setup.sh" setup "$tree" >/dev/null
# shellcheck disable=SC1091
. "$work/gander-signing.env"

"$here/packaging/sign-packages.sh" "$work/packages"/*.rpm
"$here/.github/scripts/merge-repo.sh" "$work/packages" "$tree" 5 >/dev/null
"$here/.github/scripts/gpg-setup.sh" verify "$tree" >/dev/null

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# --- layout -----------------------------------------------------------------
for path in \
  gander.gpg RPM-GPG-KEY-gander gander.repo \
  dists/stable/Release dists/stable/InRelease dists/stable/Release.gpg \
  repodata/repomd.xml repodata/repomd.xml.asc; do
  [ -f "$tree/$path" ] || fail "missing $path"
done

arch=$(dpkg-deb --field "$deb" Architecture)
[ -f "$tree/dists/stable/main/binary-$arch/Packages" ] ||
  fail "no Packages index for $arch"

# --- signatures verify against the published key ----------------------------
# A client imports exactly this file, so this is the chain that has to work.
verify_home="$work/verify"
mkdir -p "$verify_home"
chmod 700 "$verify_home"
GNUPGHOME="$verify_home" gpg --batch --quiet --import "$tree/gander.gpg" 2>/dev/null
GNUPGHOME="$verify_home" gpg --batch --quiet --verify "$tree/dists/stable/InRelease" >/dev/null 2>&1 ||
  fail "InRelease does not verify against the published key"
GNUPGHOME="$verify_home" gpg --batch --quiet --verify "$tree/dists/stable/Release.gpg" \
  "$tree/dists/stable/Release" >/dev/null 2>&1 ||
  fail "Release.gpg does not verify"
GNUPGHOME="$verify_home" gpg --batch --quiet --verify "$tree/repodata/repomd.xml.asc" \
  "$tree/repodata/repomd.xml" >/dev/null 2>&1 ||
  fail "repomd.xml.asc does not verify"

# --- the index describes the package that is actually there -----------------
grep -q "$(basename "$deb")" "$tree/dists/stable/main/binary-$arch/Packages" ||
  fail "Packages does not mention $(basename "$deb")"
grep -q "Filename: pool/main/g/gander/$(basename "$deb")" \
  "$tree/dists/stable/main/binary-$arch/Packages" ||
  fail "Packages records the wrong Filename"

if grep -q '^Valid-Until:' "$tree/dists/stable/Release"; then
  fail "Release carries Valid-Until; it would expire clients"
fi

# --- the rpm is signed and the index points at it ---------------------------
sig=$(rpm -qp --qf '%{RSAHEADER:pgpsig}%{DSAHEADER:pgpsig}' "$tree/$(basename "$rpm")" 2>/dev/null || true)
[ -n "$sig" ] || fail "$(basename "$rpm") is not signed"
# repomd.xml only names the metadata files; the package name lives inside the
# compressed primary.xml, so search the metadata rather than the index manifest.
grep -qs "$(basename "$rpm")" <(gzip -dc "$tree"/repodata/*-primary.xml.gz) ||
  fail "repodata does not mention $(basename "$rpm")"

# --- a key change must be refused -------------------------------------------
# Rotating the key silently is the failure that breaks every existing user.
other_home="$work/other"
mkdir -p "$other_home"
chmod 700 "$other_home"
GNUPGHOME="$other_home" gpg --batch --quiet --pinentry-mode loopback --passphrase '' \
  --quick-generate-key "other <other@example.com>" rsa2048 sign never 2>/dev/null
GNUPGHOME="$other_home" GANDER_GPG_PRIVATE_KEY="$(GNUPGHOME="$other_home" gpg --armor --export-secret-keys)" \
  GANDER_GPG_KEY_ID="$(GNUPGHOME="$other_home" gpg --list-keys --with-colons | awk -F: '/^fpr/{print $10; exit}')" \
  "$here/.github/scripts/gpg-setup.sh" setup "$tree" >/dev/null 2>&1 &&
  fail "setup replaced a published key without GANDER_GPG_ALLOW_KEY_CHANGE=1"

# --- pruning keeps the newest ----------------------------------------------
# Filename order would keep 0.9.0 over 0.10.0; the index must sort by version.
# Two real packages are built rather than two copies of one: the version comes
# from the control file, so renaming a file would test nothing.
prune="$work/prune"
mkdir -p "$prune/pool/main/g/gander" "$work/payload"
dpkg-deb --extract "$deb" "$work/payload"
for version in 0.9.0 0.10.0; do
  "$here/packaging/deb/build-deb.sh" "$work/payload/usr/bin/gander" "$version" "$arch" \
    "$prune/pool/main/g/gander" >/dev/null
done
cp "$tree/gander.gpg" "$prune/"
"$here/.github/scripts/publish-apt-repo.sh" "$prune" stable 1 >/dev/null
kept=$(find "$prune/pool" -name '*.deb' -exec basename {} \;)
[ "$kept" = "gander_0.10.0-1_${arch}.deb" ] ||
  fail "pruning kept '$kept', want the gander_0.10.0-1 package"

echo "repo scripts OK: layout, signatures, indexes, key-change guard, pruning"
