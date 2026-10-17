import { JSONValue } from "../stubs/taucad_runtime_types.d.mts";
//#region src/engine/protocol.d.ts
/** Runtime JSON-value guard used at every protocol trust boundary. @public */
declare const isGeoSpecJsonValue: (value: unknown, ancestors?: Set<WeakKey>) => value is JSONValue;
/** Reject a non-wire value rather than letting JSON.stringify erase it. @public */
declare const assertGeoSpecJsonValue: (value: unknown) => asserts value is JSONValue;
/** Encode client-owned canonical claim bytes. @public */
declare const encodeGeoSpecCanonicalJson: (value: JSONValue) => Uint8Array<ArrayBuffer>;
/** Parse and validate canonical bytes without re-canonicalizing them. @public */
declare const decodeGeoSpecCanonicalJson: (bytes: Uint8Array<ArrayBuffer>) => JSONValue;
/**
 * Convert an authoring value to protocol JSON, including the two explicit
 * bindings the TypeScript client needs: regex data and opaque subject refs.
 * Unsupported live objects fail before a request reaches the engine.
 *
 * @public
 */
declare const toGeoSpecProtocolJson: (value: unknown, ancestors?: Set<WeakKey>) => JSONValue;
//#endregion
export { assertGeoSpecJsonValue, decodeGeoSpecCanonicalJson, encodeGeoSpecCanonicalJson, isGeoSpecJsonValue, toGeoSpecProtocolJson };