# Repository signing key

The apt and rpm repositories are signed. This document is the whole lifecycle of
the key that signs them: how to create it, where it lives, and what breaks if it
is lost. It is short because the key is the one thing in this project that cannot
be regenerated — a new key means every user has to import a new public key by
hand, so the goal is to create it once and never need to.

## What is signed

| Artifact | Signed by | Verified by the client |
|---|---|---|
| `dists/stable/InRelease` | this key | `apt update`, always |
| `dists/stable/Release.gpg` | this key | older apt, as a fallback |
| `repodata/repomd.xml.asc` | this key | `dnf` with `repo_gpgcheck=1` |
| `gander-*.rpm` | this key | `dnf` with `gpgcheck=1` |

`.deb` files are deliberately **not** signed. apt authenticates the index, and the
index records each package's SHA-256, so signing the package as well would protect
nothing that the index signature does not already protect.

## Creating the key

Do this once, on a machine you control, not in CI.

```bash
export GNUPGHOME=$(mktemp -d)
gpg --batch --pinentry-mode loopback --passphrase '' \
  --quick-generate-key "gander repository signing <gander-maintainers@users.noreply.github.com>" \
  rsa4096 sign never
```

`never` expiry is intentional and is the main risk accepted here: an expired key
takes down every `apt update` for every user on the expiry date, which is a worse
failure than a long-lived key kept in a secret store. Rotate deliberately, by
publishing the new key beside the old one for a release, rather than by calendar.

Then:

```bash
# The key material, and the id that names it.
gpg --armor --export-secret-keys > gander-private.asc
gpg --list-keys --with-colons | awk -F: '/^fpr/{print $10; exit}'   # the key id
```

### Encrypt it before it leaves the machine

The private key is exported with **no passphrase**, because CI signs unattended and
a protected key would prompt. That makes the exported file the most dangerous
object in this project: anyone who holds it can sign packages that every
configured machine will install. It is therefore encrypted at rest, and the
encrypted file is the only copy kept.

The same file is both the offline backup and the `GANDER_GPG_PRIVATE_KEY_ENC`
secret — one artifact rather than two that can drift apart:

```bash
gpg --batch --pinentry-mode loopback --passphrase env:GANDER_PASS \
  --armor --symmetric --cipher-algo AES256 \
  --s2k-mode 3 --s2k-digest-algo SHA512 --s2k-count 65011712 \
  -o gander-private.asc.gpg gander-private.asc
```

AES-256 with PBKDF2 and a high iteration count, because this file has to stay
expensive to guess for years. `gander-private.asc.gpg` is what you keep; the
plaintext is deleted once the encrypted file has been decrypted again and proved
to contain the same key.

### The three secrets

```bash
gh secret set GANDER_GPG_PRIVATE_KEY_ENC < gander-private.asc.gpg
gh secret set GANDER_GPG_PASSPHRASE          # gh prompts; not on the command line
gh secret set GANDER_GPG_KEY_ID --body "<the fingerprint>"
```

The passphrase protects the secret store, not the workflow: anyone who can read
both the secret and `release.yml` can sign. The boundary is GitHub's secret store
and who has admin on the repository, and the passphrase is what keeps a leaked
secret file from being immediately usable.

`gpg-setup.sh` accepts either form — `GANDER_GPG_PRIVATE_KEY_ENC` with
`GANDER_GPG_PASSPHRASE`, or a plaintext `GANDER_GPG_PRIVATE_KEY` — and refuses to
continue if the imported key is still passphrase-protected, since the later
signing steps cannot prompt.

Delete `gander-private.asc` from the machine afterwards, and keep the encrypted
file somewhere you can still reach in a year. If it is lost the published
repository keeps working — the public key is already imported by users — but no
new release can be published until the same key is restored, because signing with
a different key fails every client.

**The passphrase matters as much as the file.** An encrypted backup whose
passphrase is forgotten is a backup nobody can open, and it looks exactly like a
working one until the day it is needed.

## How CI uses it

`.github/scripts/gpg-setup.sh setup` imports the secret into a throwaway keyring,
exports `gander.gpg` and `RPM-GPG-KEY-gander` into the repository tree, and
verifies that the configured id actually matches an imported secret key. The
release workflow then signs and publishes. Nothing is generated in CI.

`gpg-setup.sh verify` checks that the published public key is the key that signed
the indexes. Run it locally against the published tree after a release:

```bash
GNUPGHOME=$(mktemp -d) GANDER_GPG_KEY_ID=<fingerprint> \
  .github/scripts/gpg-setup.sh verify /path/to/repo/tree
```

## Rotating

1. Create the new key as above and add its public half to the tree, so both keys
   are published before anything is signed with the new one.
2. Publish one release signed with the old key. Users now have the new public key
   imported and have not needed it yet.
3. Switch `GANDER_GPG_KEY_ID` to the new key and publish the next release signed
   with it.
4. Remove the old public key one release later.

Do not skip step 2. A key that appears in the same release that first uses it is
a key no client has yet verified.
