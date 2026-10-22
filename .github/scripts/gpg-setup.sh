#!/usr/bin/env bash
# Prepare the signing key for both repository formats.
#
# Two modes:
#
#   setup  — import the armored private key from the environment into a fresh
#            keyring and export the public halves into the repository tree.
#   verify — check that the keyring and the published public keys agree.
#
# Usage: gpg-setup.sh <setup|verify> <root>
#
# Environment for `setup`:
#   GANDER_GPG_PRIVATE_KEY   armored private key (a repository secret)
#   GANDER_GPG_KEY_ID        the key's fingerprint, or a unique substring of it
#
# Nothing here generates a key. A key generated inside a CI run would change on
# every release, and every user's imported copy would then be wrong; the key has
# to be created once, by a maintainer, and kept.
set -euo pipefail

usage() {
  echo "usage: $0 <setup|verify> <root>" >&2
  exit 2
}

[ $# -eq 2 ] || usage
mode=$1
root=$2

: "${GNUPGHOME:?GNUPGHOME must be set before calling this script}"

case "$mode" in
  setup)
    : "${GANDER_GPG_PRIVATE_KEY:?GANDER_GPG_PRIVATE_KEY must hold the armored private key}"
    : "${GANDER_GPG_KEY_ID:?GANDER_GPG_KEY_ID must hold the key fingerprint}"

    mkdir -p "$GNUPGHOME"
    chmod 700 "$GNUPGHOME"

    # --pinentry-mode loopback with an empty passphrase is what makes this
    # non-interactive. The key is protected by being a repository secret, not by
    # a passphrase, which is the usual trade for CI-held signing keys; a
    # passphrase would have to be stored beside it and would add nothing.
    printf '%s\n' "$GANDER_GPG_PRIVATE_KEY" |
      gpg --batch --quiet --pinentry-mode loopback --passphrase '' --import

    # Fail loudly if the configured id matches nothing: publishing a repository
    # signed by a key nobody has is worse than not publishing.
    if ! gpg --batch --list-secret-keys "$GANDER_GPG_KEY_ID" >/dev/null 2>&1; then
      echo "no secret key matches GANDER_GPG_KEY_ID='$GANDER_GPG_KEY_ID'" >&2
      gpg --list-secret-keys --with-colons | awk -F: '/^sec/{print "  " $5}' >&2
      exit 1
    fi
    GPG_KEY_ID=$GANDER_GPG_KEY_ID
    export GPG_KEY_ID

    # apt wants one armored key it can `gpg --dearmor`; dnf wants the same thing
    # under the name its documentation uses. Both are the public key only.
    #
    # Overwriting a *different* published key is the one mistake here that breaks
    # every existing user at once: their imported key stops matching, and apt
    # fails closed. Refuse unless the rotation was deliberate.
    if [ -f "$root/gander.gpg" ]; then
      published=$(gpg --batch --with-colons --import-options show-only \
        --import "$root/gander.gpg" 2>/dev/null | awk -F: '/^fpr/{print $10; exit}')
      if [ -n "$published" ] && [ "$published" != "$GPG_KEY_ID" ] &&
        [ "${GANDER_GPG_ALLOW_KEY_CHANGE:-}" != "1" ]; then
        echo "$root/gander.gpg already publishes $published, not $GPG_KEY_ID." >&2
        echo "Replacing it breaks every client that imported the old key." >&2
        echo "Follow docs/SIGNING.md ('Rotating'); set GANDER_GPG_ALLOW_KEY_CHANGE=1" >&2
        echo "only when the new key has been published alongside the old one." >&2
        exit 1
      fi
    fi
    gpg --batch --yes --armor --export "$GPG_KEY_ID" >"$root/gander.gpg"
    cp "$root/gander.gpg" "$root/RPM-GPG-KEY-gander"

    # The later scripts need these two variables. A script cannot export into its
    # caller, so write them to a file (for local use) and to $GITHUB_ENV (for the
    # workflow), rather than making every caller repeat the id by hand.
    env_file="$(dirname "$GNUPGHOME")/gander-signing.env"
    {
      echo "export GNUPGHOME=$GNUPGHOME"
      echo "export GPG_KEY_ID=$GPG_KEY_ID"
    } >"$env_file"
    if [ -n "${GITHUB_ENV:-}" ]; then
      {
        echo "GNUPGHOME=$GNUPGHOME"
        echo "GPG_KEY_ID=$GPG_KEY_ID"
      } >>"$GITHUB_ENV"
    fi

    echo "signing key ready: $(gpg --list-keys --with-colons "$GPG_KEY_ID" |
      awk -F: '/^fpr/{print $10; exit}')"
    echo "environment written to $env_file (source it before signing)"
    ;;

  verify)
    : "${GANDER_GPG_KEY_ID:?GANDER_GPG_KEY_ID must be set}"

    # The published key must be importable and must be the same key that signed
    # the indexes; a repository whose advertised key is not its signing key
    # fails every client, and it is invisible until someone tries to install.
    fingerprint=$(gpg --batch --with-colons --import-options show-only \
      --import "$root/gander.gpg" 2>/dev/null | awk -F: '/^fpr/{print $10; exit}')
    [ -n "$fingerprint" ] || {
      echo "$root/gander.gpg holds no key" >&2
      exit 1
    }
    case "$fingerprint" in
      *"$GANDER_GPG_KEY_ID"* | "$GANDER_GPG_KEY_ID"*) ;;
      *)
        echo "published key $fingerprint does not match GANDER_GPG_KEY_ID=$GANDER_GPG_KEY_ID" >&2
        exit 1
        ;;
    esac
    echo "published key matches: $fingerprint"
    ;;
  *)
    usage
    ;;
esac
