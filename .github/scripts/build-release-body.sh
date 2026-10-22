#!/usr/bin/env bash
#
# Build the release body from the annotated tag the release is for.
#
# Why this exists: the body used to be a static block in `release.yml`, identical
# for every tag, so the Releases page said nothing about what changed — while the
# tag message, which said it well, was visible only to `git`. Two copies of the
# release notes, with one of them always wrong.
#
# There is no hand-maintained changelog. A changelog written by hand is a third
# copy of facts that already live in the tag and in `docs/ROADMAP.md`, and this
# repository's two documented failures are both "a restatement drifted": the
# `Field` example in DESIGN.md §4.1 and the workspace tree in §2. The tag message
# is written at release time by the person who knows what changed, cannot be
# forgotten (an annotated tag has a message or it is not one), and is reviewed by
# whoever reads `git log`. `CHANGELOG.md` is generated from these tags, not edited.
#
# Usage: build-release-body.sh <tag>    e.g. v0.6.0
# Body goes to stdout. Exits non-zero rather than publishing empty notes.

set -euo pipefail

tag="${1:-}"
if [[ -z "$tag" ]]; then
    echo "usage: $0 <tag>" >&2
    exit 2
fi

repo="${GITHUB_REPOSITORY:-pandasir/gander}"

if ! git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
    echo "error: no such tag: $tag" >&2
    exit 1
fi

# A lightweight tag points at a commit and carries no message of its own, so
# `%(contents)` would hand back the *commit's* message and the release would ship
# a changelog entry that nobody wrote as one. Refuse instead; the fix is to tag
# with `-a`, which is what the script in docs/RELEASING.md does.
#
# The second clause is what made this error misleading in CI: `actions/checkout`
# fetches a tag ref by commit SHA, so the tag object is absent and a perfectly good
# annotated tag looks lightweight. Nothing local can distinguish that from a real
# lightweight tag, so the message says both possibilities and names the workflow
# step that fixes the checkout case.
if [[ "$(git cat-file -t "$tag")" != "tag" ]]; then
    {
        echo "error: $tag does not resolve to a tag object here"
        echo "  either it is a lightweight tag (use: git tag -a $tag)"
        echo "  or the tag object was never fetched (use: git fetch --force origin "
        echo "    +refs/tags/$tag:refs/tags/$tag); actions/checkout does not do this"
    } >&2
    exit 1
fi

subject=$(git tag -l --format='%(contents:subject)' "$tag")
notes=$(git tag -l --format='%(contents:body)' "$tag")

if [[ -z "${notes//[[:space:]]/}" ]]; then
    echo "error: $tag has no tag message body; the release would ship without notes" >&2
    exit 1
fi

# The subject already reads "v0.6.0: M4 complete — …", so it becomes the title
# rather than being repeated as the body's first line.
#
# Both separators have been used in this repository's tags: `v0.6.0:` with a
# colon and `v0.6.3 — …` with an em dash. Stripping only the colon produced
# "v0.6.3 — v0.6.3 — installable with…", so both forms are handled.
title="$subject"
case "$title" in
    "$tag:"*) title="${title#"$tag: "}" ;;
    "$tag — "*) title="${title#"$tag — "}" ;;
    "$tag - "*) title="${title#"$tag - "}" ;;
    *) title="$tag — $title" ;;
esac

prev=$(git describe --tags --abbrev=0 "$tag^" 2>/dev/null || true)

# Headline first: the old body opened with "download the archive", the same
# sentence for every release, which carried no information at the one place a
# reader looks to find out what a version is.
printf '## %s\n\n' "$title"
printf '%s\n\n' "$notes"

printf -- '---\n\n'
printf '## Install\n\n'
printf '```bash\n'
printf 'tar -xzf gander-%s-<platform>.tar.gz\n' "${tag#v}"
printf './gander-%s-*/gander\n' "${tag#v}"
printf '```\n\n'
printf '| Archive | Platform |\n'
printf '|---|---|\n'
printf '| `aarch64-apple-darwin` | macOS, Apple Silicon |\n'
printf '| `x86_64-apple-darwin` | macOS, Intel |\n'
printf '| `x86_64-unknown-linux-gnu` | Linux x86_64 (glibc) |\n'
printf '| `x86_64-unknown-linux-musl` | Linux x86_64 (static, any distro) |\n'
printf '| `aarch64-unknown-linux-gnu` | Linux arm64 (glibc) |\n'
printf '| `aarch64-unknown-linux-musl` | Linux arm64 (static, any distro) |\n'
printf '| `x86_64-unknown-freebsd` | FreeBSD 14 and later |\n'
printf '\n'
printf 'Every archive has a `.sha256` beside it: `shasum -a 256 -c <file>.sha256`.\n\n'

# The repository is the install path most Linux users take, so it belongs in the
# release body rather than only in the README. It is written here from the tag
# rather than pinned to a version: `apt upgrade` follows the suite.
printf '### Debian, Ubuntu\n\n'
printf '```bash\n'
printf 'curl -fsSL https://pandasir.github.io/gander/gander.gpg \\\n'
printf '  | sudo gpg --dearmor -o /usr/share/keyrings/gander.gpg\n'
printf 'echo "deb [signed-by=/usr/share/keyrings/gander.gpg] https://pandasir.github.io/gander stable main" \\\n'
printf '  | sudo tee /etc/apt/sources.list.d/gander.list\n'
printf 'sudo apt update && sudo apt install gander\n'
printf '```\n\n'
printf '### Fedora, RHEL, Rocky\n\n'
printf '```bash\n'
printf 'sudo rpm --import https://pandasir.github.io/gander/RPM-GPG-KEY-gander\n'
printf 'sudo dnf config-manager --add-repo https://pandasir.github.io/gander/gander.repo\n'
printf 'sudo dnf install gander\n'
printf '```\n\n'
printf 'The `.deb` and `.rpm` files are also attached below for a one-off\n'
printf '`apt install ./file.deb`; `SHA256SUMS` covers all of them.\n\n'
printf 'From source: `cargo install --path crates/bin` (Rust 1.89+).\n\n'

if [[ -n "$prev" ]]; then
    printf 'Full diff and commit list: [%s...%s](https://github.com/%s/compare/%s...%s)\n\n' \
        "$prev" "$tag" "$repo" "$prev" "$tag"
else
    printf 'Full history: [commits](https://github.com/%s/commits/%s)\n\n' "$repo" "$tag"
fi

printf 'Every release is explained at length — the reasoning, the alternatives, and what a\n'
printf 'live kernel or VM taught the implementation — in\n'
printf '[docs/ROADMAP.md](https://github.com/%s/blob/%s/docs/ROADMAP.md). The notes above say\n' "$repo" "$tag"
printf 'what shipped; ROADMAP says why.\n'
