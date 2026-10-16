// eslint-disable-next-line import-x/no-extraneous-dependencies -- #wasm.js resolves to this package's own src/wasm.ts; includeInternal misclassifies the self-owned export as a dependency.
export { Engine, ProtocolError, canonicalize, initialize } from '#wasm.js';
// eslint-disable-next-line import-x/no-extraneous-dependencies -- #wasm.js resolves to this package's own src/wasm.ts; includeInternal misclassifies the self-owned export as a dependency.
export type { HostBytes, HostCapacityRefusal, HostEngine, HostEngineLimits, WasmInput } from '#wasm.js';
