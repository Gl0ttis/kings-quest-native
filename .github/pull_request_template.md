## What and why

<!-- One or two sentences. The PR title must be a conventional commit:
     feat(apple2): ..., fix(refemu): ..., docs: ..., ci: ... -->

## Checked against the original

<!-- For engine changes: which reference traces or refemu runs confirm it
     (tests run locally with the game data, since CI has no disks). -->

- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` locally **with** the disks in `ports/<port>/original/`
- [ ] Docs updated (`ports/<port>/docs/`)
