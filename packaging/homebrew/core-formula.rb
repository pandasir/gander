# Draft formula for a future homebrew-core PR. NOT for the tap — the tap
# (pandasir/homebrew-tap) installs prebuilt archives; homebrew-core policy
# requires building from source, which is why this file looks different.
#
# Why it sits here unsubmitted: homebrew-core enforces a notability requirement
# with no published number, and formulae from projects this small are routinely
# declined. Submit when the Show HN / Reddit push in marketing/README.md has
# produced visible adoption (a few hundred stars and real tap install counts),
# then paste this into a PR against Homebrew/homebrew-core, replacing the
# sha256 below with the v<N> source archive checksum (same tarball the AUR
# PKGBUILD pins; regenerate with `curl -sL <url> | shasum -a 256`).
#
# Notes for the PR:
# - `depends_on "rust"` pulls core's rust, whose version may lag upstream; if
#   the build fails there, add a version constraint rather than vendoring.
# - The test block mirrors the tap's formula so both check `--json` output,
#   which exercises the real sampling path instead of only the version string.

class Gander < Formula
  desc "Cross-platform interactive process viewer that focuses on one process"
  homepage "https://pandasir.github.io/gander"
  url "https://github.com/pandasir/gander/archive/refs/tags/v0.6.7.tar.gz"
  sha256 "fd86744680b35cb12dc7fa4f4d05031ed68ce0ca52d9eda6439fad8dcf887b5f"
  license "GPL-2.0-or-later"
  head "https://github.com/pandasir/gander.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/bin")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/gander --version")
    assert_match "interactive process viewer", shell_output("#{bin}/gander --help")
    assert_match "[", shell_output("#{bin}/gander --json --limit 1")
  end
end
