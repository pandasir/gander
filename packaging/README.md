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
has no release binary yet and continues to build from source; `--HEAD` also builds
from source on every platform. Only those source builds depend on Rust.

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
update all three binary URLs and SHA-256 values plus the Linux ARM source URL
and checksum, and reset the formula revision to zero. For packaging-only changes
to the same release, increment `revision` so existing users receive the update.
Always sync and push `Formula/gander.rb` to `pandasir/homebrew-tap`; changing only
the copy in this repository does not change users' installs.

Users with the old source-build formula can update it with:

```bash
brew update
brew upgrade pandasir/tap/gander
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
