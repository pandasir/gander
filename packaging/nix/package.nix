# Draft for a nixpkgs pkgs/by-name/ga/gander/package.nix PR. The cargoHash is
# a placeholder: this machine has no nix, so the real hash must be filled in
# per packaging/nix/README.md before the PR (ofborg will fail a fakeHash).
{
  lib,
  rustPlatform,
  fetchFromGitHub,
}:

rustPlatform.buildRustPackage rec {
  pname = "gander";
  version = "0.6.7";

  src = fetchFromGitHub {
    owner = "pandasir";
    repo = "gander";
    tag = "v${version}";
    hash = ""; # fill with the fetchFromGitHub hash from the build error
  };

  # crates/bin is the binary crate; the workspace root alone would try to
  # build every member.
  cargoRoot = "crates/bin";
  # The lockfile lives at the workspace root, not in cargoRoot.
  cargoLock = {
    lockFile = "${src}/Cargo.lock";
  };

  meta = {
    description = "Cross-platform interactive process viewer that focuses on one process";
    longDescription = ''
      gander is an interactive process viewer for the terminal, like htop,
      for Linux, macOS and FreeBSD. It keeps htop's keybindings and puts a
      live inspector beside the process table that follows the selected
      process: command line, state, memory, uptime. Pressing L flips back to
      the classic full-width htop layout, and a headless mode (--json, --sort,
      --filter, --limit) runs without a TUI for scripts.
    '';
    homepage = "https://pandasir.github.io/gander";
    changelog = "https://github.com/pandasir/gander/blob/main/CHANGELOG.md";
    license = lib.licenses.gpl2Plus;
    mainProgram = "gander";
    maintainers = with lib.maintainers; [ ]; # add your nixpkgs maintainer handle
    platforms = with lib.platforms; linux ++ darwin ++ freebsd;
  };
}
