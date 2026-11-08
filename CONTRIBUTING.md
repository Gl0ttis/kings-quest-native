# Contributing

## Game data

The repository holds no game content. To build and run everything, and to run the
tests that compare a port with the original, put your own disk images in the port's
`original/` folder (for the Apple IIe: `ports/apple2/original/`, see its README).
Everything decoded from them goes to `ports/<port>/extracted/`, which git ignores
entirely. The traces the tests compare against are in `ports/<port>/reference/`:
numbers measured from the original (rooms, positions, variable hashes, cycle
counts), with no game content in them.

Never commit disk images, ROM images, decoded resources, script listings,
disassemblies or saves. `.gitignore` covers the usual places and file types; check
`git status` before committing anyway.

## Checks

Every pull request runs (`.github/workflows/ci.yml`) `cargo fmt --all --check`, and on
Windows and Linux:

```
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --all-targets --locked
cargo test --workspace --locked
cargo test -p kq1-apple2 --no-default-features --lib --locked
```

CI has no game data, so the tests that need it skip themselves there. Run
`cargo test --workspace` locally with the game data in place (for the Apple IIe:
`extracted/disk1.do` to `disk3.do`, see its README) before opening a pull request
that touches an engine.

## Commits and pull requests

Commits and pull request titles follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <summary>
```

| Type | For | Version bump (before 1.0 / after) |
|---|---|---|
| `feat` | new behaviour | minor / minor |
| `fix` | a bug fix, including a difference from the original | patch / patch |
| `perf`, `refactor` | no behaviour change | patch / patch |
| `docs`, `test`, `build`, `ci`, `chore` | no release on their own | none |

A `!` after the type, or a `BREAKING CHANGE:` footer, marks a breaking change
(minor before 1.0, major after).

Scopes: `apple2` (the Apple IIe port), `refemu` (its reference emulator),
`workspace`, `ci`, `deps`, `docs`, `release`. New ports add their own (for example
`pcjr`). The scope is optional.

Examples:

```
feat(apple2): draw room pictures progressively, as the original plots them
fix(apple2): restore enters the room with carry set ($48DF)
feat(refemu): emulate Disk II writes for save-disk tests
docs(apple2): record the start-up timings
```

Pull requests are squash-merged, so the title becomes the commit on `main`; a
check enforces its format. Commit messages within a pull request are checked too.

## Versions and releases

Each crate has its own [semantic version](https://semver.org/), in its `Cargo.toml`,
and its own tags (`kq1-apple2-v0.2.0`). Nothing is bumped by hand:
[release-please](https://github.com/googleapis/release-please) reads the
conventional commits on `main` and keeps a release pull request open with the next
versions and changelogs (`ports/<port>/CHANGELOG.md`). Its checks are started by the
release workflow (GitHub doesn't start them for a pull request a workflow opened).
Merging it tags the release, and the release workflow attaches the builds of `kq`.
`kq1-apple2-refemu` gets versions and a changelog but no GitHub release: it is
a measuring tool, not something to download.

A change under `ports/apple2/refemu/` releases `kq1-apple2-refemu` only; anything
else under `ports/apple2/` releases `kq1-apple2`, and with it `kq1-apple2-refemu`,
which depends on it (release-please's `cargo-workspace` plugin bumps dependents).

## Adding a port

1. Create `ports/<platform>/` with its crate (`kq1-<platform>`) and add it to the
   workspace `members` in the root `Cargo.toml`.
2. Add it to `release-please-config.json` and `.release-please-manifest.json`, and
   its scope to `.commitlintrc.json` and `.github/workflows/pr-title.yml`.
3. Add it to the ports table in the root README.
