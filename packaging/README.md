# Distribution

Release sources of truth are the annotated `v*` tags. Update every version and
checksum below when cutting a release.

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
git commit -m "gander 0.6.2"
git push
```

Users install with:

```bash
brew install pandasir/tap/gander
```

Stable installs on macOS (Apple Silicon and Intel) and Linux x86_64 download
checksum-pinned release binaries instead of compiling locally. They have no Rust,
LLVM or other formula dependencies. Linux uses the static musl archive. Linux ARM
still has no release binary for v0.6.2 and continues to build from source; the
first release that ships `aarch64-unknown-linux-musl` should move it to a prebuilt
URL. `--HEAD` also builds from source on every platform. Only those source builds
depend on Rust.

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
update all four binary URLs and SHA-256 values (macOS arm64/Intel, Linux x86_64,
and Linux arm64 once that archive exists) plus the source URL and checksum for any
platform still compiling, and reset the formula revision to zero. For
packaging-only changes to the same release, increment `revision` so existing users
receive the update. Always sync and push `Formula/gander.rb` to
`pandasir/homebrew-tap`; changing only the copy in this repository does not change
users' installs.

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
  0.6.2 amd64 dist
packaging/rpm/build-rpm.sh target/x86_64-unknown-linux-musl/release/gander \
  0.6.2 x86_64 dist
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

This is not a repository. There is no `apt install gander` and no `apt upgrade`
path; users download the file and install it by path. That is deliberate for now:
a repository means hosting and signing `Release`/`InRelease` (apt) and
`repodata/` plus package signatures (dnf), a key that has to be kept alive across
releases, and a publish order that has to be right or `apt update` sees a
half-written index. Until there is a reason to pay that cost, the checksummed
file is the honest product.

The packages are unsigned for the same reason. For `apt install ./file.deb` the
checksum published beside the file is what establishes provenance; a detached
signature only becomes load-bearing once users trust a key by name.

### Version bumps

`Version` comes from the tag, never from a second place. `gander.spec` is a
template (`@VERSION@`, `@ARCH@`) that `build-rpm.sh` fills in, because a spec
cannot take `Version:` from the command line and a package whose metadata and
binary disagree is worse than a failed build. The Debian revision (`-1`) and the
RPM `Release: 1` are the packaging revision: bump them for packaging-only changes
to an already-released upstream version, exactly like the Homebrew `revision`.

When a release ships, the workflow downloads the published `-musl` archives,
builds the four packages from those exact binaries, checks them, installs each one
in `debian:12` and `rockylinux:9` containers of the matching architecture, and only
then attaches them to the release with a `SHA256SUMS`. Verify with:

```bash
gh release view v0.6.2 --json assets --jq '.assets[].name' | grep -E 'deb|rpm|SHA256SUMS'
```

## AUR

`packaging/aur/` contains the `PKGBUILD` and `.SRCINFO` for the `gander` AUR
package. Register an AUR account and add an SSH public key, then:

```bash
git clone ssh://aur@aur.archlinux.org/gander.git /tmp/aur-gander
cp packaging/aur/PKGBUILD packaging/aur/.SRCINFO /tmp/aur-gander/
cd /tmp/aur-gander
git add PKGBUILD .SRCINFO
git commit -m "gander 0.6.2"
git push
```

Users install with:

```bash
yay -S gander
```

## nixpkgs

The upstream source builds with the existing `rustPlatform.buildRustPackage`
pattern. Submit `package.nix` through a nixpkgs pull request after the crates.io
and Homebrew releases are visible, so reviewers can point to an established
distribution path.
