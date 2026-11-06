# geospec public API index

geospec 0.6.0 · 875 symbols · extracted by TypeScript 5.9.3.

Every symbol appears here exactly once. The heading above each block names the file with its signature, and each top-level symbol names the lines of that file which hold its declaration and members: read only that range.

## Functions — `public-api-functions.md`

describe (function) [lines 5-10] [1 members] — GeoSpec suite helper used inside VM-executed test modules
  describe.skip (method)
expectGeo (function) [lines 12-16] — Start a geometry assertion chain
it (function) [lines 18-23] [1 members] — GeoSpec test helper used inside VM-executed test modules
  it.skip (method)
test (function) [lines 25-30] [1 members] — Alias for {@link it}
  test.skip (method)
normalizeGeoSpecExpected (function) [lines 32-36] — Derive the `expected` value an assertion records from the call…
assertGeoSpecJsonValue (function) [lines 38-40] — Reject a non-wire value rather than letting JSON.stringify erase it
decodeGeoSpecCanonicalJson (function) [lines 42-44] — Parse and validate canonical bytes without re-canonicalizing them
encodeGeoSpecCanonicalJson (function) [lines 46-48] — Encode client-owned canonical claim bytes
isGeoSpecJsonValue (function) [lines 50-52] — Runtime JSON-value guard used at every protocol trust boundary
toGeoSpecProtocolJson (function) [lines 54-56] — Convert an authoring value to protocol JSON, including the two…
createModelLoader (function) [lines 58-61] — Create a {@link loadModel} function over a host-supplied compiled engine
loadModel (function) [lines 63-68] — Load a CAD model into GeoSpec evidence
commandExporter (function) [lines 70-74] — Create a model exporter that runs a command speaking GeoSpec…
clearCollectorGlobals (function) [lines 76-77]
createCollector (function) [lines 79-82] — Create the collector used by the embedded GeoSpec runner
installCollector (function) [lines 84-85]
compileGeoSpecTestNamePattern (function) [lines 87-97] — Compile a Vitest-style test-name pattern once for a GeoSpec run
filterGeoSpecTests (function) [lines 99-103] — Filter collected GeoSpec tests by compiled test-name pattern
matchesGeoSpecTestName (function) [lines 105-109] — Return true when a collected GeoSpec test matches the supplied…
discoverGeoSpecFiles (function) [lines 111-115] — Discover GeoSpec test files from exact files or directory roots
isGeoSpecTestFile (function) [lines 117-120] — Return true when a project-relative path names a GeoSpec test…
runGeoSpecModule (function) [lines 122-125] — Execute an ESM GeoSpec module using the shared Tau VM…
isGeoSpecTimeoutCode (function) [lines 127-130] — Whether a diagnostic or issue code belongs to the GeoSpec…
createPlainModuleVm (function) [lines 132-135] — Create GeoSpec's default module VM
createNativeGeoSpecRunner (function) [lines 137-140] — Compose compiled assertion and model bindings with the SDK's serial…
allocateNativePoolGrants (function) [lines 142-149] — Assign a bounded CPU budget across already-selected worker isolates
createGeoSpecNativeModelLoader (function) [lines 151-154] — Create a native model loader that admits actual STEP/GLB bytes…
createNodeVmFileSystem (function) [lines 156-159] — Create a Node `VmFileSystem` rooted at `root`
resolveUnderRoot (function) [lines 161-166] — Resolve one VM path to a real path under the…
createNodeGeoSpecEngine (function) [lines 168-172] — Create a GeoSpec engine for this process
defaultGeoSpecFactsStoreRoot (function) [lines 174-177] — The default store root, `$XDG_CACHE_HOME/geospec/<engine version>`, with `~/.cache` when `XDG_CACHE_HOME`…
resolveGeoSpecFactsStore (function) [lines 179-187] — The store a run over `projectRoot` opens, from `GEOSPEC_FACTS_STORE`
createGeoSpecNodePoolRunner (function) [lines 189-194] — Create a pool with one engine and one set of…
createNodeChildProcessHandle (function) [lines 196-201] — Adapt a forked pool child process to the pool's host-agnostic…
createNodeWorkerHandle (function) [lines 203-206] — Adapt a Node worker thread to the pool's host-agnostic handle
createNoMatchingGeoSpecTestsIssue (function) [lines 208-210] — Create a run-level issue when filters select no tests
createGeoSpecPoolRunner (function) [lines 212-215] — Create the host-agnostic pool runner
startGeoSpecPoolWorkerHost (function) [lines 217-220] — Serve shards until the host says shutdown
createGeoSpecAssertionClient (function) [lines 222-225] — Create a standalone native GeoSpec assertion client
createGeoSpecMatcherMethods (function) [lines 227-234] — Create the one registry-derived matcher surface used by every JavaScript…
evaluateGeoSpecNativeQuery (function) [lines 236-240] — Submit an ancillary query with positive polarity through the native…
resolveGeoSpecMaximumWorkUnitLimit (function) [lines 242-246] — Resolve a host's largest authored work-unit budget
runGeoSpecCli (function) [lines 248-252] — Run the CLI
createNodeGeoSpecCliHost (function) [lines 254-257] — Build the Node CLI host
createGeoSpecVitestAdapter (function) [lines 259-262] — Create Vitest matchers over an existing runner-independent client
installGeoSpecVitest (function) [lines 264-267] — Register GeoSpec matchers and their settlement hook in the active…
setupGeoSpecVitest (function) [lines 269-272] — Create a native client and register it with the active…
setupGeoSpecVitestProject (function) [lines 274-287] — Configure GeoSpec for the active Vitest file from the project's…
defineConfig (function) [lines 289-293] — Type a `geospec.config.*` default export
readGeoSpecConfig (function) [lines 295-299] — Read a project's `geospec.config.*` data fields through the spec VM,…
loadGeoSpecConfig (function) [lines 301-305] — Load one trusted project config using Node's native module loader

## Constants — `public-api-constants.md`

geoSpecMatcherNames (constant) [lines 5-6] — Every matcher name exposed by {@link expectGeo}, derived from the…
geoSpecMatcherDescriptors (constant) [lines 8-13] — The 26-entry matcher registry
geoSpecMeshEvidenceMatchers (constant) [lines 15-16] — The matchers mesh evidence answers, as the GeoSpec authoring skill…
geoSpecRegionScopedKinds (constant) [lines 18-20] — The claims a region-cropped subject answers
geoSpecRegionWholeModelClaimCode (constant) [lines 22-23] — Diagnostic code of a whole-model claim made on a region-cropped…
defaultGeoSpecIgnoredDirectories (constant) [lines 25-26] — Directories skipped by recursive GeoSpec discovery unless callers provide their…
defaultGeoSpecInclude (constant) [lines 28-29] — Default file globs used by GeoSpec test discovery
geoSpecTimeoutCode (constant) [lines 31-48] [3 members] — Codes for GeoSpec wall-clock timeouts, one per bound
  geoSpecTimeoutCode.test (property) — A test's async body outlived `testTimeout` / `--test-timeout`
  geoSpecTimeoutCode.shard (property) — A pool shard outlived the non-verdict `--shard-timeout` watchdog
  geoSpecTimeoutCode.run (property) — A whole run outlived its host run budget
geoSpecFactsStoreVariable (constant) [lines 50-51] — The environment variable that selects the store
defaultGeoSpecSubjectRetention (constant) [lines 53-60] [2 members] — The retention a long-lived engine defaults to
  defaultGeoSpecSubjectRetention.subjects (property) — Subjects kept after each release, least recently loaded released first
  defaultGeoSpecSubjectRetention.memoryBytes (property) — Process resident set size, in bytes, above which a release…
geoSpecMaximumWorkUnitLimit (constant) [lines 62-63] — Default largest work-unit budget a spec may author through `loadModel`…

## Types — `public-api-types.md`

GeoSpecSubject (type) [lines 5-10] [1 members] — A model admitted by one live GeoSpec host scope
  GeoSpecSubject.[subjectBrand] (property)
GeoSpecUnit (type) [lines 12-14] — Geometry units accepted at GeoSpec evidence-loading boundaries
GeoSpecAssertion (type) [lines 16-66] [9 members] — Geometry assertion collected from a GeoSpec test module
  GeoSpecAssertion.kind (property) — Assertion kind
  GeoSpecAssertion.subject (property) — User-authored value passed to expectGeo()
  GeoSpecAssertion.expected (property) — Expected geometry condition
  GeoSpecAssertion.passed (property) — True when the assertion evaluated successfully
  GeoSpecAssertion.diagnostics (property) — Structured diagnostics from matcher evaluation
  GeoSpecAssertion.report (property) — Exact compiled assertion report, including core-owned bytes and polarity
  GeoSpecAssertion.loadId (property) — The host load which admitted this assertion's subject
  GeoSpecAssertion.region (property) — The crop this assertion's subject was loaded with
  GeoSpecAssertion.durationMs (property) — Wall-clock cost of matcher evaluation in milliseconds (budgeted matchers only)
GeoSpecAssemblyOccurrenceExpectation (type) [lines 68-91] [3 members] — Assembly occurrence rule accepted by `expectGeo(...).toHaveAssemblyOccurrences(...)`
  GeoSpecAssemblyOccurrenceExpectation.name (property)
  GeoSpecAssemblyOccurrenceExpectation.count (property)
  GeoSpecAssemblyOccurrenceExpectation.bounds (property)
GeoSpecAssemblyOccurrencesExpectation (type) [lines 93-101] [2 members] — Assembly occurrence expectation accepted by `expectGeo(...).toHaveAssemblyOccurrences(...)`
  GeoSpecAssemblyOccurrencesExpectation.occurrences (property)
  GeoSpecAssemblyOccurrencesExpectation.uniqueNames (property)
