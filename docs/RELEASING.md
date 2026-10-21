# Releasing

A release is one annotated tag. Everything else — the GitHub release body and
`CHANGELOG.md` — is generated from it, so the notes exist in exactly one place and
cannot disagree with each other.

## Why the tag is the source of truth

Three ways of writing release notes were tried here, and two of them drifted:

| Store | What went wrong |
|---|---|
| A static `body:` block in `release.yml` | Identical for every tag. The Releases page for v0.5.0 said nothing about v0.5.0, and listed `x86_64-unknown-freebsd` as an available archive for a milestone that did not have it. |
| A hand-written `CHANGELOG.md` | Never existed, which is why nobody updated it. A file that must be edited by hand at release time is a file that will be forgotten. |
| The annotated tag message | Written by whoever knows what changed, at the moment they know it, and already read by reviewers through `git log`. |

So the tag message is the only input. `CHANGELOG.md` is *generated*, and the
release body is *built*, both by scripts in `.github/scripts/`.

## The steps

```bash
# 1. Decide the number.
#    minor: a milestone completes (M4 = third backend = 0.6.0)
#    patch: a fix to a released version
#    Never move a tag that has shipped a good release. Moving one that shipped a
#    broken release is correct — see "When a release is wrong" below.

# 2. Bump, and keep Cargo.lock and packaging in step.
$EDITOR Cargo.toml                       # [workspace.package] version
#    Cargo.lock is committed, and the internal workspace dependencies carry both
#    a version and a path so the crates can be published in dependency order.
#    Run `cargo update -w` after the bump, and keep `packaging/` in step.
#    The READMEs link to the latest release rather than a pinned archive, so they
#    do not need editing for a version bump.

# 3. Update docs/ROADMAP.md: mark the milestone released, and set the ship
#    version on the next one.

# 4. Commit.

# 5. Tag with a real message. This IS the release note.
git tag -a v0.6.0
#    First line: "v0.6.0: <headline>" — it becomes the release title.
#    Then the notes: what shipped, why it is shaped that way, what was learned.
#    Look at v0.4.0 or v0.6.0 for the shape and the length that has worked.

# 6. Push both, and wait for CI before calling it done.
git push origin main
git push origin v0.6.0

# 7. Regenerate the changelog and commit it (a separate commit, so the tag does
#    not need to contain a file that summarises itself).
.github/scripts/gen-changelog.sh --write
git add CHANGELOG.md && git commit -m "docs: changelog for v0.6.0"
git push origin main
```

## Check what CI actually produced

A green run is not the same as a correct release. Every one of these has been true
at least once here:

```bash
gh run list --limit 3                       # the Release run, not just CI
gh release view v0.6.0 --json assets --jq '.assets[].name'
```

- **Are all five archives attached?** v0.6.0 first shipped without the FreeBSD one
  because `release` declared `needs: build` and `freebsd` is a sibling job, so
  `download-artifact` could not see it. Nothing was red.
- **Does the body say what changed?** If it opens with "Download the archive", the
  body is still the old static block or the script did not run.
- **Download one and run it.** `shasum -a 256 -c`, then `--version`, then
  `--json --limit 1`. The archive is the product; the workflow passing means the
  build survived, not that the binary works.

## When a release is wrong

If the tag shipped something broken — missing archive, failing CI on the tagged
commit, a version that cannot be installed — delete and re-tag rather than shipping
a patch number over it:

```bash
gh release delete v0.6.0 --yes
git tag -d v0.6.0
git push origin :refs/tags/v0.6.0

# fix on main, wait for CI to go green, then re-tag the same number
```

A version that was never correct was never really released, and leaving a
half-populated v0.6.0 on the Releases page is worse than reusing the number. If the
release *worked* and is merely imperfect, ship the next patch instead; moving a tag
people have already downloaded makes two different binaries share a name.

## Version numbers so far

| Tag | Milestone | Why that number |
|---|---|---|
| v0.1.0, v0.1.1 | M0 skeleton | patch: one configurable value |
| v0.2.0 | M1 + container columns | minor: new features |
| v0.3.0, v0.3.1 | M2 | patch: the macOS memory figure |
| v0.4.0 | M3, four of five items | minor |
| v0.5.0 | M3 complete | minor: delay accounting |
| v0.6.0 | M4, FreeBSD | minor: a whole platform |
| v0.7.0 | M5 (planned) | minor: per-process I/O, `s` |

## Why there is no `version` in the release body

`--version` reads `CARGO_PKG_VERSION`, which comes from `Cargo.toml`. The tag is
`v0.6.0` and the crate is `0.6.0`; a tag with an unbumped manifest therefore
publishes binaries that report the *previous* version under the new version's
filename — an install that looks fine and says the wrong thing.

The `release` job's first step compares the tag against `[workspace.package]
version` and fails the run if they differ, so this cannot ship. It used to say that
step 2 above was "the only defence", which is a worse answer than two lines of
`grep`.
