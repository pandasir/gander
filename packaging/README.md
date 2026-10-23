# Distribution

Release sources of truth are the annotated `v*` tags. Update every version and
checksum below when cutting a release.

## Are the documented channels live?

Every install command the READMEs and the landing page print is a promise. When a
channel is missing — an AUR package that was never pushed, a tap whose formula
does not resolve — a reader who follows the docs gets an error from a command we
told them to run, and nothing in the build notices, because the code is fine.

```bash
packaging/check-channels.sh            # network checks + .SRCINFO
packaging/check-channels.sh --offline  # .SRCINFO only, no network
```

The script fails when a command in `README.md` or `site/index.html` has no live
channel behind it, and when `packaging/aur/.SRCINFO` disagrees with the
`PKGBUILD` beside it. CI runs it in the `channels` job.

The AUR is a special case. Registration has been closed since the [June 2026
malicious-packages incident](https://archlinux.org/news/active-aur-malicious-packages-incident/),
and Arch's DevOps team has said there is no manual process to create an account,
so the package cannot be pushed. The check therefore accepts two states as long
as the docs and the AUR agree: no AUR package and no `yay -S gander` in the docs
(the current state, with Arch users building from `packaging/aur/` directly), or
a live AUR package that the docs name. It fails on the mismatch in either
direction, and it fails when the AUR cannot be reached, so an outage cannot read
as a pass.

## crates.io

The registry name `gander` is already occupied by an unrelated, yanked crate, so
the executable is published as `gander-bin`. It still installs a binary named
`gander`.

Publish the workspace crates in dependency order, waiting for the index to
update between steps:

```bash
cargo publish -p gander-core
cargo publish -p gander-config
cargo publish -p gander-tui
cargo publish -p gander-bin
```

Users install with:

```bash
cargo install gander-bin
```

## Homebrew

`packaging/homebrew/Formula/gander.rb` is the formula for the
`pandasir/homebrew-tap` repository. Create the tap once, then copy the formula
there:

```bash
gh repo create pandasir/homebrew-tap --public \
  --description "Homebrew formulae for pandasir projects"
git clone git@github.com:pandasir/homebrew-tap.git /tmp/homebrew-tap
mkdir -p /tmp/homebrew-tap/Formula
cp packaging/homebrew/Formula/gander.rb /tmp/homebrew-tap/Formula/
cd /tmp/homebrew-tap
git add Formula/gander.rb
git commit -m "gander 0.6.5"
git push
```

Users install with:

```bash
brew install pandasir/tap/gander
```

Stable installs on macOS (Apple Silicon and Intel) and on Linux (x86_64 and
arm64) download checksum-pinned release binaries instead of compiling locally.
They have no Rust, LLVM or other formula dependencies. Linux uses the static musl
archive on both architectures. `--HEAD` builds from source on every platform, and
is the only path that depends on Rust.

Linux arm64 became a prebuilt download in v0.6.3, the first release that shipped
`aarch64-unknown-linux-musl`; before that it was the one stable platform that
compiled. `test_formula.rb` carries a per-platform flag for this, so a future
release that drops an archive fails the check instead of silently reverting to a
source build.

After changing the formula, check the platform/dependency matrix and test the
installed tap formula:

```bash
brew ruby packaging/homebrew/test_formula.rb
brew style packaging/homebrew/Formula/gander.rb
# Copy the updated formula into the tap checkout before these commands.
brew audit --strict pandasir/tap/gander
brew deps --tree --include-build pandasir/tap/gander
brew install pandasir/tap/gander
brew test pandasir/tap/gander
```

The normal stable dependency tree on the binary platforms must contain only
`gander`, not `rust` or `llvm`. Homebrew itself may still download metadata or
update its own runtime; those are separate from gander's dependencies.

When releasing a new version, wait for the release archives to be published,
update all four binary URLs and SHA-256 values (macOS arm64 and Intel, Linux
x86_64 and arm64), and reset the formula revision to zero. For packaging-only
changes to the same release, increment `revision` so existing users receive the
update. Always sync and push `Formula/gander.rb` to `pandasir/homebrew-tap`;
changing only the copy in this repository does not change users' installs.

Users with the old source-build formula can update it with:

```bash
brew update
brew upgrade pandasir/tap/gander
```

## Debian/Ubuntu and Fedora/RHEL

`packaging/deb/` and `packaging/rpm/` build one-off installable packages from the
static musl binary. Both scripts take the same arguments, so the release workflow
runs them in lockstep:

```bash
packaging/deb/build-deb.sh target/x86_64-unknown-linux-musl/release/gander \
  0.6.4 amd64 dist
packaging/rpm/build-rpm.sh target/x86_64-unknown-linux-musl/release/gander \
  0.6.4 x86_64 dist
packaging/check-packages.sh dist/*.deb dist/*.rpm
```

That produces `gander_<version>-1_amd64.deb` and `gander-<version>-1.x86_64.rpm`
(and the `arm64`/`aarch64` pair when built from the arm64 musl archive).

Why the static binary matters here: the package declares **no dependencies**, and
`packaging/check-packages.sh` fails the build if that stops being true. A package
that quietly picks up `libc6 >= 2.34` would install fine on the build machine and
refuse to start on the CentOS 7 box someone is actually using. The check asserts
both halves — the control/`Requires` fields are empty *and* the payload's
`readelf -d` shows no `NEEDED` entries — because either one alone can be made
true while the other regresses.

These one-off files are not a repository. The signed repositories that provide
`apt install gander` are described in the next section; the files remain because
`apt install ./file.deb` is the only option on an air-gapped machine or in a
`--no-network` container.

The .deb files are unsigned. For `apt install ./file.deb` the checksum published
beside the file is what establishes provenance; a detached signature only becomes
load-bearing once users trust a key by name. In the repository, apt authenticates
the index instead, so the package needs no signature there either.

## Signed repositories

`https://pandasir.github.io/gander/` is a signed apt repository and a signed
yum/dnf repository, both served from the `repo` branch by GitHub Pages. Users get
`apt install gander` and `dnf install gander`, and upgrades arrive with the rest
of their system.

### Layout

| Path | Format |
|---|---|
| `dists/stable/InRelease`, `Release.gpg`, `Release` | apt index, signed |
| `dists/stable/main/binary-<arch>/Packages{,.gz}` | apt package index |
| `pool/main/g/gander/*.deb` | apt packages |
| `repodata/repomd.xml`, `repomd.xml.asc` | dnf metadata, signed |
| `gander-<version>-<release>.<arch>.rpm` | dnf packages, flat layout |
| `gander.gpg`, `RPM-GPG-KEY-gander` | the public key, under both names |
| `gander.repo` | the dnf client configuration |

`.deb` files are not signed individually: apt authenticates `InRelease`, which
carries each package's SHA-256, so the chain is complete. RPMs are signed as well
as the metadata, because `gpgcheck=1` is what dnf users expect.

### Scripts

| Script | Job |
|---|---|
| `.github/scripts/gpg-setup.sh` | import the key, publish its public halves, verify |
| `.github/scripts/publish-apt-repo.sh` | rebuild `Packages`/`Release`/`InRelease`, prune old versions |
| `.github/scripts/publish-rpm-repo.sh` | rebuild `repodata/`, sign `repomd.xml` |
| `.github/scripts/merge-repo.sh` | copy one release's packages into the tree, then both of the above |
| `packaging/sign-packages.sh` | sign RPMs with the repository key |
| `packaging/test-repo.sh` | the whole chain with a throwaway key, on every pull request |

Run the last one locally after touching any of the others:

```bash
packaging/test-repo.sh dist/*.deb dist/*.rpm
```

It builds a temporary key and asserts the layout, that `InRelease`,
`Release.gpg` and `repomd.xml.asc` verify against the *published* public key,
that the index records the right `Filename:`, that a key change is refused, and
that pruning keeps `0.10.0` over `0.9.0` rather than the other way round.

### The tree is carried, not regenerated

The `repo` branch is read by the release workflow, one release added to it, and
pushed back. Users pin versions; an `apt upgrade` that removes the package they
are running is a bug. `merge-repo.sh` takes the keep-count (currently 5) and the
format scripts prune anything older, so the index stays a size apt can fetch in
one request without dropping a version someone is still running.

### Publishing order

1. `build` produces the archives; `release` attaches them.
2. `packages` downloads the published `-musl` archives, builds and signs the
   packages, and checks them.
3. `publish-repo` merges them into the tree, verifies it *as a client* inside
   `debian:12` and `rockylinux:9` containers of both architectures, and pushes
   the `repo` branch.
4. `deploy-pages` publishes that branch.

Steps 3 and 4 are separate jobs so a Pages failure is visibly its own problem
rather than looking like a broken repository.

### Key management

See [docs/SIGNING.md](../docs/SIGNING.md). The short version: the key is created
once by a maintainer, stored as `GANDER_GPG_PRIVATE_KEY` and `GANDER_GPG_KEY_ID`,
and never generated in CI. `gpg-setup.sh` refuses to overwrite a published key
with a different one unless `GANDER_GPG_ALLOW_KEY_CHANGE=1`, because that silently
breaks every user who already imported it.

### Version bumps

`Version` comes from the tag, never from a second place. `gander.spec` is a
template (`@VERSION@`, `@ARCH@`) that `build-rpm.sh` fills in, because a spec
cannot take `Version:` from the command line and a package whose metadata and
binary disagree is worse than a failed build. The Debian revision (`-1`) and the
RPM `Release: 1` are the packaging revision: bump them for packaging-only changes
to an already-released upstream version, exactly like the Homebrew `revision`.

When a release ships, the workflow downloads the published `-musl` archives,
builds the four packages from those exact binaries, signs the RPMs, checks them,
installs each one in `debian:12` and `rockylinux:9` containers of the matching
architecture, attaches them to the release with a `SHA256SUMS`, and then merges
them into the signed repositories. Verify with:

```bash
gh release view v0.6.5 --json assets --jq '.assets[].name' | grep -E 'deb|rpm|SHA256SUMS'
```

## Verifying a release end to end

The archives and packages can be checked without installing anything:

```bash
# The release assets are what the repository will serve.
sha256sum -c SHA256SUMS
rpm -K gander-0.6.5-1.x86_64.rpm          # after importing RPM-GPG-KEY-gander

# The repository index verifies against the published key.
gpg --verify <(curl -fsSL https://pandasir.github.io/gander/dists/stable/InRelease)
```

## Social preview card

`assets/social-preview.png` is the 1280×640 card shown when a link to the repository
or the site is shared. It is committed, and regenerated by hand:

```
packaging/social-preview.sh      # needs Chrome; not run in CI
```

The card is edited in `packaging/social-preview.html`. Every page declares it as
`og:image` with matching `og:image:width`/`height` and an `og:image:alt`, because a
feed that gets the dimensions wrong crops the card, and one without alt text
shows a screen reader nothing.

GitHub's repository-level preview is a separate setting and is **not** in the
repository: Settings → Social preview → Upload an image. Use the same file. The
REST API does not expose that field, so `gh api` cannot set it.

Limits, from GitHub's documentation: at least 640×320 (1280×640 recommended),
PNG/JPG/GIF, under 1 MB. `packaging/check-site.sh` enforces the dimensions, the
file size and the metadata on every page.

## Site analytics

The landing page and the comparison pages load `site/analytics.js`, which sends a
page view to GoatCounter. It sets no cookies and stores no personal data, and
visitors who send `DNT` or `Global Privacy Control` are not counted at all — the
script returns before it even fetches `count.js`.

There is exactly one place the site code is written:

```js
var SITE_CODE = 'gander';   // site/analytics.js
```

**It has to be registered before any data appears.** A code that was never created
answers HTTP 400 (`no site at this domain`) and then fails silently in the reader's
browser — there is no error to notice, just an empty dashboard.

```
https://www.goatcounter.com/signup     # account name: gander
```

Registration needs an email address, a password and the human-verification field
on that form, so it cannot be automated from here. Once it exists,
`packaging/check-channels.sh` stops reporting it as a failure and every page
starts counting with no code change.

`packaging/check-site.sh` (offline) checks that every page loads the script and
that the script declares a code; `check-channels.sh` (network) checks that the
code exists on goatcounter.com. The two are separate because the first is what CI
can do without credentials and the second is the part that actually breaks.

Local previews do not pollute the data: `count.js` skips `localhost`, `127.*`,
private ranges and `file://` on its own.

## AUR

`packaging/aur/` contains the `PKGBUILD` and `.SRCINFO` for the `gander` AUR
package. `gander` was unclaimed on the AUR as of 2026-10-23.

### It cannot be pushed yet

AUR account registration has been closed since the [2026-06-12 malicious-packages
incident](https://archlinux.org/news/active-aur-malicious-packages-incident/).
Arch's DevOps team has stated there is no manual process and that registration
requests on the mailing list will be ignored, so a new package cannot be
published no matter how correct the PKGBUILD is. Registration was reopened
briefly in July 2026 and closed again.

Because of that, the READMEs and the landing page do **not** advertise
`yay -S gander`. They tell Arch users to clone the repository and run
`makepkg -si` in `packaging/aur/` instead, which builds the same package from the
same PKGBUILD. That path is verified end to end:

```bash
git clone https://github.com/pandasir/gander
cd gander/packaging/aur && makepkg -si
```

When registration reopens, push the package with the steps below and put
`yay -S gander` back in the READMEs and the landing page.
`packaging/check-channels.sh` enforces that pairing in both directions: it fails
if the docs name an AUR package that does not exist, and it fails if the AUR
package exists but the docs never mention it.

### Before pushing

`.SRCINFO` is generated, not hand-written, and the AUR rejects a push whose
`.SRCINFO` disagrees with its `PKGBUILD`. Regenerate it from the PKGBUILD in the
same commit that changes the PKGBUILD:

```bash
cd packaging/aur
makepkg --printsrcinfo > .SRCINFO
```

The package builds and installs on x86_64 and aarch64. Both were verified end to
end in a clean `archlinux` container (`makepkg --syncdeps`, then `pacman -U`, then
`gander --version` and `gander --json --limit 2`).

Note that `Cargo.toml` sets `strip = true` for the release profile. The PKGBUILD
overrides that with `CARGO_PROFILE_RELEASE_STRIP=none` so the `-debug` package
makepkg splits off carries real debuginfo instead of a 3 KB stub; makepkg strips
the binary itself.

### Pushing

The first push creates the package. The AUR has no web form for a new package:
the name is created by the first push to a repository with that name, so the
clone below is expected to warn that it is empty.

```bash
# One-time: register at https://aur.archlinux.org/register/ and paste the
# contents of the public key into My Account -> SSH Public Key.
ssh-keygen -t ed25519 -f ~/.ssh/id_ed25519_aur   # if you have none
cat ~/.ssh/id_ed25519_aur.pub                     # paste this into the AUR

git -c init.defaultBranch=master clone ssh://aur@aur.archlinux.org/gander.git /tmp/aur-gander
cp packaging/aur/PKGBUILD packaging/aur/.SRCINFO /tmp/aur-gander/
cd /tmp/aur-gander
git add PKGBUILD .SRCINFO
git commit -m "gander 0.6.5"
git push
```

A push that only updates an existing package does not need the `clone` line.
After the push, the package page is live immediately; `yay -S gander` works once
the AUR's cgit index picks up the commit, which is normally a few seconds.

`ssh -T aur@aur.archlinux.org` is the cheapest way to test the key before a push.
With a registered key it prints a welcome line and exits; unregistered keys get
`Permission denied (publickey)` even though the host is reachable. Only the
second message means the key is not on the account — a failed TCP connection is a
network problem, not an authentication one.

Users install with:

```bash
yay -S gander
```

## FreeBSD

The port files are in `packaging/freebsd/`: `Makefile`, `distinfo`, `pkg-descr`.
They are not submitted yet — see "Submitting" below for what is left.

### Why the files are generated

A Rust port's `distinfo` carries a SHA-256 for every crate in `Cargo.lock` — 199
entries here, plus the GitHub tarball. Those values can only be produced by the
ports framework:

- `make cargo-crates` reads `Cargo.lock` through `Mk/Scripts/cargo-crates.awk`
- `make makesum` fetches every crate and hashes it

Neither has an equivalent outside FreeBSD, so `packaging/freebsd/gen-port.sh` runs
inside one. Do not hand-edit `distinfo`: a single wrong checksum fails the build
with one error that names no others, and the file goes stale whenever any
dependency moves.

```bash
git clone --depth 1 https://git.FreeBSD.org/ports.git /tmp/ports
sh packaging/freebsd/gen-port.sh /tmp/ports
```

`.github/workflows/freebsd-port.yml` does this on a FreeBSD 14.5 VM for free and
uploads the result; that is how the committed files were produced.

Three things in the `Makefile` are not obvious and were each wrong at least once:

| Setting | Why |
|---|---|
| `CARGO_INSTALL_PATH=crates/bin` | The workspace root is a *virtual* manifest and `cargo install` refuses it. The framework variable is `CARGO_INSTALL_PATH`; `CARGO_INSTALL_PACKAGES` does not exist and is silently ignored. |
| `CARGO_CRATES=` inserted by the script | `make cargo-crates` prints the block to stdout, it does not edit the `Makefile`. |
| nothing else before `.include` | The block must precede the include or the framework sees no dependencies. |

The script also strips the `===>` banner lines that `make cargo-crates` writes to
stdout. That matters more than it looks: `make` treats an unrecognised line as a
comment, so a `Makefile` polluted with them still builds, stages and packages
correctly and is only rejected on review.

### Submitting

Not done yet. It needs:

1. A Bugzilla account, and a PR at <https://bugs.freebsd.org/submit/> with
   product "Ports & Packages", component "Individual Port(s)".
2. Summary in the form `[NEW PORT] sysutils/gander <short description>`.
3. The three files as a `git format-patch` diff, since bugzilla wants a patch and
   not a tarball.

Review time is "a few days to a few months". Nothing is blocked on it: the release
archives already work on FreeBSD, and `freshports.org/sysutils/gander` will 404
until a committer takes it.

### After it lands

Bump `DISTVERSION` and regenerate `distinfo` on every release. `pkg-descr` and the
`COMMENT` line rarely change; the dependency list changes whenever `Cargo.lock`
does.

## nixpkgs

The upstream source builds with the existing `rustPlatform.buildRustPackage`
pattern. Submit `package.nix` through a nixpkgs pull request after the crates.io
and Homebrew releases are visible, so reviewers can point to an established
distribution path.
