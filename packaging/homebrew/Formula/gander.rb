class Gander < Formula
  desc "Cross-platform interactive process viewer"
  homepage "https://github.com/pandasir/gander"
  license "GPL-2.0-or-later"

  stable do
    on_macos do
      on_arm do
        url "https://github.com/pandasir/gander/releases/download/v0.6.3/gander-0.6.3-aarch64-apple-darwin.tar.gz"
        sha256 "c3e2e65e41c035a8be7be49428045013432446287ae095e65036fbd61abf6c9e"
      end
      on_intel do
        url "https://github.com/pandasir/gander/releases/download/v0.6.3/gander-0.6.3-x86_64-apple-darwin.tar.gz"
        sha256 "37e4b65d76397e9c93165a2cc10dab0cac36f573c4d3257ef83b24f274146780"
      end
    end

    on_linux do
      on_intel do
        url "https://github.com/pandasir/gander/releases/download/v0.6.3/gander-0.6.3-x86_64-unknown-linux-musl.tar.gz"
        sha256 "f1f598fa606167de544d565f99be4d00697fef27faa994e76bc6723128febc3b"
      end
      on_arm do
        # v0.6.3 is the first release with an arm64 Linux archive, so this is a
        # prebuilt download like the others. Before that the formula compiled
        # from source here, which is the one platform that needed Rust.
        url "https://github.com/pandasir/gander/releases/download/v0.6.3/gander-0.6.3-aarch64-unknown-linux-musl.tar.gz"
        sha256 "b9f0aef3ac14cd2b48975573d750d26f9397811ab4c9ddc226986471ea8aed65"
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
