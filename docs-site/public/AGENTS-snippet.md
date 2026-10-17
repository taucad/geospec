## GeoSpec geometry checks

- `npm install geospec@beta geospec-engine` (Node 24) or `pip install geospec-engine` (CPython 3.13 or 3.14).
- Export z-up: STEP for feature claims, GLB or STEP for bounds and mass; a millimetre GLB declares `sourceUnit: 'mm'` / `source_unit="mm"`.
- Load `loadModel({ source: 'part.step', format: 'step' })` / `load_model(file="part.step", format="step")`; one claim per requirement at the drawing's tolerance (default 0.02 mm).
- Run the spec with Node 24 (a failed or refused claim throws `GeoSpecAssertionError`) or with `pytest`; each claim is `passed`, `failed` or `refused`.
- Failed means the model is wrong: fix it and re-export; never widen a tolerance, drop a claim or remove detail to pass.
- Refused means the evidence cannot decide: admit the subject the code names or claim what it answers; report it as unsupported, never passed, even negated.
- Keep the canonical result bytes with the model bytes; model, spec and engine version reproduce them.
- Reference: https://geospec.xyz/docs/reference/cheat-sheet.mdx and https://geospec.xyz/llms.txt
