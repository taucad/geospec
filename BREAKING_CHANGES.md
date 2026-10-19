# Breaking changes

A `geospec-engine` release that changes a public export, the result bytes for
the same model, claims and engine inputs, or drops a supported host adds an
entry here in the same pull request: the version, what changed and how to
migrate. Entries are listed with the latest release first.

## Next minor release

`GEOSPEC_EVIDENCE_UNSUPPORTED` is gone: every refusal that used it now uses
`GEOSPEC_UNSUPPORTED_EVIDENCE`, the code the engine already used for the same
reason. Code that matches either spelling keeps working; code that matches
only `GEOSPEC_EVIDENCE_UNSUPPORTED` should match `GEOSPEC_UNSUPPORTED_EVIDENCE`.
To tell an out-of-domain refusal apart, read `details.applicability.condition`.

`inspectGeometry`, `analyzeMeshOverlap` and `analyzeBrep` answer `refused`
instead of `failed` when the evidence they read is missing or cannot decide: a
selector that is unmatched, ambiguous or unsupported on the subject, an
inconclusive component partition, an invalid component, a subject with no BRep
or a BRep the backend cannot measure. The diagnostics and their codes are
unchanged; a refused result carries no `evidence`, so the former
`{ success: false }` evidence and the empty `inspectGeometry` match lists are
gone. Every query now answers `passed` or `refused`, as `minimumDistance` and
`queryPmi` already did. Code that branches on `failed` for these queries should
branch on `refused` and read the first diagnostic's code.
