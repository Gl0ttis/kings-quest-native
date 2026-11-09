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
[release-plz](https://release-plz.dev/) (`release-plz.toml`) reads the conventional
commits on `main` since each crate's last tag and keeps a release pull request open with
the next versions and changelogs (`CHANGELOG.md` in each crate). It merges itself once
its checks pass (auto-merge); release-plz then tags the new versions, makes the ports'
GitHub releases, and the release workflow attaches the builds of `kq`. So every merge to
`main` with a `feat`, `fix` or `perf` for a port releases it, within minutes and without
anyone's hand. Below 1.0, a `feat` raises the minor version, anything else the patch.
Other types (`refactor`, `docs`, `test`, `ci`, ...) release nothing by themselves, but a
`refactor` is listed in the next release's changelog.

What a commit releases depends on the files it touches:

- under `ports/<port>/`: that port, except under `ports/<port>/refemu/`, which releases
  nothing. The measuring emulators (`kq1-apple2-refemu`, `kq1-pcjr-refemu`) are tools
  for checking the ports, never released (`release = false`).
- under `crates/<crate>/`: the shared crate (a version, a tag and a changelog, but no
  GitHub release: there is nothing to download), and with it every port that depends on
  it. The port's changelog lists the crate's change.

release-plz works with its own token, the repository secret `RELEASE_PLEASE_TOKEN` (the
name is from release-please, used before): a fine-grained personal access token for
this repository only, with *Contents* and *Pull requests* read and write. GitHub starts
no workflows for what a workflow's own `GITHUB_TOKEN` does, so with that the release
pull request would get no checks and its merge wouldn't release. Renew the token before
it expires (GitHub emails a reminder): make a new one and replace the secret.

## Adding a port

[docs/porting-guide.md](docs/porting-guide.md) describes the whole process. In the
repository:

1. Create `ports/<platform>/` with its crate (`kq1-<platform>`) and add it to the
   workspace `members` in the root `Cargo.toml`.
2. Add it, and its refemu with `release = false`, to `release-plz.toml`, and its scope
   to `.commitlintrc.json` and `.github/workflows/pr-title.yml`.
3. Add it to the ports table in the root README.
