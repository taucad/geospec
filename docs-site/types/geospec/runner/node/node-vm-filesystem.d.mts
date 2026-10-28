import { VmFileSystem } from "../../vm/types.mjs";
//#region src/runner/node/node-vm-filesystem.d.ts
/**
 * Resolve one VM path to a real path under the root.
 *
 * @param root - The project root.
 * @param path - A canonical path rooted by the supplied project capability.
 * @returns The absolute host path.
 * @throws Error when the path escapes the root.
 * @public
 */
export declare const resolveUnderRoot: (root: string, path: string) => string;
/**
 * Create a Node `VmFileSystem` rooted at `root`.
 *
 * @param root - Absolute project root path.
 * @returns A VM filesystem confined to `root`.
 * @public
 */
export declare const createNodeVmFileSystem: (root: string) => VmFileSystem;
//#endregion