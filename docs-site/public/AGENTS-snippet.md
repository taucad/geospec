## Geometry checks with GeoSpec

- Install `pip install geospec-engine` (CPython 3.13 or 3.14, pytest plugin included) or `npm install geospec-engine` (Node 24 or later).
- Export before checking: STEP for faces, holes, patterns, chamfers, fillets and wall thickness; GLB or STEP for bounds, volume and mass. Declare the unit and a z-up frame.
- One claim per drawing requirement, at the drawing's tolerance (default 0.02 mm).
- Run `pytest` or a script over `geospec-engine/node`; read each status: `passed`, `failed` or `refused`.
- Failed is evidence about the model: fix the geometry at its root and re-export. Never change a tolerance, drop a claim or remove detail to pass.
- Refused is a question asked of the wrong evidence: give it the subject it names, usually STEP, or claim what this subject answers. Report the requirement as unsupported; a refusal is never a pass, even negated.
- Keep the response bytes with the model bytes; the same model, claims and engine version reproduce them.
- Reference: https://geospec.xyz/docs/reference/cheat-sheet.mdx and https://geospec.xyz/llms.txt
