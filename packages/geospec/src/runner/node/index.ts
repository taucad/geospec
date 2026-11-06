/* oxlint-disable no-barrel-files/no-barrel-files -- Public Node-only runner subpath. */
/** Node bindings of the runner: a project filesystem on the real disk, the engine selector and the worker pool. @module */

export { createNodeVmFileSystem, resolveUnderRoot } from '#runner/node/node-vm-filesystem.js';
export { createNodeGeoSpecEngine } from '#runner/node/engine.js';
export type { CreateNodeGeoSpecEngineOptions, GeoSpecEngineBinding, NodeGeoSpecEngine } from '#runner/node/engine.js';
export {
  defaultGeoSpecFactsStoreRoot,
  geoSpecFactsStoreVariable,
  resolveGeoSpecFactsStore,
} from '#runner/node/facts-store.js';
export type { GeoSpecFactsStore } from '#runner/node/facts-store.js';
export {
  createGeoSpecNodePoolRunner,
  createNodeChildProcessHandle,
  createNodeWorkerHandle,
} from '#runner/node/pool-runner.js';
export type {
  GeoSpecNodePoolChildOptionsMessage,
  GeoSpecNodePoolOptions,
  NodeChildProcessLike,
  NodeWorkerLike,
} from '#runner/node/pool-runner.js';
export type { GeoSpecExporterModule } from '#runner/node/pool-worker-entry.js';
export { defaultGeoSpecSubjectRetention } from '#runner/node/subject-retention.js';
export type { GeoSpecSubjectRetention } from '#runner/node/subject-retention.js';
