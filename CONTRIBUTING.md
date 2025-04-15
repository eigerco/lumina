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

## Upgrading dependencies (if you need to)

Some of our users use `celestia-types` with [risc0](https://github.com/risc0)
zkVM, which offers [precompiles](https://dev.risczero.com/api/zkvm/precompiles)
for some of the cryptographic and algebraic functions in the form of patched crates.
This requires using one of the specifically supported versions of those crates.

To support that we've created `./tools/upgrade-deps.sh` script which upgrades all
dependencies in `Cargo.toml` except the ones that are patched by risc0. If you want
to bump the depednecies, please use it.

How to upgrade:
```bash
./tools/upgrade-deps.sh -i  # `-i` upgrades incompatible versions too
cargo update
```
