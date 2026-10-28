import { ModuleVm, ModuleVmOptions } from "./types.mjs";
//#region src/vm/plain-module-vm.d.ts
/**
 * Create GeoSpec's default module VM: native `esbuild` over the project filesystem, resolving project files and
 * registered builtins only. A bare npm or URL import is refused with a `BUNDLER_FAILED` issue that names the
 * specifier. Outside Node every bundle is refused with `BUNDLER_FAILED` until a host injects a VM.
 *
 * @param options - The project filesystem.
 * @returns A module VM.
 * @public
 */
export declare const createPlainModuleVm: (options: ModuleVmOptions) => Promise<ModuleVm>;
//#endregion