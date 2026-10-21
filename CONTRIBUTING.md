# Contributing to gander

Thanks for helping improve gander. The project is a cross-platform process
viewer with three live backends, so a useful contribution is usually more than a
patch: it is a reproduction, a platform observation, or a test that keeps a
kernel-specific assumption honest.

## Before you start

- Search existing issues and pull requests before opening a new one.
- For a bug, include the exact gander version, platform, install method, and
  reproduction steps.
- For a larger feature or a refactor, open an issue first so the design can be
  discussed before the code is written.
- Security problems must not be reported in a public issue. Follow
  [SECURITY.md](SECURITY.md).
- AI-assisted contributions are welcome. You are responsible for every line you
  submit, must understand it, and must be able to respond to review. Do not
  submit bulk, low-effort, or unreviewed generated changes.

## Development setup

gander requires Rust 1.89 or newer.

```bash
git clone https://github.com/pandasir/gander
cd gander
cargo build --locked
cargo run --locked -p gander-bin
```

The workspace is split into four crates:

| Crate | Responsibility |
|---|---|
| `gander-core` | Platform sampling, parsing, process model, actions |
| `gander-config` | TOML settings, history, and `htoprc` import |
| `gander-tui` | ratatui rendering, layouts, input, and runtime |
| `gander-bin` | Argument parsing, batch output, and the executable |

Keep `gander-core` free of UI dependencies. The `--json` and batch modes depend
on that boundary.

## Running the checks

Run these before opening a pull request:

```bash
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
```

Formatting is checked in CI on the files a pull request touches. The existing
tree is not fully `rustfmt`-clean, so a whole-workspace `cargo fmt --all --check`
will report pre-existing diffs. Do not reformat unrelated files to make that
command pass; format the code you changed and keep the diff reviewable.

For a faster edit loop, test the crate you changed first:

```bash
cargo test --locked -p gander-core
cargo test --locked -p gander-config
cargo test --locked -p gander-tui
```

The live macOS tests read the system process table. If your terminal or CI
sandbox denies that access, run the suite outside the restricted sandbox; do not
weaken the test to make a restricted environment pass.

FreeBSD has no GitHub-hosted runner. CI compiles the FreeBSD target and runs the
full suite in a FreeBSD 14.5 VM. Say in your pull request whether you tested on a
real FreeBSD machine, and include the version.

## Tests and fixtures

- Put parser and accounting tests next to the code they cover.
- Use fixtures under `crates/core/tests/fixtures/` when the input is a stable
  kernel file.
- Use a live test only when the contract depends on the running kernel.
- A live test should compare against an independent source such as `ps(1)`,
  `sysctl(8)`, `free(1)`, or `netstat(1)` where possible.
- Assert the contract, not the development machine. Do not hard-code a terminal
  name, a process count, or a page-cache-dependent I/O value.

## Platform changes

Platform differences are selected with `cfg`, not runtime flags. A new
`PlatformSampler` method must be implemented by Linux, macOS, and FreeBSD, or
explain why the shared trait should not grow.

When changing `/proc`, `sysctl`, Mach, or `libkvm` behavior, include the kernel
version and the output that proves the old assumption was wrong. The
[portability notes](docs/PORTABILITY.md) record the traps that have already cost
this project time.

## Pull requests

A good pull request:

- solves one problem;
- links the issue it fixes, if there is one;
- explains the user-visible behavior before and after;
- includes a regression test for a bug;
- lists the platforms tested;
- keeps unrelated formatting and refactoring out of the diff.

Use a conventional, lowercase commit subject where practical, for example:

```text
fix(linux): read smaps_rollup before falling back to smaps
docs: explain the macOS affinity limitation
```

Maintainers review for correctness first, then portability, then style. Review
comments may ask for a smaller change or a test that fails before the fix.

Do not edit `CHANGELOG.md`. It is generated from annotated release tags; see
[docs/RELEASING.md](docs/RELEASING.md).

## Issue and review lifecycle

Maintainers aim to triage new reports with one of these outcomes:

| Outcome | What happens next |
|---|---|
| Reproduced | Add a failing test, fix it, and link the issue from the release tag |
| Needs information | Ask for a missing version, platform, or command |
| Confirmed limitation | Document why the platform cannot provide the data |
| Not planned | Close with a concrete reason, not a bare `wontfix` |

When a fix ships, the issue is closed only after the release containing it is
available. The release tag records the user-visible change, which is what makes
the review and release history auditable later.

## Licensing

gander is GPL-2.0-or-later. By contributing, you agree that your contribution
may be distributed under that license. Do not copy code from a project whose
license is incompatible with GPL-2.0-or-later.
