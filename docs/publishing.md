# Versioned releases and crates.io publishing

Pushing source to GitHub does not publish it to crates.io. A release is an
explicit action that records a version commit, changelog, and immutable Git tag
before publishing the tagged revision.

## Release a version

1. Merge and review the changes on `main`.
2. Open Actions → **Run release** → **Run workflow**, select `main`, and choose
   `patch` for fixes, `minor` for features, or `major` for breaking changes.
3. The workflow runs CI, calculates the next version, and generates release notes
   from Git history. It updates `Cargo.toml`, the `pscon` entry in `Cargo.lock`,
   and `CHANGELOG.md` together.
4. It commits `chore(release): vX.Y.Z`, creates an annotated version tag, and
   pushes the commit and tag atomically. If main changed while CI ran, it stops
   instead of releasing untested changes; rerun it against the latest main.
5. The publisher verifies and tests that tag, publishes to crates.io, checks the
   registry's source revision, and attaches the CLI tarball and SHA-256 checksum
   to its GitHub release.

For example, with the source currently at `0.1.0`, a patch release creates `0.1.1`:

```bash
gh workflow run release.yml --repo vyusec/pscon --ref main -f bump=patch
```

This uses the built-in GitHub token and does not require organization permission
for bots to create pull requests. The publisher is invoked directly: it does not
rely on bot-created tag events triggering another workflow.

## Version and credential checks

CI rejects mismatches between the crate version, lockfile, changelog, and release
tag. The release must be a real tag pointing to a commit on `main`. Existing
versions and tags are never repurposed for new code. The publisher refuses to
skip an existing registry version unless its `.cargo_vcs_info.json` commit
matches the release revision.

The `crates-io` GitHub environment supplies `CARGO_REGISTRY_TOKEN`, authorized to
publish `pscon`. Keep it in environment secrets; it is never committed. The
existing local Cargo credential has been configured there for initial setup.
Rotate it in that environment when needed. Trusted publishing can replace this
credential later without changing version control or release commands.

Workflow permissions are read-only by default. Only the release preparation job
can commit/tag and only the publisher can access the registry environment.
Checks and package verification run for pull requests, main pushes, manual CI,
and again on the tagged source before publication.

## Retry a release

If publication or asset upload fails after the version tag was pushed, use
Actions → **Publish pscon** → **Run workflow** from `main` with that existing tag:

```bash
gh workflow run publish.yml --repo vyusec/pscon --ref main -f tag=v0.1.1
```

A retry uses the tagged revision. Matching release assets are kept; different
assets are rejected instead of overwritten. A yanked version requires a new
version. Run **Run release** again only when you intend to increment the version.

After release, users update with `cargo install pscon --locked`. JSner has its
own independent [release pipeline](https://github.com/vyusec/JSner/blob/main/docs/releases.md).

## References

- [Cargo publishing](https://doc.rust-lang.org/cargo/commands/cargo-publish.html)
- [GitHub workflow dispatch](https://docs.github.com/en/actions/using-workflows/manually-running-a-workflow)
- [crates.io trusted publishing](https://crates.io/docs/trusted-publishing)
