import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { appendFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { sha256 } from './lib.mjs';

const repo = process.env.GITHUB_REPOSITORY;
assert.match(repo ?? '', /^[\w.-]+\/[\w.-]+$/);
const headers = { Authorization: `Bearer ${process.env.GH_TOKEN}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' };
const base = `https://api.github.com/repos/${repo}`;
async function api(path) {
  const response = await fetch(base + path, { headers, signal: AbortSignal.timeout(120_000) });
  assert(response.ok, `GitHub artifact API failed: HTTP ${response.status}`);
  return response;
}
const mode = process.argv[2], output = resolve(process.argv[3]);
await mkdir(dirname(output), { recursive: true });
if (mode === 'source') {
  const id = process.env.NATIVE_SOURCE_RUN, commit = process.env.NATIVE_SOURCE_COMMIT;
  assert.match(id ?? '', /^\d+$/); assert.match(commit ?? '', /^[0-9a-f]{40}$/);
  const run = await (await api(`/actions/runs/${id}`)).json();
  assert.equal(run.head_sha, commit, 'Artifact source commit mismatch');
  assert.equal(run.conclusion, 'success');
  assert.equal(run.name, 'Build and verify');
  assert.equal(run.repository.full_name, repo);
  // An older binary is valid only when all production inputs still match the tested checkout.
  execFileSync('git', ['diff', '--exit-code', commit, 'HEAD', '--', 'src', 'src-tauri', 'public', 'index.html', 'vite.config.ts', 'pnpm-lock.yaml'], { stdio: 'pipe' });
  const oldPackage = JSON.parse(execFileSync('git', ['show', `${commit}:package.json`], { encoding: 'utf8' }));
  const currentPackage = JSON.parse(await readFile('package.json', 'utf8'));
  for (const key of ['version', 'dependencies', 'devDependencies']) assert.deepEqual(currentPackage[key], oldPackage[key], `Changed production package ${key}`);
  const artifacts = (await (await api(`/actions/runs/${id}/artifacts?per_page=100`)).json()).artifacts;
  const selected = {};
  for (const [platform, name] of [['windows', 'xy-switch-windows-X64'], ['ios', 'xy-switch-ios-simulator-ARM64']]) {
    const matches = artifacts.filter(artifact => artifact.name === name && !artifact.expired);
    assert.equal(matches.length, 1, `Missing ${name}`);
    const artifact = matches[0];
    assert.match(artifact.digest, /^sha256:[0-9a-f]{64}$/);
    selected[platform] = { id: artifact.id, name, digest: artifact.digest, size: artifact.size_in_bytes };
    await appendFile(process.env.GITHUB_OUTPUT, `${platform}_id=${artifact.id}\n${platform}_digest=${artifact.digest}\n`);
  }
  const report = { sourceCommit: commit, sourceRun: run.id, sourceAttempt: run.run_attempt, sourceUrl: run.html_url, harnessCommit: process.env.GITHUB_SHA, productionInputsUnchanged: true, artifacts: selected };
  await writeFile(output, JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report));
} else if (mode === 'download') {
  assert.match(process.env.NATIVE_ARTIFACT_ID ?? '', /^\d+$/);
  assert.match(process.env.NATIVE_ARTIFACT_DIGEST ?? '', /^sha256:[0-9a-f]{64}$/);
  const response = await api(`/actions/artifacts/${process.env.NATIVE_ARTIFACT_ID}/zip`);
  const data = Buffer.from(await response.arrayBuffer());
  assert.equal('sha256:' + sha256(data), process.env.NATIVE_ARTIFACT_DIGEST, 'Artifact ZIP differs from GitHub provenance');
  await writeFile(output, data);
  console.log(JSON.stringify({ artifactId: process.env.NATIVE_ARTIFACT_ID, bytes: data.length, digest: process.env.NATIVE_ARTIFACT_DIGEST, verified: true }));
} else throw new Error('Use source or download');
