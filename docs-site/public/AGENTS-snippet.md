## GeoSpec geometry checks

- `npm install --save-dev geospec@beta` (Node 24) or `pip install geospec-engine` (CPython 3.13 or 3.14).
- Export z-up: STEP for feature claims, GLB or STEP for bounds and mass; a millimetre GLB declares `sourceUnit: 'mm'` / `source_unit="mm"`.
- Load `loadModel({ source: 'part.step', format: 'step' })` / `load_model(file="part.step", format="step")`; one claim per requirement at the drawing's tolerance (default 0.02 mm).
- Run `geospec run .` over `*.geospec.ts` files (`describe`, `it` and `expectGeo` from `geospec`, `loadModel` from `geospec/model`; exit 0 only when every test passed; a refused claim leaves its test `inconclusive`; `--json` prints the report) or `pytest` over `test_*.py`; each claim is `passed`, `failed` or `refused`.
- Failed means the model is wrong: fix it and re-export; never widen a tolerance, drop a claim or remove detail to pass.
- Refused means the evidence cannot decide: admit the subject the code names or claim what it answers; report it as unsupported, never passed, even negated.
- Keep the canonical result bytes with the model bytes; model, spec and engine version reproduce them.
- Reference: https://www.geospec.xyz/docs/reference/cheat-sheet.mdx and https://www.geospec.xyz/llms.txt
