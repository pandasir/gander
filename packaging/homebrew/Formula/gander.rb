class Gander < Formula
  desc "Cross-platform interactive process viewer"
  homepage "https://github.com/pandasir/gander"
  license "GPL-2.0-or-later"

  stable do
    on_macos do
      on_arm do
        url "https://github.com/pandasir/gander/releases/download/v0.6.4/gander-0.6.4-aarch64-apple-darwin.tar.gz"
        sha256 "783dcad4f3786c232d8b12727438f15020e69a0a1eea442008e63da31cbe1ce9"
      end
      on_intel do
        url "https://github.com/pandasir/gander/releases/download/v0.6.4/gander-0.6.4-x86_64-apple-darwin.tar.gz"
        sha256 "ce811f724602ef68b5a52c6b3249dfa086d2986844dfa664ce987668c6887bb9"
      end
    end

    on_linux do
      on_intel do
        url "https://github.com/pandasir/gander/releases/download/v0.6.4/gander-0.6.4-x86_64-unknown-linux-musl.tar.gz"
        sha256 "274144a4b4a779ea376d6eb1c6c487709de21915bd405a3b8ac2419befefc57b"
      end
      on_arm do
        # v0.6.3 was the first release with an arm64 Linux archive, so this is a
        # prebuilt download like the others. Before that the formula compiled
        # from source here, which is the one platform that needed Rust.
        url "https://github.com/pandasir/gander/releases/download/v0.6.4/gander-0.6.4-aarch64-unknown-linux-musl.tar.gz"
        sha256 "473bbd581d4467367f43bfc1da6297c8c58d78006b9f9618f6a49732f2aef39d"
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