GeoSpecAxisExpectation (type) [lines 103-114] [3 members] — Axis-keyed numeric expectation used by high-level geometry matchers
  GeoSpecAxisExpectation.x (property)
  GeoSpecAxisExpectation.y (property)
  GeoSpecAxisExpectation.z (property)
GeoSpecBoundingBoxExpectation (type) [lines 116-133] [5 members] — Bounding-box expectation accepted by `expectGeo(...).toHaveBoundingBox(...)`
  GeoSpecBoundingBoxExpectation.min (property)
  GeoSpecBoundingBoxExpectation.max (property)
  GeoSpecBoundingBoxExpectation.size (property)
  GeoSpecBoundingBoxExpectation.center (property)
  GeoSpecBoundingBoxExpectation.tolerance (property)
GeoSpecCenterOfMassExpectation (type) [lines 135-145] [2 members] — Center-of-mass expectation accepted by `expectGeo(...).toHaveCenterOfMass(...)`
  GeoSpecCenterOfMassExpectation.point (property) — Exact expected point in millimetres
  GeoSpecCenterOfMassExpectation.tolerance (property)
GeoSpecChamferFeatureExpectation (type) [lines 147-158] [3 members] — Chamfer-feature expectation accepted by `expectGeo(...).toHaveChamferFeature(...)`
  GeoSpecChamferFeatureExpectation.distance (property)
  GeoSpecChamferFeatureExpectation.selection (property)
  GeoSpecChamferFeatureExpectation.tolerance (property)
GeoSpecCircularHoleExpectation (type) [lines 160-177] [5 members] — Circular-hole expectation accepted by `expectGeo(...).toHaveCircularHole(...)`
  GeoSpecCircularHoleExpectation.diameter (property)
  GeoSpecCircularHoleExpectation.through (property)
  GeoSpecCircularHoleExpectation.axis (property)
  GeoSpecCircularHoleExpectation.center (property)
  GeoSpecCircularHoleExpectation.tolerance (property)
GeoSpecCylindricalFaceExpectation (type) [lines 179-190] [3 members] — Cylindrical-face expectation accepted by `expectGeo(...).toHaveCylindricalFace(...)`
  GeoSpecCylindricalFaceExpectation.radius (property)
  GeoSpecCylindricalFaceExpectation.axis (property)
  GeoSpecCylindricalFaceExpectation.tolerance (property)
GeoSpecExpectOptions (type) [lines 192-204] [1 members] — Per-chain options accepted by `expectGeo(subject, options)`
  GeoSpecExpectOptions.workUnitLimit (property) — Deterministic work-unit budget for every claim in this chain
GeoSpecComponentInterferenceAllowance (type) [lines 206-223] [5 members] — Intentional component interference allowance accepted by `expectGeo(...).toHaveNoComponentInterference(...)`
  GeoSpecComponentInterferenceAllowance.kind (property)
  GeoSpecComponentInterferenceAllowance.left (property)
  GeoSpecComponentInterferenceAllowance.right (property)
  GeoSpecComponentInterferenceAllowance.maxVolume (property)
  GeoSpecComponentInterferenceAllowance.reason (property)
GeoSpecComponentInterferenceExpectation (type) [lines 225-237] [3 members] — Component-interference expectation accepted by `expectGeo(...).toHaveNoComponentInterference(...)`
  GeoSpecComponentInterferenceExpectation.tolerance (property)
  GeoSpecComponentInterferenceExpectation.pairs (property)
  GeoSpecComponentInterferenceExpectation.allowances (property)
GeoSpecComponentInterferencePairExpectation (type) [lines 239-247] [2 members] — A pair-specific component-interference check accepted by `expectGeo(...).toHaveNoComponentInterference(...)`
  GeoSpecComponentInterferencePairExpectation.left (property)
  GeoSpecComponentInterferencePairExpectation.right (property)
GeoSpecConnectedComponentsExpectation (type) [lines 249-260] [3 members] — Connected-components expectation accepted by `expectGeo(...).toHaveConnectedComponents(...)`
  GeoSpecConnectedComponentsExpectation.count (property)
  GeoSpecConnectedComponentsExpectation.tolerance (property)
  GeoSpecConnectedComponentsExpectation.toleranceMm (property)
GeoSpecGeometrySelector (type) [lines 262-296] — Geometry selector used by inspection and spatial relationship matchers
GeoSpecMatcher (type) [lines 298-524] [27 members] — Assertion chain returned by `expectGeo(subject)`
  GeoSpecMatcher.not (property) — Core-owned negation
  GeoSpecMatcher.toSatisfyRationalPlate (method) — Assert the fixed rational plate contract
  GeoSpecMatcher.toSatisfyParallelPlaneDistance (method) — Assert the fixed parallel-plane distance contract
  GeoSpecMatcher.toHaveBoundingBox (method) — Assert axis-aligned bounds, size, or center for a loaded geometry…
  GeoSpecMatcher.toHaveConnectedComponents (method) — Assert how many spatially disjoint chunks the mesh contains
  GeoSpecMatcher.toBeWatertight (method) — Assert closed mesh edge incidence
  GeoSpecMatcher.toHaveNoComponentInterference (method) — Assert that separate assembly components do not occupy the same…
  GeoSpecMatcher.toHaveAssemblyOccurrences (method) — Assert that named assembly occurrences exist with expected counts and…
  GeoSpecMatcher.toHaveSpatialRelationships (method) — Assert that selected entities satisfy declared spatial relationships
  GeoSpecMatcher.toHaveMeshIntegrity (method) — Assert rendered mesh evidence is internally trustworthy for downstream checks
  GeoSpecMatcher.toHaveNoDiagnostics (method) — Assert that the subject carries no diagnostics at the rejected…
  GeoSpecMatcher.toHaveSurfaceArea (method) — Assert total surface area, preferring exact BRep mass properties when…
  GeoSpecMatcher.toHaveVolume (method) — Assert enclosed volume, preferring exact BRep mass properties when available
  GeoSpecMatcher.toHaveMass (method) — Assert mass derived from exact mass properties or volume times…
  GeoSpecMatcher.toHaveCenterOfMass (method) — Assert the center of mass or mesh-derived centroid for a…
  GeoSpecMatcher.toBeValidBrep (method) — Assert that exact BRep evidence reports a valid shape
  GeoSpecMatcher.toHaveTopologyCounts (method) — Assert exact BRep topology counts
  GeoSpecMatcher.toHaveStepUnits (method) — Assert the STEP unit evidence
  GeoSpecMatcher.toHaveProductStructure (method) — Assert STEP product-structure evidence
  GeoSpecMatcher.toHavePlanarFace (method) — Assert that BRep evidence contains a planar face matching the…
  GeoSpecMatcher.toHaveCylindricalFace (method) — Assert that BRep evidence contains a cylindrical face with the…
  GeoSpecMatcher.toHaveCircularHole (method) — Assert that BRep evidence contains a circular hole matching diameter,…
  GeoSpecMatcher.toHaveCircularHolePattern (method) — Assert that BRep evidence contains a repeated circular-hole pattern
  GeoSpecMatcher.toHaveChamferFeature (method) — Assert that BRep evidence contains a chamfer feature with the…
  GeoSpecMatcher.toHaveFilletFeature (method) — Assert that BRep evidence contains a fillet feature with the…
  GeoSpecMatcher.toHaveMinimumWallThickness (method) — Assert that BRep evidence reports a minimum wall thickness satisfying…
  GeoSpecMatcher.toHaveVoidContinuity (method) — Assert that the declared waypoints share one connected void, stay…
GeoSpecMassExpectation (type) [lines 526-548] [4 members] — Mass expectation accepted by `expectGeo(...).toHaveMass(...)`
  GeoSpecMassExpectation.value (property)
  GeoSpecMassExpectation.density (property)
  GeoSpecMassExpectation.tolerance (property)
  GeoSpecMassExpectation.relativeTolerance (property) — Tolerance relative to the expected `value` (for example `1e-4`)
GeoSpecMeshIntegrityExpectation (type) [lines 550-581] [5 members] — Mesh integrity expectation accepted by `expectGeo(...).toHaveMeshIntegrity(...)`
  GeoSpecMeshIntegrityExpectation.finitePositions (property)
  GeoSpecMeshIntegrityExpectation.degenerateTriangles (property)
  GeoSpecMeshIntegrityExpectation.duplicateFaces (property)
  GeoSpecMeshIntegrityExpectation.watertight (property)
  GeoSpecMeshIntegrityExpectation.triangleCount (property)
GeoSpecNoDiagnosticsExpectation (type) [lines 583-589] [1 members] — Diagnostic severities rejected by `expectGeo(...).toHaveNoDiagnostics(...)`
  GeoSpecNoDiagnosticsExpectation.severities (property)
GeoSpecMinimumWallThicknessExpectation (type) [lines 591-599] [2 members] — Minimum-wall-thickness expectation accepted by `expectGeo(...).toHaveMinimumWallThickness(...)`
  GeoSpecMinimumWallThicknessExpectation.value (property)
  GeoSpecMinimumWallThicknessExpectation.tolerance (property)
GeoSpecCircularHolePatternExpectation (type) [lines 601-621] [6 members] — Circular-hole-pattern expectation accepted by `expectGeo(...).toHaveCircularHolePattern(...)`
  GeoSpecCircularHolePatternExpectation.count (property)
  GeoSpecCircularHolePatternExpectation.holeDiameter (property)
  GeoSpecCircularHolePatternExpectation.boltCircleDiameter (property)
  GeoSpecCircularHolePatternExpectation.axis (property)
  GeoSpecCircularHolePatternExpectation.center (property)
  GeoSpecCircularHolePatternExpectation.tolerance (property)
GeoSpecFilletFeatureExpectation (type) [lines 623-634] [3 members] — Fillet-feature expectation accepted by `expectGeo(...).toHaveFilletFeature(...)`
  GeoSpecFilletFeatureExpectation.radius (property)
  GeoSpecFilletFeatureExpectation.selection (property)
  GeoSpecFilletFeatureExpectation.tolerance (property)
