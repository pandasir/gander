class Gander < Formula
  desc "Cross-platform interactive process viewer"
  homepage "https://github.com/pandasir/gander"
  license "GPL-2.0-or-later"

  stable do
    on_macos do
      on_arm do
        url "https://github.com/pandasir/gander/releases/download/v0.6.6/gander-0.6.6-aarch64-apple-darwin.tar.gz"
        sha256 "cccb144aceb8f26d7525eff1b5bda2d9a30e64ab6588dab9a755d986ca959c18"
      end
      on_intel do
        url "https://github.com/pandasir/gander/releases/download/v0.6.6/gander-0.6.6-x86_64-apple-darwin.tar.gz"
        sha256 "db19e99011a6251d530ee55d8b23af378b358202d8e2b4e9a7cf3feaa163603f"
      end
    end

    on_linux do
      on_intel do
        url "https://github.com/pandasir/gander/releases/download/v0.6.6/gander-0.6.6-x86_64-unknown-linux-musl.tar.gz"
        sha256 "7e397574300f9e4f0874c30851a2df406f7dafed53a457ddcc2885c9f07c2a8a"
      end
      on_arm do
        # v0.6.3 was the first release with an arm64 Linux archive, so this is a
        # prebuilt download like the others. Before that the formula compiled
        # from source here, which is the one platform that needed Rust.
        url "https://github.com/pandasir/gander/releases/download/v0.6.6/gander-0.6.6-aarch64-unknown-linux-musl.tar.gz"
        sha256 "b63535580862c1365ef8536d7d7c1e884951d767770498d2997c6a3748533aa5"
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
