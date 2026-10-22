# typed: strict
# frozen_string_literal: true

# Run with: brew ruby packaging/homebrew/test_formula.rb
require "formula"
require "formulary"
require "simulate_system"

path = Pathname.new(__dir__)/"Formula/gander.rb"
contents = path.read
# An explicit tap keeps this local check independent of the Homebrew core API.
tap = Tap.fetch("pandasir/tap")
specs = [:stable, :head]
platforms = [
  [:macos, :arm, "aarch64-apple-darwin", false],
  [:macos, :intel, "x86_64-apple-darwin", false],
  [:linux, :intel, "x86_64-unknown-linux-musl", false],
  [:linux, :arm, "aarch64-unknown-linux-musl", false],
]

platforms.each do |os, arch, target, source_platform|
  Homebrew::SimulateSystem.with(os: os, arch: arch) do
    specs.each do |spec|
      Formulary.clear_cache
      formula = Formulary.from_contents("gander", path, contents, spec, tap:)
      # A source build needs Rust; a prebuilt archive must have no dependencies at
      # all. That asymmetry is the regression this matrix exists to catch.
      source_build = spec == :head || source_platform
      expected_deps = source_build ? ["rust"] : []
      actual_deps = formula.deps.map(&:name).sort
      label = "#{os}/#{arch}/#{spec}"
      raise "#{label}: expected #{expected_deps}, got #{actual_deps}" if actual_deps != expected_deps

      if spec == :head
        raise "#{label}: expected the main Git repository" if formula.head.url != "https://github.com/pandasir/gander.git"
      else
        version = formula.stable.version
        base = "https://github.com/pandasir/gander"
        expected_url = if source_platform
          "#{base}/archive/refs/tags/v#{version}.tar.gz"
        else
          "#{base}/releases/download/v#{version}/gander-#{version}-#{target}.tar.gz"
        end
        raise "#{label}: wrong download URL #{formula.stable.url}" if formula.stable.url != expected_url
        raise "#{label}: missing SHA-256" unless formula.stable.checksum.to_s.match?(/\A[0-9a-f]{64}\z/)
        # A placeholder hash means the release has not been published yet. Say so
        # rather than printing OK for a formula nobody could install.
        if formula.stable.checksum.to_s.match?(/\A0{64}\z/)
          raise "#{label}: SHA-256 is still the placeholder; fill it from the published archive"
        end
      end
      puts "#{label}: #{source_build ? "source build with Rust" : "prebuilt binary without dependencies"} OK"
    end
  end
end
