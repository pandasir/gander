## Summary

<!-- What problem does this change solve? -->

## Linked issue

<!-- Use "Fixes #123" when the pull request should close the issue. -->

## Behavior

<!-- What changed for the user, and what stayed the same? -->

## Testing

<!-- Paste the commands you ran and their results. -->

```text
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
```

Format the files you changed. A whole-workspace `cargo fmt --check` currently
reports pre-existing differences, so do not include unrelated reformatting.

Platforms tested:

- [ ] Linux
- [ ] macOS
- [ ] FreeBSD
- [ ] Not platform-specific

## Checklist

- [ ] The change is focused on one problem.
- [ ] A bug fix includes a regression test.
- [ ] A platform-specific change explains the kernel or API behavior involved.
- [ ] I updated documentation or comments where behavior changed.
- [ ] I did not edit `CHANGELOG.md`; it is generated from release tags.
- [ ] I removed secrets and unrelated personal data from logs and fixtures.
