#!/usr/bin/env bash
# Build (or refresh) the signed apt repository that serves `apt install gander`.
#
# The repository is a plain directory tree that can be published by any static
# host. Layout:
#
#   <root>/gander.gpg                                  armored public key
#   <root>/dists/<suite>/Release                      apt-ftparchive release
#   <root>/dists/<suite>/InRelease                    Release, clearsigned
#   <root>/dists/<suite>/Release.gpg                  Release, detached signature
#   <root>/dists/<suite>/main/binary-<arch>/Packages{,.gz}
#   <root>/pool/main/g/gander/*.deb
#
# Usage: publish-apt-repo.sh <root> <suite> <keep-versions>
#
# Environment:
#   GNUPGHOME   the keyring holding the signing key (set by gpg-setup.sh)
#   GPG_KEY_ID  the key to sign with (set by gpg-setup.sh)
#
# The pool must already contain the .deb files to publish. Older versions beyond
# <keep-versions> are pruned: an apt index is read by every client on every
# `apt update`, and a repository that keeps every release forever grows an index
# nobody needs. `0` keeps everything.
set -euo pipefail

usage() {
  echo "usage: $0 <root> <suite> <keep-versions>" >&2
  exit 2
}

[ $# -eq 3 ] || usage
root=$1
suite=$2
keep=$3

: "${GNUPGHOME:?GNUPGHOME must be set by gpg-setup.sh}"
: "${GPG_KEY_ID:?GPG_KEY_ID must be set by gpg-setup.sh}"

[[ $suite =~ ^[a-z][a-z0-9-]*$ ]] || {
  echo "suite must be lowercase alphanumeric with dashes, got '$suite'" >&2
  exit 2
}
[[ $keep =~ ^[0-9]+$ ]] || {
  echo "keep-versions must be a number, got '$keep'" >&2
  exit 2
}

pool="$root/pool/main/g/gander"
mkdir -p "$pool"

if [ ! -f "$root/gander.gpg" ]; then
  echo "missing $root/gander.gpg; run gpg-setup.sh first" >&2
  exit 2
fi

# --- prune ------------------------------------------------------------------
# Sort by version, not by filename: `sort` would order 0.10.0 before 0.9.0 and
# drop the newest package instead of the oldest.
if [ "$keep" -gt 0 ]; then
  mapfile -t versions < <(
    for deb in "$pool"/*.deb; do
      [ -e "$deb" ] || continue
      dpkg-deb --field "$deb" Version
    done | sort -u -V
  )
  if [ "${#versions[@]}" -gt "$keep" ]; then
    for version in "${versions[@]:0:$((${#versions[@]} - keep))}"; do
      echo "pruning gander $version"
      rm -f "$pool"/gander_"$version"_*.deb
    done
  fi
fi

# --- indexes ----------------------------------------------------------------
# Architecture directories are derived from the packages rather than passed in,
# so a new architecture cannot be added to the pool and then silently left out of
# the index.
mapfile -t arches < <(
  for deb in "$pool"/*.deb; do
    [ -e "$deb" ] || continue
    dpkg-deb --field "$deb" Architecture
  done | sort -u
)
[ "${#arches[@]}" -gt 0 ] || {
  echo "no packages in $pool" >&2
  exit 1
}

dists="$root/dists/$suite"
rm -rf "$dists"
for arch in "${arches[@]}"; do
  mkdir -p "$dists/main/binary-$arch"
  # Run from the repository root so `Filename:` is recorded as
  # `pool/main/g/gander/...`, which apt resolves against the repository URL.
  (cd "$root" && apt-ftparchive packages pool) >"$dists/main/binary-$arch/Packages"
  gzip -9nc "$dists/main/binary-$arch/Packages" >"$dists/main/binary-$arch/Packages.gz"
done

(cd "$root" && apt-ftparchive \
  -o APT::FTPArchive::Release::Origin=gander \
  -o APT::FTPArchive::Release::Label=gander \
  -o APT::FTPArchive::Release::Suite="$suite" \
  -o APT::FTPArchive::Release::Codename="$suite" \
  -o APT::FTPArchive::Release::Architectures="${arches[*]}" \
  -o APT::FTPArchive::Release::Components=main \
  -o APT::FTPArchive::Release::Description="gander packages" \
  release "dists/$suite") >"$dists/Release"

# No Valid-Until on purpose. The value would have to be longer than the gap
# between releases, and a stale one turns every client's `apt update` into a
# hard failure rather than a warning about an old index.
if grep -q '^Valid-Until:' "$dists/Release"; then
  echo "Release unexpectedly carries Valid-Until; see the comment above" >&2
  exit 1
fi

# --- signatures -------------------------------------------------------------
# InRelease is the clearsigned one modern apt prefers; Release.gpg is the
# detached form older apt reads. Both are produced so that a client which does
# not understand InRelease still verifies rather than silently skipping.
gpg --batch --yes --local-user "$GPG_KEY_ID" --digest-algo SHA256 \
  --clearsign -o "$dists/InRelease" "$dists/Release"
gpg --batch --yes --local-user "$GPG_KEY_ID" --digest-algo SHA256 \
  --armor --detach-sign -o "$dists/Release.gpg" "$dists/Release"

count=$(find "$pool" -maxdepth 1 -name '*.deb' | wc -l | tr -d ' ')
echo "apt repository: $root (suite $suite, arch ${arches[*]}, $count packages)"
