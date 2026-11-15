import assert from 'node:assert/strict';
import { realpath } from 'node:fs/promises';
import path from 'node:path';

function samePath(left, right) {
  return process.platform === 'win32' ? left.toLowerCase() === right.toLowerCase() : left === right;
}

export async function resolveIsolatedDebugRuntime(runtimeRoot, workspaceRoot = process.cwd()) {
  assert.equal(typeof runtimeRoot, 'string', 'An explicit isolated runtime is required');
  assert(path.isAbsolute(runtimeRoot), 'The isolated runtime path must be absolute');
  const outputRoot = await realpath(path.join(workspaceRoot, 'out'));
  const root = await realpath(runtimeRoot);
  const relative = path.relative(outputRoot, root);
  assert(relative && !relative.startsWith('..') && !path.isAbsolute(relative),
    'The isolated runtime must be a child of this workspace out directory');
  const dataDir = await realpath(path.join(root, 'data'));
  const homeDir = await realpath(path.join(root, 'home'));
  assert(samePath(dataDir, path.join(root, 'data')), 'The data directory must not redirect outside the runtime');
  assert(samePath(homeDir, path.join(root, 'home')), 'The home directory must not redirect outside the runtime');
  return { root, dataDir, homeDir, databasePath: path.join(dataDir, 'xy-switch.db') };
}

export async function assertIsolatedDebugAppPaths(runtime, actual) {
  for (const field of ['dataDir', 'homeDir', 'databasePath']) {
    assert.equal(typeof actual?.[field], 'string', `Native app_paths did not return ${field}`);
    assert(samePath(await realpath(actual[field]), runtime[field]),
      `Native ${field} is not the requested isolated debug path; refusing UI operations`);
  }
  const codexHome = path.resolve(actual.codexHome);
  assert(samePath(codexHome, path.join(runtime.homeDir, '.codex')),
    'Native client configuration must stay inside the isolated home');
}
