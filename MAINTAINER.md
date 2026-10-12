# Maintainer Guide

## Pull requests

`ci-gate` is the only required check. It asserts every job the event needs
and every job it expects skipped. Shipped engine changes carry a Version Plan.
Byte or timing regressions land with an explicit budget edit. Zero approvals
is the solo-maintainer ruleset; revisit it when a second maintainer joins.

## Release

Version Plans drive releases. After every successful `main` CI run with
pending plans, `release-pr.yml` regenerates the release commit through
`release:prepare --from-plans` as `tau-release-bot`, using the App
credentials held by the `release-pr` environment, and force-updates the
standing pull request on `release/next`; with none pending it closes that pull
request. Review it and squash-merge it with the title unchanged; that is the
entire release act. Do not push to `release/next` or enable auto-merge on it.

Every other push to `main` publishes a `next` prerelease of the engine and
its platform packages (`<next patch>-next.<run number>`), so consumers can pin
an unreleased commit.

Manual fallback when the bot is broken: on a fresh branch off `main`, run
`pnpm release:prepare -- <version> --dry-run` and then the real run, commit
only generated release files as `chore(release): geospec-engine v<version>`,
and open the pull request yourself.

## Registry administration

`names.json` lists the published names: `geospec-engine` (WASM inside) and
one `geospec-engine-<platform>` NAPI package per row, plus the PyPI wheel
`geospec-engine`. Its `publish` switches stay off until the trusted
publishers exist.

Each npm package has one Trusted Publisher: repository `taucad/geospec`,
workflow filename `ci.yml`, environment `npm-publish`. The PyPI project trusts
workflow `ci.yml` with environment `pypi-publish`. npm matches the filename
exactly; a provenance-signed publish that fails with `E404` means the binding
names another workflow or environment. Audit bindings with
`npm trust list <package> --json` and never replace a correct one. A binding
with no publish within two days lapses, so bind only when the publish job is
ready to run. Publishing access requires two-factor authentication and
disallows tokens.

## Repository operations

The `tau-cloud` stack manages repository rules (`ci-gate` required, merge
policy), secret scanning, push protection, private vulnerability reporting,
and the `release-pr`, `npm-publish` and `pypi-publish` environments. Change
them there, never in the repository settings.
