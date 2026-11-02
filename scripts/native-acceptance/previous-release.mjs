import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { safeFilename, sha256 } from './lib.mjs';

const repo = process.env.GITHUB_REPOSITORY;
assert.match(repo ?? '', /^[\w.-]+\/[\w.-]+$/);
const [platform, outputArgument] = process.argv.slice(2);
const names = {
  windows: ['XY-Switch-1.0.0-windows-x64-setup.exe', 'XY-Switch-1.0.0-windows-x64.msi'],
  linux: ['XY-Switch-1.0.0-linux-x64.deb', 'XY-Switch-1.0.0-linux-x64.AppImage'],
  macos: ['XY-Switch-1.0.0-macos-universal.dmg'],
}[platform];
assert(names && outputArgument, 'Specify a desktop platform and a new output directory');
const output = resolve(outputArgument);
await mkdir(output, { recursive: false });
const headers = { Authorization: `Bearer ${process.env.GH_TOKEN}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' };
const base = `https://api.github.com/repos/${repo}`;
async function api(path) {
  const response = await fetch(base + path, { headers, signal: AbortSignal.timeout(120_000) });
  assert(response.ok, `Previous release API returned HTTP ${response.status}`);
  return response.json();
}
const release = await api('/releases/tags/v1.0.0');
assert.equal(release.tag_name, 'v1.0.0');
assert.equal(release.draft, false);
assert.equal(release.prerelease, false);
let tag = (await api('/git/ref/tags/v1.0.0')).object;
for (let depth = 0; tag.type === 'tag' && depth < 4; depth++) tag = (await api(`/git/tags/${tag.sha}`)).object;
assert.equal(tag.type, 'commit');
assert.match(tag.sha, /^[0-9a-f]{40}$/);
async function asset(name) {
  const matches = release.assets.filter(item => item.name === name);
  assert.equal(matches.length, 1, `Missing previous release asset ${name}`);
  const metadata = matches[0];
  const response = await fetch(`${base}/releases/assets/${metadata.id}`, { headers: { ...headers, Accept: 'application/octet-stream' }, redirect: 'manual', signal: AbortSignal.timeout(120_000) });
  let download = response;
  if ([301, 302, 303, 307, 308].includes(response.status)) {
    const location = new URL(response.headers.get('location'));
    assert.equal(location.protocol, 'https:');
    assert(location.hostname.endsWith('.githubusercontent.com') || location.hostname.endsWith('.github.com'), 'Unexpected release download host');
    download = await fetch(location, { signal: AbortSignal.timeout(180_000) });
  }
  assert(download.ok, `Previous asset download returned HTTP ${download.status}`);
  const data = Buffer.from(await download.arrayBuffer());
  assert.equal(data.length, metadata.size);
  if (metadata.digest) assert.equal('sha256:' + sha256(data), metadata.digest);
  return { metadata, data };
}
const sums = await asset('SHA256SUMS.txt');
const checksums = new Map(sums.data.toString('utf8').split(/\r?\n/).filter(Boolean).map(line => {
  const match = line.match(/^([0-9a-f]{64})\s+\*?(.+)$/i);
  assert(match, 'Malformed release checksum entry');
  return [safeFilename(match[2]), match[1].toLowerCase()];
}));
const files = [];
for (const name of names) {
  const { metadata, data } = await asset(name);
  assert.equal(sha256(data), checksums.get(name), `Release checksum mismatch for ${name}`);
  const path = join(output, name);
  await writeFile(path, data, { flag: 'wx' });
  files.push({ id: metadata.id, name, path, bytes: data.length, sha256: sha256(data), githubDigest: metadata.digest ?? null });
}
const report = { verified: true, repository: repo, tag: release.tag_name, version: '1.0.0', commit: tag.sha, releaseId: release.id, releaseUrl: release.html_url, checksumFileSha256: sha256(sums.data), platform, files };
await writeFile(join(output, 'previous-release.json'), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify(report));