GeoSpecNumericExpectation (type) [lines 636-643] — Shared scalar expectation used by geometry measurements
GeoSpecPlanarFaceExpectation (type) [lines 645-659] [4 members] — Planar-face expectation accepted by `expectGeo(...).toHavePlanarFace(...)`
  GeoSpecPlanarFaceExpectation.normal (property)
  GeoSpecPlanarFaceExpectation.offset (property)
  GeoSpecPlanarFaceExpectation.area (property)
  GeoSpecPlanarFaceExpectation.tolerance (property)
GeoSpecPointExpectation (type) [lines 661-662] — Point expectation accepted by center and feature matchers
GeoSpecProductStructureExpectation (type) [lines 664-672] [2 members] — Product-structure expectation accepted by `expectGeo(...).toHaveProductStructure(...)`
  GeoSpecProductStructureExpectation.names (property)
  GeoSpecProductStructureExpectation.count (property)
GeoSpecSpatialRelationshipExpectation (type) [lines 674-720] [13 members] — One spatial relationship accepted by `expectGeo(...).toHaveSpatialRelationships(...)`
  GeoSpecSpatialRelationshipExpectation.id (property)
  GeoSpecSpatialRelationshipExpectation.kind (property)
  GeoSpecSpatialRelationshipExpectation.subject (property)
  GeoSpecSpatialRelationshipExpectation.target (property)
  GeoSpecSpatialRelationshipExpectation.tolerance (property)
  GeoSpecSpatialRelationshipExpectation.angularToleranceDegrees (property)
  GeoSpecSpatialRelationshipExpectation.angleDegrees (property) — Expected angle in degrees for `kind
  GeoSpecSpatialRelationshipExpectation.axis (property) — Declared insertion axis (subject-frame direction) for `kind
  GeoSpecSpatialRelationshipExpectation.min (property)
  GeoSpecSpatialRelationshipExpectation.max (property)
  GeoSpecSpatialRelationshipExpectation.minVolume (property)
  GeoSpecSpatialRelationshipExpectation.maxVolume (property)
  GeoSpecSpatialRelationshipExpectation.reason (property)
GeoSpecSpatialRelationshipsExpectation (type) [lines 722-727] [1 members] — Spatial relationship expectation accepted by `expectGeo(...).toHaveSpatialRelationships(...)`
  GeoSpecSpatialRelationshipsExpectation.relationships (property)
GeoSpecStepUnitsExpectation (type) [lines 729-734] [1 members] — STEP unit expectation accepted by `expectGeo(...).toHaveStepUnits(...)`
  GeoSpecStepUnitsExpectation.unit (property)
GeoSpecSurfaceAreaExpectation (type) [lines 736-755] [3 members] — Surface-area expectation accepted by `expectGeo(...).toHaveSurfaceArea(...)`
  GeoSpecSurfaceAreaExpectation.value (property)
  GeoSpecSurfaceAreaExpectation.tolerance (property)
  GeoSpecSurfaceAreaExpectation.relativeTolerance (property) — Tolerance relative to the expected `value` (for example `1e-4`)
GeoSpecTopologyCountsExpectation (type) [lines 757-783] [8 members] — Topology-count expectation accepted by `expectGeo(...).toHaveTopologyCounts(...)`
  GeoSpecTopologyCountsExpectation.vertices (property)
  GeoSpecTopologyCountsExpectation.edges (property)
  GeoSpecTopologyCountsExpectation.wires (property)
  GeoSpecTopologyCountsExpectation.faces (property)
  GeoSpecTopologyCountsExpectation.shells (property)
  GeoSpecTopologyCountsExpectation.solids (property)
  GeoSpecTopologyCountsExpectation.compounds (property)
  GeoSpecTopologyCountsExpectation.tolerance (property)
GeoSpecValidBrepExpectation (type) [lines 785-809] [6 members] — Exact BRep validity expectation accepted by `expectGeo(...).toBeValidBrep(...)`
  GeoSpecValidBrepExpectation.maxTolerance (property)
  GeoSpecValidBrepExpectation.freeBounds (property)
  GeoSpecValidBrepExpectation.minEdgeLength (property)
  GeoSpecValidBrepExpectation.sameParameter (property)
  GeoSpecValidBrepExpectation.closedShells (property)
  GeoSpecValidBrepExpectation.closedWires (property)
GeoSpecVoidContinuityExpectation (type) [lines 811-851] [5 members] — Void-continuity expectation accepted by `expectGeo(...).toHaveVoidContinuity(...)`
  GeoSpecVoidContinuityExpectation.path (property) — Ordered waypoints (>= 1) known to lie in the void…
  GeoSpecVoidContinuityExpectation.material (property) — Occurrence names whose solids bound the void
  GeoSpecVoidContinuityExpectation.minCrossSection (property) — Minimum required bottleneck cross-section (mm²), sampled
  GeoSpecVoidContinuityExpectation.isolatedFrom (property) — Points that must NOT be reachable from the path void…
  GeoSpecVoidContinuityExpectation.bounds (property) — Region bounded for the proof (subject frame)
GeoSpecVoidWaypoint (type) [lines 853-856] — One void-continuity waypoint
GeoSpecVolumeExpectation (type) [lines 858-877] [3 members] — Volume expectation accepted by `expectGeo(...).toHaveVolume(...)`
  GeoSpecVolumeExpectation.value (property)
  GeoSpecVolumeExpectation.tolerance (property)
  GeoSpecVolumeExpectation.relativeTolerance (property) — Tolerance relative to the expected `value` (for example `1e-4`)
GeometryDiagnostic (type) [lines 879-907] [6 members] — Diagnostic emitted by GeoSpec loaders, analyzers, and matchers
  GeometryDiagnostic.code (property)
  GeometryDiagnostic.severity (property)
  GeometryDiagnostic.message (property)
  GeometryDiagnostic.suggestion (property)
  GeometryDiagnostic.spatial (property)
  GeometryDiagnostic.details (property)
Vec3 (type) [lines 909-910] — Numeric 3D vector
GeoSpecVec3 (type) [lines 912-913] — Numeric 3D vector
GeoSpecMatcherDescriptor (type) [lines 915-926] [3 members] — One matcher's contract entry
  GeoSpecMatcherDescriptor.kind (property)
  GeoSpecMatcherDescriptor.expected (property)
  GeoSpecMatcherDescriptor.mode (property)
