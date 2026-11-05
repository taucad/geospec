// Run the solidcore harness compiled for wasm32-wasip1 under Node.js WASI.
// Usage: node wasi-run.mjs <harness.wasm> [harness args...] <file.step>
// Compiles the module, then runs it once; prints the harness JSON line plus
// compile time so module compilation can be separated from the run.
import { readFileSync } from 'node:fs';
import { WASI } from 'node:wasi';
import { performance } from 'node:perf_hooks';
const [wasmPath, ...args] = process.argv.slice(2);
const t0 = performance.now();
const module = await WebAssembly.compile(readFileSync(wasmPath));
const t1 = performance.now();
const wasi = new WASI({ version: 'preview1', args: ['harness', ...args], preopens: { '/': '/' }, returnOnExit: true });
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
wasi.start(instance);
process.stderr.write(JSON.stringify({ compileMs: t1 - t0, runMs: performance.now() - t1 }) + '\n');
