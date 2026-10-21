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
  [:macos, :arm, "aarch64-apple-darwin"],
  [:macos, :intel, "x86_64-apple-darwin"],
  [:linux, :intel, "x86_64-unknown-linux-musl"],
  [:linux, :arm, nil],
]

platforms.each do |os, arch, target|
  Homebrew::SimulateSystem.with(os: os, arch: arch) do
    specs.each do |spec|
      Formulary.clear_cache
      formula = Formulary.from_contents("gander", path, contents, spec, tap:)
      source_build = spec == :head || target.nil?
      expected_deps = source_build ? ["rust"] : []
      actual_deps = formula.deps.map(&:name).sort
      label = "#{os}/#{arch}/#{spec}"
      raise "#{label}: expected #{expected_deps}, got #{actual_deps}" if actual_deps != expected_deps

      if spec == :head
        raise "#{label}: expected the main Git repository" if formula.head.url != "https://github.com/pandasir/gander.git"
      else
        version = formula.stable.version
        base = "https://github.com/pandasir/gander"
        expected_url = if target
          "#{base}/releases/download/v#{version}/gander-#{version}-#{target}.tar.gz"
        else
          "#{base}/archive/refs/tags/v#{version}.tar.gz"
        end
        raise "#{label}: wrong download URL #{formula.stable.url}" if formula.stable.url != expected_url
        raise "#{label}: missing SHA-256" unless formula.stable.checksum.to_s.match?(/\A[0-9a-f]{64}\z/)
      end
      puts "#{label}: #{source_build ? "source build with Rust" : "prebuilt binary without dependencies"} OK"
    end
  end
end
