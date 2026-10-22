#!/usr/bin/env bash
# Sign packages with the repository key.
#
# Only .rpm needs this. A .deb carries no signature of its own: apt authenticates
# the *index* (InRelease), and the index carries each package's SHA-256, so the
# chain from the signing key to the bytes is complete without signing the .deb.
# rpm has the same index-level check (repo_gpgcheck) plus a per-package one
# (gpgcheck), and dnf users expect the second, so the packages are signed too.
#
# Usage: sign-packages.sh <package>...
#
# Environment:
#   GNUPGHOME   the keyring holding the signing key
#   GPG_KEY_ID  the key to sign with
set -euo pipefail

[ $# -ge 1 ] || {
  echo "usage: $0 <package>..." >&2
  exit 2
}

: "${GNUPGHOME:?GNUPGHOME must be set}"
: "${GPG_KEY_ID:?GPG_KEY_ID must be set}"

if ! command -v rpmsign >/dev/null; then
  echo "rpmsign is required to sign RPMs (install rpm-sign or rpm)" >&2
  exit 2
fi

# Debian and Ubuntu package rpm with `__gpg` pointing at `/usr/bin/gpg2`, a name
# their gnupg does not install, so rpmsign fails with "Could not exec gpg". The
# override is harmless on distributions where gpg2 does exist.
gpg_bin=$(command -v gpg)

signed=0
for package in "$@"; do
  case "$package" in
    *.rpm)
      rpmsign --define "_gpg_name $GPG_KEY_ID" \
        --define "_gpg_path $GNUPGHOME" \
        --define "__gpg $gpg_bin" \
        --addsign "$package" >/dev/null
      # A signature that did not land is worse than none: the repository would
      # advertise gpgcheck=1 and then fail for every user.
      #
      # rpm 4.14+ stores a header signature in the RSAHEADER (or DSAHEADER) tag
      # rather than the older SIGPGP/SIGGPG pair, so checking only the old tags
      # reports "none" for a correctly signed package -- and would pass a package
      # whose signature silently failed.
      if ! rpm -qp --qf '%{RSAHEADER:pgpsig}%{DSAHEADER:pgpsig}%{SIGPGP:pgpsig}%{SIGGPG:pgpsig}' \
        "$package" 2>/dev/null | grep -q .; then
        echo "$package: signing produced no signature" >&2
        exit 1
      fi
      signed=$((signed + 1))
      ;;
    *.deb)
      # Nothing to do; see the comment at the top.
      ;;
    *)
      echo "$package: not a .rpm or .deb" >&2
      exit 2
      ;;
  esac
done

echo "signed $signed RPM(s)"
