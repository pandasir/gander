# Building a contributor community

The goal is not a star count. It is a record that other people found real
problems, reported them, reviewed a change, and used the release that contained
it. That record is what makes a maintenance claim credible.

## What counts as evidence

An external contribution is credible when:

- the account belongs to a person who is not a project maintainer;
- the report contains a reproduction, not just praise or a feature wish;
- a maintainer responds with a diagnosis, a requested test, or a scope decision;
- the resulting change has a review, a CI run, and a release that names it;
- the contributor can explain the change and respond to follow-up questions.

Do not create multiple accounts, ask friends to post empty issues, or trade
stars. Those signals are easy to detect and they undermine the exact claim the
application is making.

## The first ten contributions

The most reliable path is to make it easy for a user to turn an observation into
a focused report. In order:

1. **Run the release on real machines.** Use Linux, macOS, and FreeBSD at home,
   at work, and in a VM. Record anything that differs from `ps`, `htop`, or the
   documented behavior.
2. **Publish the install paths.** crates.io and the Homebrew tap are already
   live. Put those commands in the README, release notes, and any post you write.
3. **Ask for observations, not stars.** A useful post says: "I built a process
   viewer for macOS/FreeBSD. If you run it, please report one thing that differs
   from htop or `ps` on your machine."
4. **Reply to every report with a decision.** Reproduce, ask for one missing
   fact, document a limitation, or decline with a reason. A closed issue with a
   clear decision is still a good maintenance record.
5. **Turn each accepted report into one small issue.** Do not bundle five
   unrelated fixes into a "cleanup" issue.
6. **Label a few genuinely approachable tasks.** Use `good first issue` only when
   the file, test, and expected behavior are clear enough for an outside
   contributor.
7. **Review the first external PR carefully.** Ask for a regression test, a
   platform note, or a smaller diff. The review is part of the evidence.
8. **Release the fix and link the issue.** The tag message and release body
   should name the user-visible change. Close the issue after the release is
   available.
9. **Credit the contributor.** Use `Co-authored-by` where appropriate, and thank
   them in the release notes if they want to be named.
10. **Repeat with a different user.** One enthusiastic contributor is useful;
    several independent users are a community.

## Where to ask

Choose places where the request is on-topic and the audience can test the claim:

- a Rust community forum or local Rust meetup;
- a macOS, FreeBSD, or Linux operations community;
- a terminal/TUI community that already discusses htop alternatives;
- a package-maintainer community for the platform you just packaged.

Do not post the same text to many channels in one day. Write a short, specific
post with the install command, the supported platforms, and one question you
actually need answered. Link to the issue template rather than asking people to
send private messages.

## Issue labels and milestones

Use the existing labels consistently:

| Label | Meaning |
|---|---|
| `bug` | Reproducible incorrect behavior |
| `enhancement` | A scoped feature request |
| `documentation` | A docs-only correction |
| `good first issue` | Clear, small, with a named file and test |
| `help wanted` | Maintainers have agreed the work is useful |
| `question` | Needs information before it can become a bug |

When an issue is fixed, add the release tag in a comment and close it. When a
release ships several community fixes, list them in the annotated tag message so
the generated changelog carries the evidence forward.

## A realistic target

Five to ten external issues and pull requests over several weeks is a healthy
first milestone. It is more convincing than fifty one-line typo fixes in a day.
Track the list in a private maintainer note or a public milestone, but do not
inflate it with your own accounts.

## Candidate first issues

These are scoped from real gaps in the current tree. Open them as issues only
when a user has reported the problem or when the work is genuinely independent
of a maintainer-only decision. Do not open all of them at once.

### 1. Expose the measured sample interval

The README explains that throughput meters divide by the measured interval, not
the configured delay. The interval is computed inside the sampler, but it is not
part of the snapshot, so batch output cannot report it and a consumer cannot tell
whether a sample was late.

- Files: `crates/core/src/system.rs`, `crates/core/src/lib.rs`,
  `crates/bin/src/batch.rs`
- Test: two samples with a deliberately slow first scan report an interval
  larger than the configured delay
- Difficulty: medium; the sampler must record the interval without changing the
  first-sample zero-rate contract

### 2. Reject a zero limit and zero iterations

`--limit 0` prints an empty array and `-n 0` prints only a header. Both look like
a successful sample of an empty machine. The parser should reject zero with a
message that names the option.

- Files: `crates/bin/src/args.rs`
- Tests: `--limit 0` and `-n 0` return a non-zero exit and an error
- Difficulty: very small; use the existing error style

### 3. Document the JSON schema

The JSON output has stable top-level keys and a nested `memory` object that is
`null` when the read was skipped. That contract exists in code and tests but not
in the README. A consumer currently has to read the serializer.

- Files: `README.md`, `README.zh-CN.md`, `docs/KEYS.md`
- Test: not applicable; add one example with every key and the null case
- Difficulty: small; requires running the binary on Linux and macOS

### 4. Add an integration test for `cargo install gander-bin`

The registry package name and binary name differ. The release process verifies
the local path build but not the crates.io install path. A test or release check
should install into a temporary root and assert the `gander` binary appears.

- Files: `.github/workflows/release.yml`, `packaging/README.md`
- Test: the check must fail if the binary is named `gander-bin`
- Difficulty: medium; needs network access in CI

### 5. Add a platform-report issue form

The bug form asks for a reproduction. A separate form for "gander shows a
different figure from `ps`/`htop` on this platform" would capture the command,
the two outputs, and the kernel version without forcing the reporter to decide
whether it is a bug.

- Files: `.github/ISSUE_TEMPLATE/`, `CONTRIBUTING.md`
- Test: GitHub renders the form and requires the comparison fields
- Difficulty: small; no Rust required

### 6. Make the README test counts reproducible

The two READMEs quote different test totals, and both are stale relative to the
current suite. Replace the hard-coded totals with the command to count them, or
update both files from one release check.

- Files: `README.md`, `README.zh-CN.md`, `docs/RELEASING.md`
- Test: a release check compares the documented command with CI output
- Difficulty: small; documentation and a release-script change
