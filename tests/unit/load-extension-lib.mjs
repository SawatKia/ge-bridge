import fs from 'node:fs';
import vm from 'node:vm';
import { webcrypto } from 'node:crypto';
import { fileURLToPath } from 'node:url';

export const LIB_PATH = fileURLToPath(new URL('../../extension/lib/gemini.js', import.meta.url));

export function newContext(overrides = {}) {
  const ctx = vm.createContext({
    crypto: webcrypto,
    TextEncoder,
    TextDecoder,
    URL,
    atob,
    btoa,
    AbortSignal,
    fetch: async () => { throw new Error('fetch stub: no network in unit tests'); },
    ...overrides
  });
  ctx.globalThis = ctx;
  return ctx;
}

// Evaluates the classic script in `ctx` (like a content script / importScripts) and returns GeBridgeLib.
export function loadLib(ctx = newContext()) {
  vm.runInContext(fs.readFileSync(LIB_PATH, 'utf8'), ctx, { filename: LIB_PATH });
  return vm.runInContext('GeBridgeLib', ctx);
}
