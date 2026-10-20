#!/usr/bin/env bash
#
# Regenerate `CHANGELOG.md` from the annotated tags.
#
# The tags are the only hand-written record of a release. Everything else — the
# Releases page, and now this file — is derived, so there is no second place for
# the facts to drift. See the header of `build-release-body.sh` for why this
# repository stopped keeping its release notes in more than one place.
#
# Ordering is by version number, not by tag date: `v0.1.0` has no tagger date at
# all (it was re-pointed after the fact) and sorting by date would have put it
# first or last arbitrarily.
#
# Usage: gen-changelog.sh [--write]
#   with no argument, prints to stdout (for a diff)
#   with --write, rewrites CHANGELOG.md

set -euo pipefail
cd "$(dirname "$0")/../.."

write=false
[[ "${1:-}" == "--write" ]] && write=true

repo="${GITHUB_REPOSITORY:-pandasir/gander}"

out=$(
    printf '# Changelog\n\n'
    printf 'Newest first. Every entry is the annotated tag message, verbatim: these notes are\n'
    printf 'not maintained here, they are *derived* from the tag. The same text is what\n'
    printf '`git show v0.6.0` prints and what the GitHub release shows, because all three\n'
    printf 'read it from the same place.\n\n'
    printf 'The reasoning behind each milestone — why a feature is shaped the way it is, and\n'
    printf 'what a live kernel taught the implementation — is in [docs/ROADMAP.md](docs/ROADMAP.md).\n'
    printf 'The per-release notes below say what shipped; ROADMAP says why.\n\n'
    printf -- '---\n\n'

    # `sort -V` gives version order: v0.1.0 < v0.1.1 < v0.2.0 < … < v0.6.0.
    # Reversed for newest-first.
    for tag in $(git tag -l 'v*' | sort -V -r); do
        # v0.1.0 is a lightweight tag: it points at a commit and has no message of
        # its own, so `%(contents)` would return the *commit's* message ("fix:
        # put_str loop counter lint…") and present it as release notes nobody
        # wrote. It is still listed — a gap in a changelog reads as a lost
        # release — but with an honest note instead of borrowed text.
        annotated=true
        if [[ "$(git cat-file -t "$tag")" != "tag" ]]; then
            annotated=false
        fi

        subject=$(git tag -l --format='%(contents:subject)' "$tag")
        body=$(git tag -l --format='%(contents:body)' "$tag")
        date=$(git for-each-ref --format='%(taggerdate:short)' "refs/tags/$tag")
        if [[ -z "$date" ]]; then
            date=$(git log -1 --format='%ad' --date=short "$tag")
        fi

        # Early tags put a conventional-commit summary in the subject instead of
        # the version ("fix: put_str loop counter lint…"). Use it as the heading
        # text rather than printing the version twice.
        heading="$subject"
        case "$heading" in
            "$tag:"*) heading="${heading#"$tag: "}" ;;
        esac

        printf '## %s' "$tag"
        [[ -n "$date" ]] && printf ' — %s' "$date"
        printf '\n\n'

        if ! $annotated; then
            printf '_Tagged without a message, so there are no notes for this one. It was the\n'
            printf 'first release; the notes start at v0.1.1._\n\n'
        elif [[ -n "${body//[[:space:]]/}" ]]; then
            if [[ "$heading" != "$subject" ]]; then
                printf '**%s**\n\n' "$heading"
            fi
            printf '%s\n\n' "$body"
        else
            printf '**%s**\n\n' "$heading"
            printf '_The tag has no message body beyond its subject._\n\n'
        fi

        prev=$(git describe --tags --abbrev=0 "$tag^" 2>/dev/null || true)
        if [[ -n "$prev" ]]; then
            printf '<sub>[compare %s...%s](https://github.com/%s/compare/%s...%s)</sub>\n\n' \
                "$prev" "$tag" "$repo" "$prev" "$tag"
        fi
    done
)

if $write; then
    printf '%s\n' "$out" > CHANGELOG.md
    echo "wrote CHANGELOG.md ($(wc -l < CHANGELOG.md | tr -d ' ') lines)" >&2
else
    printf '%s\n' "$out"
fi
