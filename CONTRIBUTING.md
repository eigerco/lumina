# Contributing to Lumina

First off, thanks for taking the time to contribute! ❤️ We appreciate all the efforts to help both Lumina and the Celestia ecosystem.


Below are a few requirements we have for incoming submissions.

## Signed Commits

All commits need to be signed before PR can be merged, to ensure integrity
and authenticity of the submitted code. If you don't have git signing set up,
see [github's guide](https://docs.github.com/en/authentication/managing-commit-signature-verification/signing-commits)
for reference.

## Conventional Commits

We follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/)
specification when writing commits messages. Consistency helps with readability
of git history, additionally we use it to generate our changelog.

## Semantic versioning

When making a change, please make sure to check if it is SemVer compatible. Pull requests
that contain breaking changes should have an exclamation mark, e.g. `feat(types)!: remove some public method on Blob`.
This will ensure that the version of a crate will be updated accordingly upon the release.

## Adding / upgrading dependencies

We don't enforce the latest dependency versions to give more flexibility in dependency resolution to the dependant crates.
When adding or upgrading dependencies, try setting the minimal required version of a crate, e.g. if current latest version of
`serde-json` is `1.0.256`, start with version `1`. The CI job will then check what is the minimal required version and
suggest e.g. `1.0.101`, which you then can use in the respective `toml` file.

If a dependency is used in multiple crates of the workspace, set it in `workspace.dependencies` section
instead of repeating it in multiple crates.
