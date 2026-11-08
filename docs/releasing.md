# Releasing Parzi

## Every change

Add a line to `CHANGELOG.md` under **Unreleased** for anything people would
notice, in plain words. That list is what the release page shows.

## A dev build

**Actions, Release, Run workflow**, leave *What to release* on **dev**, run.
Or ask Claude to cut one. It works out the next version (1.4.1-dev.2 becomes
1.4.1-dev.3, and after 1.4.1 comes 1.4.2-dev.1), writes it into every file,
commits, tags, builds the Windows and Mac apps, the command line and the Linux
server builds, and publishes a pre-release whose notes are the Unreleased list.

People get dev builds only when they turn on **Settings, General, Dev builds**.
Everyone else stays on full releases.

## A full release

The same, with *What to release* on **patch**, **minor** or **major**. A dev
version becomes its release (1.4.1-dev.3 becomes 1.4.1). The Unreleased notes
move under the new version in `CHANGELOG.md`, and the in-app updater offers it
to everyone.

Or type an exact version, such as `1.5.0-dev.1`, to start a new line.

## By hand

```bash
node scripts/release.mjs cut dev      # or patch, minor, major, or 1.5.0
git commit -am "release $(node scripts/release.mjs version)"
git tag "v$(node scripts/release.mjs version)" && git push --follow-tags
```

A pushed tag builds the same way, as long as it matches the version in the
files. `node scripts/release.mjs` lists the other commands.

## What runs where

| Workflow | When | What |
| --- | --- | --- |
| `ci.yml` | Every push, and pull requests from forks | Formatting, one version everywhere, clippy and tests on Linux, Windows and macOS, the UI checks |
| `release.yml` | Run workflow, or a `v*` tag | Bumps (when run by hand), builds and publishes; points the dev channel at the new build |
| `cache.yml` | Dependency changes on main, and weekly | Keeps the release build cache warm so releases build in minutes |
| `pages.yml` | Changes to `site/` on main | The website |

The toolchain is pinned once in `rust-toolchain.toml`, and the lint rules once
in each `Cargo.toml`, so `cargo clippy` and `cargo fmt` behave the same on your
machine as in CI. `.github/actions/setup` installs Rust, Node and the caches
for every workflow.

## The dev channel

Each release also uploads its `latest.json` to a rolling pre-release called
`updater-dev`. The Dev builds switch makes Parzi check that file. It is a
pre-release, so the stable channel (`releases/latest`) never sees it.
