import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { resolveIsolatedDebugRuntime, assertIsolatedDebugAppPaths } from './isolated-debug.mjs';

async function fixture(t) {
  const workspace = await mkdtemp(path.join(tmpdir(), 'xy-debug-guard-'));
  t.after(() => rm(workspace, { recursive: true, force: true }));
  const root = path.join(workspace, 'out', 'native');
  await mkdir(path.join(root, 'data'), { recursive: true });
  await mkdir(path.join(root, 'home'), { recursive: true });
  await writeFile(path.join(root, 'data', 'xy-switch.db'), 'synthetic guard fixture');
  const runtime = await resolveIsolatedDebugRuntime(root, workspace);
  return { workspace, root, runtime, actual: { ...runtime, codexHome: path.join(runtime.homeDir, '.codex') } };
}

test('local native acceptance verifies every debug-only data and client path', async t => {
  const { runtime, actual } = await fixture(t);
  await assertIsolatedDebugAppPaths(runtime, actual);
});

test('local native acceptance rejects relative paths and paths outside workspace out', async t => {
  const { workspace } = await fixture(t);
  await assert.rejects(resolveIsolatedDebugRuntime('out/native', workspace), /absolute/);
  await assert.rejects(resolveIsolatedDebugRuntime(workspace, workspace), /child/);
  await assert.rejects(resolveIsolatedDebugRuntime(path.join(workspace, 'out'), workspace), /child/);
});

test('release builds or partial data overrides cannot pass the local debug guard', async t => {
  const { runtime, workspace, actual } = await fixture(t);
  for (const field of ['dataDir', 'homeDir', 'databasePath']) {
    await assert.rejects(assertIsolatedDebugAppPaths(runtime, { ...actual, [field]: workspace }), /isolated debug path/);
  }
  await assert.rejects(assertIsolatedDebugAppPaths(runtime, { ...actual, codexHome: workspace }), /isolated home/);
});
