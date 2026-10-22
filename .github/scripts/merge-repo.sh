#!/usr/bin/env bash
# Merge one release's packages into the existing published repository tree.
#
# The tree lives on the `repo` branch and is carried between releases, so this
# script only ever *adds*: it copies the new packages in, then lets the format
# scripts rebuild the indexes around everything present. A release therefore
# never removes the versions users already have pinned.
#
# Usage: merge-repo.sh <packages-dir> <repo-tree> <keep-versions>
#
# Environment:
#   GNUPGHOME   set up by gpg-setup.sh
#   GPG_KEY_ID  set up by gpg-setup.sh
set -euo pipefail

usage() {
  echo "usage: $0 <packages-dir> <repo-tree> <keep-versions>" >&2
  exit 2
}

[ $# -eq 3 ] || usage
packages=$1
tree=$2
keep=$3

: "${GNUPGHOME:?GNUPGHOME must be set}"
: "${GPG_KEY_ID:?GPG_KEY_ID must be set}"

here=$(cd "$(dirname "$0")" && pwd)

mkdir -p "$tree/pool/main/g/gander"

deb_count=0
rpm_count=0
for package in "$packages"/*.deb; do
  [ -e "$package" ] || continue
  cp -f "$package" "$tree/pool/main/g/gander/"
  deb_count=$((deb_count + 1))
done
for package in "$packages"/*.rpm; do
  [ -e "$package" ] || continue
  # Flat layout: RPMs live at the repository root.
  cp -f "$package" "$tree/"
  rpm_count=$((rpm_count + 1))
done

[ "$deb_count" -gt 0 ] || [ "$rpm_count" -gt 0 ] || {
  echo "no .deb or .rpm in $packages" >&2
  exit 1
}

# The suite name is part of the URL users put in sources.list. `stable` is the
# only one published today; a second suite would need a second client snippet.
"$here/publish-apt-repo.sh" "$tree" stable "$keep"
"$here/publish-rpm-repo.sh" "$tree" "$keep"
