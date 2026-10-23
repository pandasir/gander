class Gander < Formula
  desc "Cross-platform interactive process viewer"
  homepage "https://github.com/pandasir/gander"
  license "GPL-2.0-or-later"

  stable do
    on_macos do
      on_arm do
        url "https://github.com/pandasir/gander/releases/download/v0.6.5/gander-0.6.5-aarch64-apple-darwin.tar.gz"
        sha256 "27bde03c98e84f0b5de2dd1ba326e454291e4f7a7774aff0b2e1a55ed6cf90a3"
      end
      on_intel do
        url "https://github.com/pandasir/gander/releases/download/v0.6.5/gander-0.6.5-x86_64-apple-darwin.tar.gz"
        sha256 "4eb5ae1f3733e625ab38c60dbdd9b4c8845c12dff3987cc6d37b64ab968bec51"
      end
    end

    on_linux do
      on_intel do
        url "https://github.com/pandasir/gander/releases/download/v0.6.5/gander-0.6.5-x86_64-unknown-linux-musl.tar.gz"
        sha256 "a4eb2a43ce0943cb0fd6e6b4b722d10fc47178d871361b72e4b460bfb64e9f41"
      end
      on_arm do
        # v0.6.3 was the first release with an arm64 Linux archive, so this is a
        # prebuilt download like the others. Before that the formula compiled
        # from source here, which is the one platform that needed Rust.
        url "https://github.com/pandasir/gander/releases/download/v0.6.5/gander-0.6.5-aarch64-unknown-linux-musl.tar.gz"
        sha256 "d94b03b69e47254a6e748217a0fde4cdbf1ad454b7dd11983d2a98cbca2fd15d"
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
