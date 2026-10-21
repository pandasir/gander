class Gander < Formula
  desc "Cross-platform interactive process viewer"
  homepage "https://github.com/pandasir/gander"
  url "https://github.com/pandasir/gander/archive/refs/tags/v0.6.2.tar.gz"
  sha256 "TO_BE_FILLED"
  license "GPL-2.0-or-later"
  head "https://github.com/pandasir/gander.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--locked", *std_cargo_args(path: "crates/bin")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/gander --version")
    assert_match "interactive process viewer", shell_output("#{bin}/gander --help")
  end
end