GeoSpecMatcherExpectedShape (type) [lines 928-930] — How the substrate derives an assertion's recorded `expected` value from…
GeoSpecMatcherMode (type) [lines 932-933] — Whether a matcher settles synchronously (throwing its `GeoSpecAssertionError` inside the…
GeoSpecMatcherName (type) [lines 935-936] — Every matcher name exposed by `expectGeo(...)`
GeoSpecAccuracyRefusalCode (type) [lines 938-940] — Typed refusals of the accuracy contract
GeoSpecFactAccuracy (type) [lines 942-976] [5 members] — The accuracy record of one numeric fact, reported as `evidence.accuracy`
  GeoSpecFactAccuracy.bound (property) — Absolute error bound in the fact's own unit
  GeoSpecFactAccuracy.achieved (property) — The bound relative to the measurement for volume, area and…
  GeoSpecFactAccuracy.provenance (property)
  GeoSpecFactAccuracy.fileTolerance (property) — The file's own maximum topology tolerance in mm, when the…
  GeoSpecFactAccuracy.verdictRule (property) — `legacy`
GeoSpecFactProvenanceMethod (type) [lines 978-980] — How a fact was computed, which decides what its `bound`…
GeoSpecNumericProfile (type) [lines 982-983] — The engine serves `geospec-demand-v6` and refuses the declared bounded-facts profile…
MeshBufferSource (type) [lines 985-999] [4 members] — In-memory triangle mesh source
  MeshBufferSource.format (property)
  MeshBufferSource.positions (property)
  MeshBufferSource.indices (property)
  MeshBufferSource.name (property)
MeshSource (type) [lines 1001-1002] — Mesh source forms accepted by {@link import ('./load-model.js').loadModel}
GeometryExportIntent (type) [lines 1004-1071] [3 members] — Runtime export intent recorded when GeoSpec obtains evidence from a…
  GeometryExportIntent.requested (property)
  GeometryExportIntent.honored (property)
  GeometryExportIntent.route (property)
GeometryFileFormat (type) [lines 1073-1074] — Geometry file formats a GeoSpec export intent can name
MeshFileFormat (type) [lines 1076-1077] — Geometry file formats supported by the P0 mesh loader
CreateModelLoaderOptions (type) [lines 1079-1140] [10 members] — Defaults accepted by {@link import ('./load-model.js').createModelLoader}
  CreateModelLoaderOptions.engine (property) — Initialized compiled engine supplied by the host, never selected by…
  CreateModelLoaderOptions.readSource (property) — Rooted host reader for direct filesystem or URL sources
  CreateModelLoaderOptions.format (property) — Geometry format to export when an individual call does not…
  CreateModelLoaderOptions.exporters (property) — Model exporters for source-kind files
  CreateModelLoaderOptions.projectPath (property) — Absolute project root passed to the exporter
  CreateModelLoaderOptions.stepStreaming (property) — STEP reader strategy used for STEP sources or exports
  CreateModelLoaderOptions.mesh (property) — Whether STEP loading should also produce mesh evidence
  CreateModelLoaderOptions.meshLinearTolerance (property) — Linear tolerance used while meshing exact BRep evidence
  CreateModelLoaderOptions.meshAngularToleranceDegrees (property) — Angular tolerance in degrees used while meshing exact BRep evidence
  CreateModelLoaderOptions.retention (property) — Keep subjects admitted by this loader's scopes for later loaders…
GeoSpecExporterCapabilities (type) [lines 1142-1164] [4 members] — What a {@link GeoSpecModelExporter} declares it can do
  GeoSpecExporterCapabilities.formats (property) — The formats `export` produces
  GeoSpecExporterCapabilities.parameters (property) — Accepts `parameters`
  GeoSpecExporterCapabilities.region (property) — Crops to `region` (an axis-aligned millimetre box) before export
  GeoSpecExporterCapabilities.inlineSources (property) — Accepts `inline` sources, which back `loadModel({ code })`
GeoSpecExportFormat (type) [lines 1166-1167] — Formats a {@link GeoSpecModelExporter} may produce from a source-kind file
GeoSpecLoadRegion (type) [lines 1169-1177] [2 members] — Axis-aligned crop box in canonical Z-up millimetres
  GeoSpecLoadRegion.min (property)
  GeoSpecLoadRegion.max (property)
GeoSpecModelArtifact (type) [lines 1179-1222] [7 members] — The bytes an exporter returns, in the canonical Z-up millimetre…
  GeoSpecModelArtifact.bytes (property)
  GeoSpecModelArtifact.format (property)
  GeoSpecModelArtifact.sourceUnit (property) — Unit of raw GLB coordinates
  GeoSpecModelArtifact.resources (property) — External resources the primary bytes reference, by name
  GeoSpecModelArtifact.consumedSources (property) — Every source file the export read, as a path from…
  GeoSpecModelArtifact.diagnostics (property) — Notices from a successful export, admitted with the subject so…
  GeoSpecModelArtifact.exportOptions (property) — The export options the exporter applied, recorded in the load…
GeoSpecModelExporter (type) [lines 1224-1258] [6 members] — Turns a source-kind model file into GLB or STEP bytes…
  GeoSpecModelExporter.id (property)
  GeoSpecModelExporter.extensions (property)
  GeoSpecModelExporter.capabilities (property)
  GeoSpecModelExporter.export (method)
  GeoSpecModelExporter.revision (method) — The sources an export of `file` would read now, without…
  GeoSpecModelExporter.close (method) — Releases what the exporter created for one scope
GeoSpecModelExportRequest (type) [lines 1260-1304] [8 members] — One export request
  GeoSpecModelExportRequest.file (property) — Entry file, as a path from the project root
  GeoSpecModelExportRequest.projectPath (property) — Absolute project root the exporter resolves `file` and its imports…
  GeoSpecModelExportRequest.format (property)
  GeoSpecModelExportRequest.parameters (property) — JSON-serialisable parameter values
  GeoSpecModelExportRequest.mesh (property) — Tessellation tolerances for a mesh export
  GeoSpecModelExportRequest.region (property) — Millimetre crop box
  GeoSpecModelExportRequest.inline (property) — Source text keyed by project-relative path
  GeoSpecModelExportRequest.signal (property)
GeoSpecModelFormat (type) [lines 1306-1307] — Geometry formats accepted by {@link import ('./load-model.js').loadModel}
GeoSpecModelLoader (type) [lines 1309-1310] — Function shape used by GeoSpec runners to provide model loading…
ManagedGeoSpecModelLoader (type) [lines 1312-1318] [1 members] — A configured loader whose admissions can be released with its…
  ManagedGeoSpecModelLoader.dispose (method)
GeoSpecConsumedSources (type) [lines 1320-1324] — The source files one export read, each with its SHA-256…
LoadModelCodeOptions (type) [lines 1326-1398] [12 members] — Inline code-CAD model load options
  LoadModelCodeOptions.code (property) — Source files keyed by project-relative path
  LoadModelCodeOptions.file (property) — Entry path to render from {@link code }
  LoadModelCodeOptions.rootDir (property) — Not accepted
  LoadModelCodeOptions.format (property) — Geometry format to export
  LoadModelCodeOptions.parameters (property) — Explicit parameters passed to the exporter
  LoadModelCodeOptions.projectPath (property) — Absolute project root passed to the exporter
  LoadModelCodeOptions.stepStreaming (property) — STEP reader strategy used for STEP exports
  LoadModelCodeOptions.mesh (property) — Whether STEP loading should also produce mesh evidence
  LoadModelCodeOptions.meshLinearTolerance (property) — Linear tolerance used while meshing exact BRep evidence
  LoadModelCodeOptions.meshAngularToleranceDegrees (property) — Angular tolerance in degrees used while meshing exact BRep evidence
  LoadModelCodeOptions.workUnitLimit (property) — Default work-unit budget for every claim on this subject
  LoadModelCodeOptions.region (property) — Axis-aligned millimetre box the exporter crops the model to before…
LoadModelFileOptions (type) [lines 1400-1474] [11 members] — Filesystem-backed model load options
  LoadModelFileOptions.file (property) — Model file to render, as a path from the project…
  LoadModelFileOptions.rootDir (property) — Project-relative folder `file` resolves in, for a suite that loads…
  LoadModelFileOptions.projectPath (property) — Absolute project root passed to the exporter
  LoadModelFileOptions.format (property) — Geometry format to export
  LoadModelFileOptions.parameters (property) — Explicit parameters passed to the exporter
  LoadModelFileOptions.stepStreaming (property) — STEP reader strategy used for STEP exports
  LoadModelFileOptions.mesh (property) — Whether STEP loading should also produce mesh evidence
  LoadModelFileOptions.meshLinearTolerance (property) — Linear tolerance used while meshing exact BRep evidence
  LoadModelFileOptions.meshAngularToleranceDegrees (property) — Angular tolerance in degrees used while meshing exact BRep evidence
  LoadModelFileOptions.workUnitLimit (property) — Default work-unit budget for every claim on this subject
  LoadModelFileOptions.region (property) — Axis-aligned millimetre box the exporter crops the model to before…

## Types (2) — `public-api-types-2.md`

LoadModelOptions (type) [lines 5-30] [8 members] — Options accepted by {@link import ('./load-model.js').loadModel}
  LoadModelOptions.rootDir (property) — Project-relative folder `file` resolves in, for a suite that loads…
  LoadModelOptions.format (property) — Geometry format to export
  LoadModelOptions.parameters (property) — Explicit parameters passed to the exporter
  LoadModelOptions.stepStreaming (property) — STEP reader strategy used for STEP exports
  LoadModelOptions.mesh (property) — Whether STEP loading should also produce mesh evidence
  LoadModelOptions.meshLinearTolerance (property) — Linear tolerance used while meshing exact BRep evidence
  LoadModelOptions.meshAngularToleranceDegrees (property) — Angular tolerance in degrees used while meshing exact BRep evidence
  LoadModelOptions.workUnitLimit (property) — Default work-unit budget for every claim on this subject
LoadModelSourceOptions (type) [lines 32-112] [13 members] — Direct geometry-source model load options
  LoadModelSourceOptions.source (property) — Geometry bytes, path, browser file/blob, or in-memory mesh buffer
  LoadModelSourceOptions.resources (property) — Named external resources consumed alongside the direct geometry bytes
  LoadModelSourceOptions.rootDir (property) — Project-relative folder a path `source` and each resource path resolve…
  LoadModelSourceOptions.format (property) — Source geometry format
  LoadModelSourceOptions.path (property) — Source path recorded in provenance
  LoadModelSourceOptions.name (property) — Human-readable source name recorded in provenance
  LoadModelSourceOptions.sourceUnit (property) — Coordinate unit of raw GLB/glTF or mesh-buffer data before canonical…
  LoadModelSourceOptions.parameters (property) — Explicit parameters recorded in provenance
  LoadModelSourceOptions.stepStreaming (property) — STEP reader strategy used for STEP sources
  LoadModelSourceOptions.mesh (property) — Whether STEP loading should also produce mesh evidence
  LoadModelSourceOptions.meshLinearTolerance (property) — Linear tolerance used while meshing exact BRep evidence
  LoadModelSourceOptions.meshAngularToleranceDegrees (property) — Angular tolerance in degrees used while meshing exact BRep evidence
  LoadModelSourceOptions.workUnitLimit (property) — Default work-unit budget for every claim on this subject
CommandExporterOptions (type) [lines 114-149] [7 members] — Options for {@link commandExporter}
  CommandExporterOptions.id (property)
  CommandExporterOptions.extensions (property) — File extensions without the dot, or `'*'`
  CommandExporterOptions.command (property) — Executable and leading arguments, such as `['python', 'export.py']`
  CommandExporterOptions.capabilities (property) — What the command can do
  CommandExporterOptions.cwd (property) — Working directory of the command
  CommandExporterOptions.env (property) — Extra environment variables, merged over this process's
  CommandExporterOptions.exportTimeout (property) — Positive milliseconds one export may run before the command is…
GeoSpecCollector (type) [lines 151-180] [7 members] — Collects suites, tests, assertions, and async completion state for one…
  GeoSpecCollector.tests (property)
  GeoSpecCollector.describe (method)
  GeoSpecCollector.describeSkip (method)
  GeoSpecCollector.it (method)
  GeoSpecCollector.itSkip (method)
  GeoSpecCollector.expectGeo (method)
  GeoSpecCollector.waitForCompletion (method)
GeoSpecCollectorOptions (type) [lines 182-192] [2 members] — Per-module collector configuration
  GeoSpecCollectorOptions.nativeAssertions (property)
  GeoSpecCollectorOptions.onTestEvent (property) — Observes each selected test as it starts and settles, for…
GeoSpecTestNamePattern (type) [lines 194-195] — Compiled Vitest-style test-name pattern used by a GeoSpec run
GeoSpecTimeoutCode (type) [lines 197-198] — One GeoSpec timeout code
DiscoverGeoSpecFilesOptions (type) [lines 200-221] [6 members] — Options for recursive GeoSpec test discovery
  DiscoverGeoSpecFilesOptions.filesystem (property)
  DiscoverGeoSpecFilesOptions.projectPath (property)
  DiscoverGeoSpecFilesOptions.files (property)
  DiscoverGeoSpecFilesOptions.include (property)
  DiscoverGeoSpecFilesOptions.exclude (property)
  DiscoverGeoSpecFilesOptions.ignoredDirectories (property)
GeoSpecDiscoveryFileKind (type) [lines 223-224] — File kind returned by a GeoSpec discovery filesystem
GeoSpecDiscoveryFileStat (type) [lines 226-231] [1 members] — Minimal stat object required for recursive GeoSpec test discovery
  GeoSpecDiscoveryFileStat.kind (property)
GeoSpecDiscoveryFileSystem (type) [lines 233-244] [2 members] — Minimal filesystem contract used by GeoSpec test discovery
  GeoSpecDiscoveryFileSystem.readdir (method)
  GeoSpecDiscoveryFileSystem.stat (method)
GeoSpecDiscoveryResult (type) [lines 246-255] [2 members] — Result returned by recursive GeoSpec test discovery
  GeoSpecDiscoveryResult.files (property)
  GeoSpecDiscoveryResult.unmatchedRoots (property)
GeoSpecModuleBundleCache (type) [lines 257-274] — Worker-local cache for successful GeoSpec bundles
GeoSpecRunFailure (type) [lines 276-298] [6 members] — Failed GeoSpec run result
  GeoSpecRunFailure.success (property)
  GeoSpecRunFailure.issues (property)
  GeoSpecRunFailure.bundle (property)
  GeoSpecRunFailure.accounting (property)
  GeoSpecRunFailure.lineage (property)
  GeoSpecRunFailure.tests (property) — Tests registered before a module execution failure, including their retained…
GeoSpecRunResult (type) [lines 300-312] [5 members] — Result returned by {@link import ('./run-geospec-module.js').runGeoSpecModule}
  GeoSpecRunResult.success (property)
  GeoSpecRunResult.bundle (property)
  GeoSpecRunResult.accounting (property)
  GeoSpecRunResult.lineage (property)
  GeoSpecRunResult.tests (property) — Tests registered before a module execution failure, including their retained…
GeoSpecRunSuccess (type) [lines 314-336] [6 members] — Successful GeoSpec run result
  GeoSpecRunSuccess.success (property)
  GeoSpecRunSuccess.passed (property) — True only when selected tests completed successfully with coherent available…
  GeoSpecRunSuccess.tests (property)
  GeoSpecRunSuccess.bundle (property)
  GeoSpecRunSuccess.accounting (property)
  GeoSpecRunSuccess.lineage (property)
GeoSpecTestCase (type) [lines 338-375] [7 members] — A collected GeoSpec test case
  GeoSpecTestCase.ordinal (property) — Registration ordinal before filtering, scoped to the source module's collection
  GeoSpecTestCase.suite (property) — Hierarchical suite path
  GeoSpecTestCase.name (property) — Test case name
  GeoSpecTestCase.assertions (property) — Assertions produced by the test callback
  GeoSpecTestCase.status (property) — Final test status
  GeoSpecTestCase.diagnostics (property) — Structured diagnostics emitted by test execution
  GeoSpecTestCase.durationMs (property) — Wall-clock cost of the test body plus its pending assertions,…
GeoSpecTestEvent (type) [lines 377-385] [2 members] — A selected test starting or settling inside a module run
  GeoSpecTestEvent.type (property)
  GeoSpecTestEvent.test (property)
GeoSpecTestStatus (type) [lines 387-388] — Test case status after runner collection
RunGeoSpecModuleOptions (type) [lines 390-463] [11 members] — Options for executing a GeoSpec ESM test module
  RunGeoSpecModuleOptions.filesystem (property) — Filesystem containing the test module and its project imports
  RunGeoSpecModuleOptions.entryPath (property) — Filesystem-root-relative ESM test entry path
  RunGeoSpecModuleOptions.testNamePattern (property) — JavaScript regular expression matched against full `suite > test` names
  RunGeoSpecModuleOptions.testTimeout (property) — Timeout for async test callbacks, in milliseconds
  RunGeoSpecModuleOptions.onTestEvent (property) — Observes each selected test as it starts and settles, for…
  RunGeoSpecModuleOptions.nativeAssertions (property) — Host-provided protocol-3 compiled assertion client
  RunGeoSpecModuleOptions.nativeModelLoader (property) — Host-composed identity loader
  RunGeoSpecModuleOptions.builtinModules (property) — Additional in-memory modules made available to the VM
  RunGeoSpecModuleOptions.createModuleVm (property) — Module VM factory
  RunGeoSpecModuleOptions.bundleCache (property) — Successful bundle cache owned by a serial runner worker
  RunGeoSpecModuleOptions.collectOnly (property) — List-only collection pass (shard splitting)
BuiltinModule (type) [lines 465-498] [5 members] — A pre-bundled module served from memory under its import specifier
  BuiltinModule.code (property) — ESM source of the module
  BuiltinModule.version (property) — Version reported for the module
  BuiltinModule.globalName (property) — Script global that bundles bind to this module
  BuiltinModule.package (property) — The npm package this module stands for, as a project…
  BuiltinModule.registryExpression (property) — JavaScript expression evaluating to the module's exports, which a bundle…
BundleResult (type) [lines 500-520] [6 members] — Outcome of bundling an entry point
  BundleResult.code (property)
  BundleResult.sourceMap (property)
  BundleResult.issues (property)
  BundleResult.success (property)
  BundleResult.dependencies (property)
  BundleResult.unresolvedPaths (property)
CreateModuleVm (type) [lines 522-523] — Create a module VM
GeoSpecModuleVmModule (type) [lines 525-530] [1 members] — A module a Node pool worker imports by URL for…
  GeoSpecModuleVmModule.default (property)
ModuleVm (type) [lines 532-570] [5 members] — The ESM module VM GeoSpec bundles and executes specs and…
  ModuleVm.detectImports (method) — Detect bare specifiers and project dependencies without fetching anything
  ModuleVm.bundle (method) — Bundle an ESM entry point and its transitive dependency graph
  ModuleVm.execute (method) — Execute bundled ESM code in the current JavaScript environment
  ModuleVm.registerModule (method) — Register an in-memory builtin module
  ModuleVm.dispose (method) — Release VM-owned resources
ModuleVmOptions (type) [lines 572-579] [1 members] — Options GeoSpec passes when it creates a module VM
  ModuleVmOptions.filesystem (property) — Filesystem the VM reads project files from
VmExecuteResult (type) [lines 581-591] [1 members] — Result of executing bundled ESM code
  VmExecuteResult.success (property)
VmFileSystem (type) [lines 593-622] [4 members] — Minimal filesystem contract required by the VM bundler
  VmFileSystem.exists (method) — Return true when a path exists
  VmFileSystem.readFile (method) — Read a file as binary bytes
  VmFileSystem.writeFile (method) — Write a UTF-8 text file
  VmFileSystem.ensureDir (method) — Ensure a directory exists, creating parents as needed
VmIssue (type) [lines 624-668] [6 members] — A diagnostic emitted while bundling or executing a VM module
  VmIssue.message (property) — Human-readable diagnostic message
  VmIssue.code (property) — Stable machine-readable code
  VmIssue.type (property) — Diagnostic category
  VmIssue.severity (property) — Diagnostic severity
  VmIssue.location (property) — Optional source location
  VmIssue.details (property) — Optional original error object or metadata
GeoSpecNativeRunnerAssertions (type) [lines 670-687] [6 members] — Native assertion options whose engine can also admit and release…
  GeoSpecNativeRunnerAssertions.claimId (property)
  GeoSpecNativeRunnerAssertions.evidenceProfile (property) — Success-evidence profile of every claim and query
  GeoSpecNativeRunnerAssertions.subjectSlot (property)
  GeoSpecNativeRunnerAssertions.workUnitLimit (property)
  GeoSpecNativeRunnerAssertions.maximumWorkUnitLimit (property) — The largest `workUnitLimit` a spec may author in `loadModel` or…
  GeoSpecNativeRunnerAssertions.engine (property)
GeoSpecNativeRunnerOptions (type) [lines 689-715] [6 members] — Options for the native serial runner
  GeoSpecNativeRunnerOptions.filesystem (property) — Filesystem containing the project and test modules
  GeoSpecNativeRunnerOptions.builtinModules (property) — Additional in-memory modules made available to the VM
  GeoSpecNativeRunnerOptions.createModuleVm (property) — Module VM factory for every file
  GeoSpecNativeRunnerOptions.nativeAssertions (property) — Actual protocol-3 engine used by authored assertions
  GeoSpecNativeRunnerOptions.nativeModelLoader (property) — Optional managed loader
  GeoSpecNativeRunnerOptions.model (property) — Defaults used when the runner constructs its own native loader
CreateGeoSpecNativeModelLoaderOptions (type) [lines 717-767] [8 members] — Defaults and host dependencies for a managed native model loader
  CreateGeoSpecNativeModelLoaderOptions.engine (property)
  CreateGeoSpecNativeModelLoaderOptions.format (property)
  CreateGeoSpecNativeModelLoaderOptions.projectPath (property)
  CreateGeoSpecNativeModelLoaderOptions.readSource (property)
  CreateGeoSpecNativeModelLoaderOptions.exporters (property) — Model exporters for source-kind files
  CreateGeoSpecNativeModelLoaderOptions.carried (property) — Subject handles carried between the release scopes of loaders that…
  CreateGeoSpecNativeModelLoaderOptions.retainedSubjects (property) — With a carrier, how many subjects `releaseAll` keeps for later…
  CreateGeoSpecNativeModelLoaderOptions.maximumWorkUnitLimit (property) — The largest `loadModel` `workUnitLimit` a spec may author
GeoSpecNativeLoadModelOptions (type) [lines 769-805] [10 members] — Native additions accepted by the injected `geospec/runner/native` loader
  GeoSpecNativeLoadModelOptions.rootDir (property) — Project-relative folder `file` resolves in, for a suite that loads…
  GeoSpecNativeLoadModelOptions.format (property) — Geometry format to export
  GeoSpecNativeLoadModelOptions.parameters (property) — Explicit parameters passed to the exporter
  GeoSpecNativeLoadModelOptions.stepStreaming (property) — STEP reader strategy used for STEP exports
  GeoSpecNativeLoadModelOptions.mesh (property) — Whether STEP loading should also produce mesh evidence
  GeoSpecNativeLoadModelOptions.meshLinearTolerance (property) — Linear tolerance used while meshing exact BRep evidence
  GeoSpecNativeLoadModelOptions.meshAngularToleranceDegrees (property) — Angular tolerance in degrees used while meshing exact BRep evidence
  GeoSpecNativeLoadModelOptions.workUnitLimit (property) — Default work-unit budget for every claim on this subject
  GeoSpecNativeLoadModelOptions.resources (property) — Ordered external resource payloads declared to the native admission request
  GeoSpecNativeLoadModelOptions.ingestOptions (property) — Format-specific native ingest options
GeoSpecNativeModelEngine (type) [lines 807-827] [5 members] — Native engine operations required for model admission and run-level cleanup
  GeoSpecNativeModelEngine.evaluateClaim (method)
  GeoSpecNativeModelEngine.processRequest (method)
  GeoSpecNativeModelEngine.ingestSubject (method)
  GeoSpecNativeModelEngine.subjectHandle (method)
  GeoSpecNativeModelEngine.releaseSubject (method)
GeoSpecNativeModelLoader (type) [lines 829-831] — Model loader injected into native VM runs
GeoSpecNativeModelSubject (type) [lines 833-845] [3 members] — Subject identity returned by native STEP/GLB admission
  GeoSpecNativeModelSubject.contentHash (property)
  GeoSpecNativeModelSubject.subjectHash (property)
  GeoSpecNativeModelSubject.load (property) — Exact successful-load evidence retained by the admitting host, when available
GeoSpecNativeModelResource (type) [lines 847-855] [2 members] — One named external resource referenced by a direct glTF-family source
  GeoSpecNativeModelResource.name (property)
  GeoSpecNativeModelResource.source (property)
GeoSpecNativeSourceReader (type) [lines 857-859] — Resolve a non-memory source into ordinary ArrayBuffer-backed bytes
ManagedGeoSpecNativeModelLoader (type) [lines 861-881] [1 members] — Reusable native model loader whose admitted subjects can be released…
  ManagedGeoSpecNativeModelLoader.releaseAll (method) — Drain registered admissions, including additions while drainage awaits, then release…
CreateNodeGeoSpecEngineOptions (type) [lines 883-903] [3 members] — Options for {@link createNodeGeoSpecEngine}
  CreateNodeGeoSpecEngineOptions.executionPermits (property) — Caller-inclusive CPU permits for the native addon
  CreateNodeGeoSpecEngineOptions.projectPath (property) — The project the engine serves
  CreateNodeGeoSpecEngineOptions.factsStore (property) — An explicit facts store, or `false` for none
GeoSpecEngineBinding (type) [lines 905-906] — Which build of `geospec-engine` a Node engine runs on
NodeGeoSpecEngine (type) [lines 908-925] [3 members] — An engine and the build it runs on
  NodeGeoSpecEngine.engine (property)
  NodeGeoSpecEngine.binding (property)
  NodeGeoSpecEngine.factsStore (property) — The root of the facts store the engine opened
GeoSpecFactsStore (type) [lines 927-935] [2 members] — A persisted facts store for a native engine
  GeoSpecFactsStore.root (property)
  GeoSpecFactsStore.projectRoot (property)
GeoSpecNodePoolChildOptionsMessage (type) [lines 937-945] [2 members] — The first message a pool child process receives
  GeoSpecNodePoolChildOptionsMessage.type (property)
  GeoSpecNodePoolChildOptionsMessage.options (property)
GeoSpecNodePoolOptions (type) [lines 947-1025] [10 members] — Options for the Node pool
  GeoSpecNodePoolOptions.projectPath (property) — Absolute project root
  GeoSpecNodePoolOptions.configPath (property) — The `geospec.config.*` each worker imports for its exporters
  GeoSpecNodePoolOptions.exporterModules (property) — URLs of modules each worker imports whose default export creates…
  GeoSpecNodePoolOptions.moduleVmModule (property) — URL of a module each worker imports whose default export…
  GeoSpecNodePoolOptions.workers (property) — Number of outer worker isolates
  GeoSpecNodePoolOptions.isolation (property) — Where each worker runs
  GeoSpecNodePoolOptions.budget (property) — Caller-inclusive total CPU budget
  GeoSpecNodePoolOptions.shardTimeout (property) — Non-verdict shard watchdog in milliseconds
  GeoSpecNodePoolOptions.sources (property) — Spec sources each worker serves from memory, keyed by project-rooted…
  GeoSpecNodePoolOptions.retention (property) — Keep each worker's admitted subjects, with the facts its engine…
NodeChildProcessLike (type) [lines 1027-1049] [4 members] — The slice of `node:child_process`' `ChildProcess` the pool drives
  NodeChildProcessLike.pid (property)
  NodeChildProcessLike.send (method)
  NodeChildProcessLike.on (method)
  NodeChildProcessLike.kill (method)
NodeWorkerLike (type) [lines 1051-1070] [3 members] — The slice of `node:worker_threads`' `Worker` the pool drives
  NodeWorkerLike.postMessage (method)
  NodeWorkerLike.on (method)
  NodeWorkerLike.terminate (method)
GeoSpecExporterModule (type) [lines 1072-1081] [1 members] — A module whose default export creates exporters for one worker
  GeoSpecExporterModule.default (property)
GeoSpecSubjectRetention (type) [lines 1083-1095] [2 members] — How many admitted subjects a long-lived engine keeps between shards,…
  GeoSpecSubjectRetention.subjects (property) — Subjects kept after each release, least recently loaded released first
  GeoSpecSubjectRetention.memoryBytes (property) — Process resident set size, in bytes, above which a release…
GeoSpecPoolHostMessage (type) [lines 1097-1114] [1 members]
  GeoSpecPoolHostMessage.type (property)
GeoSpecPoolShard (type) [lines 1116-1133] [3 members] — One schedulable work unit
  GeoSpecPoolShard.id (property) — Stable shard id, unique within one pool run
  GeoSpecPoolShard.file (property) — GeoSpec file this shard executes
  GeoSpecPoolShard.testNamePattern (property) — Exact-test pattern for split shards (`(file, testNamePattern)` work units)
GeoSpecPoolWorkerHandle (type) [lines 1135-1160] [4 members]
  GeoSpecPoolWorkerHandle.postMessage (method)
  GeoSpecPoolWorkerHandle.onMessage (method)
  GeoSpecPoolWorkerHandle.onExit (method)
  GeoSpecPoolWorkerHandle.terminate (method) — Hard-stop the worker thread (the watchdog primitive)
GeoSpecPoolWorkerMessage (type) [lines 1162-1201] [1 members]
  GeoSpecPoolWorkerMessage.type (property)
GeoSpecRunner (type) [lines 1203-1233] [4 members] — Public GeoSpec runner lifecycle surface
  GeoSpecRunner.run (method) — Execute GeoSpec files and return a compact aggregate result
  GeoSpecRunner.on (method) — Subscribe to one lifecycle event type
  GeoSpecRunner.abort (method) — Request cooperative abort before the next file starts
  GeoSpecRunner.close (method) — Close the runner and release owned resources
GeoSpecRunnerEvent (type) [lines 1235-1267] [1 members] — Lifecycle event emitted by GeoSpec worker-style runners
  GeoSpecRunnerEvent.type (property)
GeoSpecRunnerFileResult (type) [lines 1269-1296] [5 members] — One GeoSpec test file executed by a worker-style runner
  GeoSpecRunnerFileResult.file (property) — Absolute or project-relative GeoSpec test file path supplied to the…
  GeoSpecRunnerFileResult.result (property) — Low-level module execution result for this file
  GeoSpecRunnerFileResult.durationMs (property) — Wall-clock cost of executing this file, in milliseconds
  GeoSpecRunnerFileResult.primaryLoadKey (property) — First deterministic model-load cache key observed in this file (affinity…
  GeoSpecRunnerFileResult.workerMemoryBytes (property) — Executing worker's isolate-resident memory at file completion, in bytes (memory-class…
GeoSpecRunnerOptions (type) [lines 1298-1325] [5 members] — Shared options for GeoSpec runner factories
  GeoSpecRunnerOptions.filesystem (property) — Filesystem containing the project and test modules
  GeoSpecRunnerOptions.nativeAssertions (property) — Protocol-3 assertion client shared by every authored assertion and model…
  GeoSpecRunnerOptions.nativeModelLoader (property) — Managed native model loader released after each settled run
  GeoSpecRunnerOptions.builtinModules (property) — Additional in-memory modules made available to the VM
  GeoSpecRunnerOptions.createModuleVm (property) — Module VM factory for every file
GeoSpecRunnerResult (type) [lines 1327-1374] [9 members] — Aggregate result returned by GeoSpec worker-style runners
  GeoSpecRunnerResult.success (property) — True when no files or tests failed and at least…
  GeoSpecRunnerResult.passed (property) — Number of non-skipped tests that passed
  GeoSpecRunnerResult.failed (property) — Number of file-level or test-level failures
  GeoSpecRunnerResult.selectedTests (property) — Number of collected tests after filters were applied
  GeoSpecRunnerResult.files (property) — Per-file module execution results
  GeoSpecRunnerResult.issues (property) — Run-level issues such as aborts or empty filter selections
  GeoSpecRunnerResult.durationMs (property) — Wall-clock cost of the whole run, in milliseconds
  GeoSpecRunnerResult.accounting (property) — Complete requested-file accounting
  GeoSpecRunnerResult.lineageStatus (property) — Coherence of the consumed source graphs, not a geometry verdict
GeoSpecRunnerRunOptions (type) [lines 1376-1410] [5 members] — Options accepted by a GeoSpec worker-style runner run
  GeoSpecRunnerRunOptions.files (property) — GeoSpec test files to execute
  GeoSpecRunnerRunOptions.testNamePattern (property) — JavaScript regular expression matched against full `suite > test` names
  GeoSpecRunnerRunOptions.testTimeout (property) — Timeout for each async test callback, in milliseconds
  GeoSpecRunnerRunOptions.workUnitLimit (property) — Engine work-unit budget for this run's claims, replacing the runner's…
  GeoSpecRunnerRunOptions.bail (property) — Stop after the first failing file

## Types (3) — `public-api-types-3.md`

GeoSpecPoolOptions (type) [lines 5-50] [8 members] — Everything the pool needs that differs between Node and the…
  GeoSpecPoolOptions.createWorker (property) — Spawn one worker running `startGeoSpecPoolWorkerHost`
  GeoSpecPoolOptions.initializeWorker (property) — Optional one-time worker initialization performed after its ready signal
  GeoSpecPoolOptions.gracefulShutdown (property) — Wait for each native worker's cleanup acknowledgement before terminating it
  GeoSpecPoolOptions.workers (property) — Worker count
  GeoSpecPoolOptions.timings (property) — Optional scheduling telemetry
  GeoSpecPoolOptions.shardTimeout (property) — Per-shard non-verdict watchdog, milliseconds
  GeoSpecPoolOptions.splitThreshold (property) — Split files whose recorded duration exceeds this
  GeoSpecPoolOptions.retainsAcrossRuns (property) — Whether each worker keeps what it admitted for later runs…
ShardTiming (type) [lines 52-64] [2 members] — One shard's recorded cost
  ShardTiming.durationMs (property) — Wall duration of the last recorded run, in milliseconds
  ShardTiming.peakRssBytes (property) — Highest RSS observed for this shard, in bytes
ShardTimings (type) [lines 66-92] [4 members] — A mutable timings table
  ShardTimings.record (method) — Merge one observation
  ShardTimings.read (method) — The hint for one shard, or `undefined` when unknown
  ShardTimings.longestFirst (method) — Every recorded shard, sorted by descending duration (longest-first)
  ShardTimings.save (method) — Persist the table
GeoSpecPoolWorkerHostOptions (type) [lines 94-141] [9 members] — Options accepted by {@link startGeoSpecPoolWorkerHost}
  GeoSpecPoolWorkerHostOptions.filesystem (property) — Filesystem containing the project and test modules
  GeoSpecPoolWorkerHostOptions.nativeAssertions (property) — Native assertion client shared with this worker's model admissions
  GeoSpecPoolWorkerHostOptions.nativeModelLoader (property) — Managed native admissions, released after every shard and collection pass…
  GeoSpecPoolWorkerHostOptions.builtinModules (property) — Additional in-memory modules made available to the VM
  GeoSpecPoolWorkerHostOptions.createModuleVm (property) — Module VM factory for every shard
  GeoSpecPoolWorkerHostOptions.postMessage (property) — Post a message to the pool host
  GeoSpecPoolWorkerHostOptions.onHostMessage (property) — Subscribe to pool-host messages
  GeoSpecPoolWorkerHostOptions.measureMemoryBytes (property) — Sample this worker's resident memory in bytes, as telemetry
  GeoSpecPoolWorkerHostOptions.onShutdown (property) — Release platform resources on shutdown, after the native subjects are…
GeoSpecAssertionClient (type) [lines 143-160] [2 members] — Runner-independent native assertion client
  GeoSpecAssertionClient.expectGeo (method) — Start a chain
  GeoSpecAssertionClient.query (method)
GeoSpecAssertionClientOptions (type) [lines 162-190] [6 members] — Flat construction options for a runner-independent native assertion client
  GeoSpecAssertionClientOptions.claimId (property)
  GeoSpecAssertionClientOptions.engine (property)
  GeoSpecAssertionClientOptions.evidenceProfile (property) — Success-evidence profile of every claim and query
  GeoSpecAssertionClientOptions.subjectSlot (property)
  GeoSpecAssertionClientOptions.workUnitLimit (property)
  GeoSpecAssertionClientOptions.maximumWorkUnitLimit (property) — The largest `workUnitLimit` a spec may author in `loadModel` or…
GeoSpecAssertionMatchers (type) [lines 192-197] [1 members] — Standalone native matcher chain, including core-owned negation
  GeoSpecAssertionMatchers.not (property)
GeoSpecAuthoringInvocation (type) [lines 199-219] [6 members] — One authored call shared by the collector and native assertion…
  GeoSpecAuthoringInvocation.arguments (property)
  GeoSpecAuthoringInvocation.expected (property)
  GeoSpecAuthoringInvocation.kind (property)
  GeoSpecAuthoringInvocation.matcher (property)
  GeoSpecAuthoringInvocation.polarity (property)
  GeoSpecAuthoringInvocation.subject (property)
GeoSpecMatcherMethods (type) [lines 221-224] — Matcher methods derived mechanically from the existing GeoSpec registry
GeoSpecQueryOptions (type) [lines 226-241] [4 members] — One positive-only ancillary query
  GeoSpecQueryOptions.capability (property)
  GeoSpecQueryOptions.claimId (property)
  GeoSpecQueryOptions.payload (property)
  GeoSpecQueryOptions.subject (property)
MinimumDistanceFact (type) [lines 243-272] [9 members] — Complete native minimum and ordered finite witnesses in canonical millimetres/Z-up
  MinimumDistanceFact.source (property)
  MinimumDistanceFact.assurance (property)
  MinimumDistanceFact.unit (property)
  MinimumDistanceFact.coordinateSystem (property)
  MinimumDistanceFact.subjectHash (property)
  MinimumDistanceFact.algorithmProfile (property)
  MinimumDistanceFact.occurrences (property)
  MinimumDistanceFact.distance (property)
  MinimumDistanceFact.points (property)
MinimumDistanceQuery (type) [lines 274-297] [3 members] — Complete AP242 minimum over two subject-bound occurrence paths
  MinimumDistanceQuery.capability (property)
  MinimumDistanceQuery.subject (property)
  MinimumDistanceQuery.payload (property)
MinimumDistanceResult (type) [lines 299-313] [1 members] — Geometry refusal and infrastructure interruption never masquerade as facts
  MinimumDistanceResult.status (property)
GeoSpecNativeQueryOptions (type) [lines 315-338] [9 members] — Flat native query transport options
  GeoSpecNativeQueryOptions.engine (property)
  GeoSpecNativeQueryOptions.claimId (property)
  GeoSpecNativeQueryOptions.evidenceProfile (property)
  GeoSpecNativeQueryOptions.subjectSlot (property)
  GeoSpecNativeQueryOptions.workUnitLimit (property)
  GeoSpecNativeQueryOptions.maximumWorkUnitLimit (property) — The host's largest authored budget, reported as `details.maximumLimit` on `MATCHER_TIMEOUT`
  GeoSpecNativeQueryOptions.subject (property)
  GeoSpecNativeQueryOptions.capability (property)
  GeoSpecNativeQueryOptions.payload (property)
GeoSpecQueryCapability (type) [lines 340-341] — Existing positive-only ancillary operations owned by the native core
GeoSpecPmiField (type) [lines 343-354] [3 members] — Explicit support state for one source-attributed inventory field
  GeoSpecPmiField.status (property)
  GeoSpecPmiField.value (property)
  GeoSpecPmiField.reason (property)
GeoSpecPmiRawEntity (type) [lines 356-367] [3 members] — Original Part21 entity ID and exact source argument tokens
  GeoSpecPmiRawEntity.sourceId (property)
  GeoSpecPmiRawEntity.kind (property)
  GeoSpecPmiRawEntity.arguments (property)
GeoSpecPmiNumber (type) [lines 369-389] [6 members] — Source-authored scalar and Rust-normalized reduced rational millimetres
  GeoSpecPmiNumber.sourceId (property)
  GeoSpecPmiNumber.authoredText (property)
  GeoSpecPmiNumber.unitId (property)
  GeoSpecPmiNumber.unitRecords (property)
  GeoSpecPmiNumber.millimetres (property)
  GeoSpecPmiNumber.name (property)
GeoSpecPmiFaceAssociation (type) [lines 391-405] [4 members] — A uniquely forward-transferred face
  GeoSpecPmiFaceAssociation.sourceFaceId (property)
  GeoSpecPmiFaceAssociation.occurrenceRoute (property)
  GeoSpecPmiFaceAssociation.occurrence (property)
  GeoSpecPmiFaceAssociation.publicFaceOrdinal (property)
GeoSpecPmiShapeReference (type) [lines 407-424] [5 members] — One ordered role reference, including incomplete source/transfer evidence
  GeoSpecPmiShapeReference.sourceAspectId (property)
  GeoSpecPmiShapeReference.sourceUsageIds (property)
  GeoSpecPmiShapeReference.sourceItemIds (property)
  GeoSpecPmiShapeReference.requestedRoute (property)
  GeoSpecPmiShapeReference.associations (property)
GeoSpecPmiLimits (type) [lines 426-437] [3 members] — Normalized authored limits
  GeoSpecPmiLimits.lowerMillimetres (property)
  GeoSpecPmiLimits.upperMillimetres (property)
  GeoSpecPmiLimits.basis (property)
GeoSpecPmiRecord (type) [lines 439-474] [11 members] — Source record preserving semantic/presentation separation and ordered roles
  GeoSpecPmiRecord.sourceId (property)
  GeoSpecPmiRecord.family (property)
  GeoSpecPmiRecord.channel (property)
  GeoSpecPmiRecord.kind (property)
  GeoSpecPmiRecord.name (property)
  GeoSpecPmiRecord.first (property)
  GeoSpecPmiRecord.second (property)
  GeoSpecPmiRecord.numbers (property)
  GeoSpecPmiRecord.limits (property)
  GeoSpecPmiRecord.interpretation (property)
  GeoSpecPmiRecord.raw (property)
GeoSpecPmiInventory (type) [lines 476-493] [5 members] — Positive-only inventory value inside the ordinary canonical query report
  GeoSpecPmiInventory.contract (property)
  GeoSpecPmiInventory.status (property)
  GeoSpecPmiInventory.fileSchema (property)
  GeoSpecPmiInventory.editionValidation (property)
  GeoSpecPmiInventory.records (property)
GeoSpecPmiQueryPayload (type) [lines 495-503] [2 members] — Strict inventory output limits
  GeoSpecPmiQueryPayload.maxRecords (property)
  GeoSpecPmiQueryPayload.maxOutputBytes (property)
GeoSpecPmiQueryValue (type) [lines 505-516] [3 members] — Complete inventory value
  GeoSpecPmiQueryValue.inventory (property)
  GeoSpecPmiQueryValue.subjectHash (property)
  GeoSpecPmiQueryValue.provenance (property)
GeoSpecCanonicalClaimReport (type) [lines 518-552] [10 members] — Full native assertion result with the exact core-owned bytes retained
  GeoSpecCanonicalClaimReport.canonicalClaim (property)
  GeoSpecCanonicalClaimReport.canonicalPlan (property)
  GeoSpecCanonicalClaimReport.canonicalResult (property)
  GeoSpecCanonicalClaimReport.claim (property)
  GeoSpecCanonicalClaimReport.claimId (property)
  GeoSpecCanonicalClaimReport.diagnostics (property) — Engine diagnostics
  GeoSpecCanonicalClaimReport.evidence (property)
  GeoSpecCanonicalClaimReport.polarity (property)
  GeoSpecCanonicalClaimReport.result (property)
  GeoSpecCanonicalClaimReport.status (property)
GeoSpecNativeClaimEvaluation (type) [lines 554-565] [3 members] — Exact core bytes of one claim evaluated in one engine…
  GeoSpecNativeClaimEvaluation.canonicalClaim (property)
  GeoSpecNativeClaimEvaluation.canonicalPlan (property)
  GeoSpecNativeClaimEvaluation.canonicalResult (property)
GeoSpecNativeEngine (type) [lines 567-577] [2 members] — Byte-only engine surface consumed by the runner-independent assertion client
  GeoSpecNativeEngine.evaluateClaim (method)
  GeoSpecNativeEngine.processRequest (method)
GeoSpecNativeEvidenceProfile (type) [lines 579-580] — Success-evidence profile a product selects for its plans (PERF-OUTPUT-01)
GeoSpecNativeSubject (type) [lines 582-590] [2 members] — Content-addressed subject accepted by a protocol-3 assertion plan
  GeoSpecNativeSubject.contentHash (property)
  GeoSpecNativeSubject.subjectHash (property)
GeoSpecFixedMatcherDescriptor (type) [lines 592-603] [3 members] — A canonical fixed-contract matcher
  GeoSpecFixedMatcherDescriptor.contract (property)
  GeoSpecFixedMatcherDescriptor.expected (property)
  GeoSpecFixedMatcherDescriptor.mode (property)
GeoSpecCliHost (type) [lines 605-705] [7 members] — The platform the CLI runs against
  GeoSpecCliHost.cwd (method) — Absolute project root for a relative `projectPath`
  GeoSpecCliHost.write (method)
  GeoSpecCliHost.discoveryFileSystem (method) — Recursive discovery filesystem
  GeoSpecCliHost.loadConfig (method) — Import the project's `geospec.config.*`, or the explicit one, for its…
  GeoSpecCliHost.watch (method) — Watch a project for `geospec run --watch` until the returned…
  GeoSpecCliHost.onStop (method) — Call `listener` once when the user stops a watch, such…
  GeoSpecCliHost.createRunner (method) — Build the runner the selected files execute on
GeoSpecCliWatcher (type) [lines 707-735] [4 members] — What `geospec run --watch` asks of a host's watcher
  GeoSpecCliWatcher.baseline (property) — Paths whose bytes the watcher records once it watches, so…
  GeoSpecCliWatcher.relevant (method) — Whether the watch reruns for a path
  GeoSpecCliWatcher.onChange (method) — A relevant path whose bytes differ from the last bytes…
  GeoSpecCliWatcher.onError (method) — The watcher failed and stopped
NodeGeoSpecCliHostOptions (type) [lines 737-769] [4 members] — Options for {@link createNodeGeoSpecCliHost}
  NodeGeoSpecCliHostOptions.reportStream (property) — Report sink used by embedding hosts and tests
  NodeGeoSpecCliHostOptions.exporterModules (property) — URLs of modules each pool worker imports for exporters that…
  NodeGeoSpecCliHostOptions.moduleVmModule (property) — URL of a module each pool worker imports for its…
  NodeGeoSpecCliHostOptions.isolation (property) — Where each worker runs (see `GeoSpecNodePoolOptions.isolation`)
GeoSpecVitestAdapter (type) [lines 771-780] [2 members] — Installed Vitest matcher map and lifecycle settlement hook
  GeoSpecVitestAdapter.matchers (property)
  GeoSpecVitestAdapter.flush (method)
GeoSpecVitestProject (type) [lines 782-804] [5 members] — The installed matchers plus the project's model loader
  GeoSpecVitestProject.matchers (property)
  GeoSpecVitestProject.flush (method)
  GeoSpecVitestProject.loadModel (property) — Load a model through the config's exporters
  GeoSpecVitestProject.configPath (property) — The config file that was read, when the project has…
  GeoSpecVitestProject.binding (property) — The build the created engine runs on
SetupGeoSpecVitestProjectOptions (type) [lines 806-852] [9 members] — Options for {@link setupGeoSpecVitestProject}
  SetupGeoSpecVitestProjectOptions.claimId (property)
  SetupGeoSpecVitestProjectOptions.evidenceProfile (property) — Success-evidence profile of every claim and query
  SetupGeoSpecVitestProjectOptions.subjectSlot (property)
  SetupGeoSpecVitestProjectOptions.workUnitLimit (property)
  SetupGeoSpecVitestProjectOptions.projectPath (property) — Absolute project root
  SetupGeoSpecVitestProjectOptions.configPath (property) — Explicit config file, resolved relative to the project unless absolute
  SetupGeoSpecVitestProjectOptions.overrides (property) — Fields that replace the config's whole, as a CLI flag…
  SetupGeoSpecVitestProjectOptions.engine (property) — A caller-owned engine, kept open after the file
  SetupGeoSpecVitestProjectOptions.persistentEngine (property) — Share one engine, with the project's facts store, across the…
ReadGeoSpecConfigOptions (type) [lines 854-874] [3 members] — Options for {@link readGeoSpecConfig}
  ReadGeoSpecConfigOptions.filesystem (property) — The project filesystem
  ReadGeoSpecConfigOptions.projectPath (property) — Rooted directory holding the config, in the filesystem's spelling
  ReadGeoSpecConfigOptions.createModuleVm (property) — Module VM factory
ReadGeoSpecConfigResult (type) [lines 876-894] [1 members] — Outcome of {@link readGeoSpecConfig}
  ReadGeoSpecConfigResult.status (property)
GeoSpecConfig (type) [lines 896-925] [6 members] — Trusted project configuration using existing discovery and runner options
  GeoSpecConfig.include (property)
  GeoSpecConfig.exclude (property)
  GeoSpecConfig.testNamePattern (property)
  GeoSpecConfig.testTimeout (property) — Positive finite milliseconds
  GeoSpecConfig.bail (property)
  GeoSpecConfig.exporters (property) — Model exporters for `loadModel({ file })`
GeoSpecConfigData (type) [lines 927-939] [5 members] — The config's data fields
  GeoSpecConfigData.include (property)
  GeoSpecConfigData.exclude (property)
  GeoSpecConfigData.testNamePattern (property)
  GeoSpecConfigData.testTimeout (property) — Positive finite milliseconds
  GeoSpecConfigData.bail (property)
LoadedGeoSpecConfig (type) [lines 941-962] [4 members] — Resolved file identity and validated configuration data
  LoadedGeoSpecConfig.configPath (property)
  LoadedGeoSpecConfig.options (property)
  LoadedGeoSpecConfig.sources (property) — Project-relative paths, `/`-separated, of every file this load imported from…
  LoadedGeoSpecConfig.sourceDigests (property) — The SHA-256, as hex, of each of `sources` as this…
LoadGeoSpecConfigOptions (type) [lines 964-979] [3 members] — Options for one trusted Node configuration load
  LoadGeoSpecConfigOptions.projectPath (property)
  LoadGeoSpecConfigOptions.configPath (property) — Explicit file, resolved relative to the project unless absolute
  LoadGeoSpecConfigOptions.overrides (property) — Own defined fields override configuration without deep merging

## Classes — `public-api-classes.md`

GeoSpecModelLoadError (class) [lines 5-13] [2 members] — Error thrown by {@link import ('./load-model.js').loadModel} when geometry cannot be…
  GeoSpecModelLoadError.diagnostics (property) — Structured diagnostics explaining why model loading failed
  GeoSpecModelLoadError.constructor (constructor)
GeoSpecAssertionError (class) [lines 15-22] [2 members] — Assertion error thrown by GeoSpec matchers when an expectation does…
  GeoSpecAssertionError.diagnostics (property)
  GeoSpecAssertionError.constructor (constructor)
