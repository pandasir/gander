#!/usr/bin/env bash
# Build (or refresh) the signed yum/dnf repository that serves
# `dnf install gander`.
#
# Layout:
#
#   <root>/RPM-GPG-KEY-gander                armored public key
#   <root>/gander.repo                       client configuration, for reading
#   <root>/repodata/repomd.xml               createrepo_c metadata
#   <root>/repodata/repomd.xml.asc           detached signature over repomd.xml
#   <root>/repodata/repomd.xml.key           public key apt-style clients want
#   <root>/gander-<version>-<release>.<arch>.rpm
#
# Flat layout, no subdirectories: a flat repository needs no `baseurl` guessing
# and is what `dnf config-manager --add-repo` produces by default.
#
# Usage: publish-rpm-repo.sh <root> <keep-versions>
#
# Environment:
#   GNUPGHOME   the keyring holding the signing key (set by gpg-setup.sh)
#   GPG_KEY_ID  the key to sign with (set by gpg-setup.sh)
set -euo pipefail

usage() {
  echo "usage: $0 <root> <keep-versions>" >&2
  exit 2
}

[ $# -eq 2 ] || usage
root=$1
keep=$2

: "${GNUPGHOME:?GNUPGHOME must be set by gpg-setup.sh}"
: "${GPG_KEY_ID:?GPG_KEY_ID must be set by gpg-setup.sh}"

[[ $keep =~ ^[0-9]+$ ]] || {
  echo "keep-versions must be a number, got '$keep'" >&2
  exit 2
}

if [ ! -f "$root/RPM-GPG-KEY-gander" ]; then
  echo "missing $root/RPM-GPG-KEY-gander; run gpg-setup.sh first" >&2
  exit 2
fi

# --- prune ------------------------------------------------------------------
if [ "$keep" -gt 0 ]; then
  # Version order, not filename order: `0.10.0` sorts before `0.9.0` as text.
  mapfile -t versions < <(
    for rpm in "$root"/gander-*.rpm; do
      [ -e "$rpm" ] || continue
      rpm -qp --qf '%{VERSION}\n' "$rpm" 2>/dev/null
    done | sort -u -V
  )
  if [ "${#versions[@]}" -gt "$keep" ]; then
    for version in "${versions[@]:0:$((${#versions[@]} - keep))}"; do
      echo "pruning gander $version"
      rm -f "$root"/gander-"$version"-*.rpm
    done
  fi
fi

[ -n "$(ls -A "$root"/gander-*.rpm 2>/dev/null)" ] || {
  echo "no packages in $root" >&2
  exit 1
}

# --- index ------------------------------------------------------------------
# --checksum sha256 rather than the default: dnf verifies it, and sha256 is the
# one every supported client understands.
# --compression-type gz rather than the default: newer createrepo_c (Ubuntu 26
# runners, Oct 2026) emits zchunk metadata — primary.xml.zck — while repomd
# signing below and test-repo.sh's check for the package inside primary.xml.gz
# both assume the .gz layout. Pin it so the repository shape does not drift
# with whatever distro the builder happens to run.
createrepo_c --quiet --checksum sha256 --database --compression-type gz --outputdir "$root" "$root"

# --- signatures -------------------------------------------------------------
# The repository metadata is signed, and the packages were signed when they were
# built (rpmsign in the release workflow). Signing only one of the two leaves a
# hole: repomd.xml.asc proves the index is ours, the package signature proves the
# file the index points at is ours.
gpg --batch --yes --local-user "$GPG_KEY_ID" --armor --digest-algo SHA256 \
  --detach-sign -o "$root/repodata/repomd.xml.asc" "$root/repodata/repomd.xml"

cat >"$root/gander.repo" <<'EOF'
# gander — https://github.com/pandasir/gander
#
# Install with:
#   sudo rpm --import https://pandasir.github.io/gander/RPM-GPG-KEY-gander
#   sudo dnf config-manager --add-repo https://pandasir.github.io/gander/gander.repo
#   sudo dnf install gander
[gander]
name=gander
baseurl=https://pandasir.github.io/gander/
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=https://pandasir.github.io/gander/RPM-GPG-KEY-gander
EOF

count=$(find "$root" -maxdepth 1 -name 'gander-*.rpm' | wc -l | tr -d ' ')
echo "rpm repository: $root ($count packages)"
