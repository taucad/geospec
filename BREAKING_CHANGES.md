# Breaking changes

A `geospec-engine` release that changes a public export, the result bytes for
the same model, claims and engine inputs, or drops a supported host adds an
entry here in the same pull request: the version, what changed and how to
migrate. Entries are listed with the latest release first.

## Next minor release

A claim whose payload has an unknown or malformed field refuses alone with
`GEOSPEC_INVALID_EXPECTATION`, and the plan's other claims still run, instead of
the engine rejecting the whole plan with `ProtocolError` `invalid-claim`. The
diagnostic keeps the former message, and `details` carry `field` and, when that
key occurs once in the payload, its `path`. `expectGeo` and `expect_geo` raise
the refusal as `GeoSpecAssertionError`. Code that catches `invalid-claim` for
a malformed payload should read the refused claim's first diagnostic instead.
A negated query, an unadmitted subject, an out-of-range budget and other
plan-level errors still throw `invalid-claim`.

`toHaveNoComponentInterference` on a STEP subject answers `failed` with
`GEOSPEC_COMPONENT_INTERFERENCE`, instead of `refused` with
`GEOSPEC_UNSUPPORTED_EVIDENCE`, for a pair with no allowance whose exact
regular-solid Common is a solid: two overlapping parts, or a part embedded in
another. The pair's evidence row has the criterion
`exact-regular-solid-common` with the Common's volume and bounds, and
`measured` adds `interferingPairs` and `undecidedPairs`. One interfering pair
decides the claim even when another pair is undecided. Touching pairs still
refuse. Code that treated this refusal as "not checked" should read the failed
claim's diagnostic instead.

`toHaveMinimumWallThickness` answers `passed` or `failed`, instead of
`refused` with `GEOSPEC_UNSUPPORTED_EVIDENCE`, for a STEP subject that is one
valid axis-aligned box with one closed axis-aligned box cavity. `measured`
then has the domain `axis-aligned-box-with-box-cavity` and
`classificationSlackMm` in place of `maximumTopologyToleranceMm`, and the
witnesses' `materialInterval.method` is `validated-box-with-box-cavity`. Every
other subject outside the box and cylinder domains still refuses.

## 0.3.0

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
