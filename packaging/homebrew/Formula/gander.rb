class Gander < Formula
  desc "Cross-platform interactive process viewer"
  homepage "https://github.com/pandasir/gander"
  license "GPL-2.0-or-later"
  revision 1

  stable do
    on_macos do
      on_arm do
        url "https://github.com/pandasir/gander/releases/download/v0.6.2/gander-0.6.2-aarch64-apple-darwin.tar.gz"
        sha256 "621e1246cd513743db1d1d8dde5109f35e424266df5d678abd70925325c10783"
      end
      on_intel do
        url "https://github.com/pandasir/gander/releases/download/v0.6.2/gander-0.6.2-x86_64-apple-darwin.tar.gz"
        sha256 "8017e5134d32f2af861e16b9494e7792183eec503accd1ef111acdf4a4618f05"
      end
    end

    on_linux do
      on_intel do
        url "https://github.com/pandasir/gander/releases/download/v0.6.2/gander-0.6.2-x86_64-unknown-linux-musl.tar.gz"
        sha256 "b81f232dd27ba9bf9c391103c5bee31cfeaaff416b3da009941d63b83e6c0fcc"
      end
      on_arm do
        url "https://github.com/pandasir/gander/archive/refs/tags/v0.6.2.tar.gz"
        sha256 "b7b71b63af46c91313b7f53306dc7caa0f689358d6ac5008cc8ac5a757e7a4de"
        depends_on "rust" => :build
      end
    end
  end

  head do
    url "https://github.com/pandasir/gander.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    if build.head? || (OS.linux? && Hardware::CPU.arm?)
      system "cargo", "install", *std_cargo_args(path: "crates/bin")
    else
      bin.install "gander"
    end
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/gander --version")
    assert_match "interactive process viewer", shell_output("#{bin}/gander --help")
    assert_match "[", shell_output("#{bin}/gander --json --limit 1")
  end
end
