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
