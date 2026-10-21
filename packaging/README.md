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
