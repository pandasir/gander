class Gander < Formula
  desc "Cross-platform interactive process viewer"
  homepage "https://github.com/pandasir/gander"
  license "GPL-2.0-or-later"

  stable do
    on_macos do
      on_arm do
        url "https://github.com/pandasir/gander/releases/download/v0.6.7/gander-0.6.7-aarch64-apple-darwin.tar.gz"
        sha256 "62234cb41c03becf734b4313af0f90a22736658bd88505462445dfef71f86167"
      end
      on_intel do
        url "https://github.com/pandasir/gander/releases/download/v0.6.7/gander-0.6.7-x86_64-apple-darwin.tar.gz"
        sha256 "bdef40db860149b7323403040b95f027c3165431223d3e24821b61719e45c081"
      end
    end

    on_linux do
      on_intel do
        url "https://github.com/pandasir/gander/releases/download/v0.6.7/gander-0.6.7-x86_64-unknown-linux-musl.tar.gz"
        sha256 "7569654ed553dfe0fd8d29d8a1dfc7ab500771d82849ad014878a619304b1b64"
      end
      on_arm do
        # v0.6.3 was the first release with an arm64 Linux archive, so this is a
        # prebuilt download like the others. Before that the formula compiled
        # from source here, which is the one platform that needed Rust.
        url "https://github.com/pandasir/gander/releases/download/v0.6.7/gander-0.6.7-aarch64-unknown-linux-musl.tar.gz"
        sha256 "573ecfe6a8e9179fb8d7cfddac33f799a62abcc50ad5b0ef40cd2ee2577f035f"
      end
    end
  end

  head do
    url "https://github.com/pandasir/gander.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    # Every stable install is a prebuilt archive; only --HEAD compiles.
    if build.head?
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
