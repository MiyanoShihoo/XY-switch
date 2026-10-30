import assert from 'node:assert/strict';
import test from 'node:test';
import { access, readFile } from 'node:fs/promises';
import { packageInput, requireDisposableRunner } from './native-acceptance/lib.mjs';

const root = new URL('../', import.meta.url);
const read = path => readFile(new URL(path, root), 'utf8');

test('public build commands retain only Windows, macOS and Linux targets', async () => {
  const { scripts } = JSON.parse(await read('package.json'));
  for (const platform of ['windows', 'linux', 'macos', 'macos:universal']) assert(scripts[`build:${platform}`]);
  assert(!Object.keys(scripts).some(name => /ios|android|mobile/i.test(name)));
  assert(!/package-android|smoke-android|select-ios|smoke-ios/.test(scripts['test:packaging']));
});

test('CI contains only desktop builds and the Windows native acceptance path', async () => {
  const workflow = await read('.github/workflows/build.yml');
  const jobs = [...workflow.slice(workflow.indexOf("\njobs:\n")).matchAll(/^  ([a-z][a-z-]*):$/gm)].map(match => match[1]);
  assert.deepEqual(jobs, ['frontend', 'website', 'desktop', 'macos', 'native-source', 'native-windows-sync']);
  assert(!/ios|android|simulator|native_target/i.test(workflow));
  for (const line of ['platform: windows', 'platform: linux', 'pnpm build:macos:universal']) assert(workflow.includes(line));
  await assert.rejects(access(new URL('.github/workflows/ios-device.yml', root)), { code: 'ENOENT' });
});

test('active package and workflow script references do not point at removed entry points', async () => {
  const inputs = [await read('package.json'), await read('.github/workflows/build.yml')];
  const paths = new Set(inputs.flatMap(text => [...text.matchAll(/scripts\/[a-zA-Z0-9_./-]+\.(?:mjs|ps1|sh)/g)].map(match => match[0])));
  assert(paths.size > 10);
  for (const path of paths) await access(new URL(path, root));
});

test('Windows acceptance does not require or accept a mobile artifact', async () => {
  const source = await read('scripts/native-acceptance/artifact.mjs');
  assert(source.includes("[['windows', 'xy-switch-windows-X64']]"));
  assert(!/ios|android|simulator/i.test(source));
  for (const platform of ['ios-arm64-simulator', 'android-universal', 'macos-universal']) {
    await assert.rejects(packageInput('.', platform, '1.2.0'), /Unsupported native acceptance package/);
  }
  assert.throws(() => requireDisposableRunner('darwin', { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted' }), /requires Windows/);
});

test('both website catalogs and visible platform cards advertise exactly three desktops', async () => {
  const html = await read('website/index.html');
  const cards = [...html.matchAll(/<h3 data-i18n="(windows|linux|macos|android|ios)">/g)].map(match => match[1]);
  assert.deepEqual(cards, ['windows', 'linux', 'macos']);
  const catalog = JSON.parse(html.match(/<script id="translations" type="application\/json">(.*?)<\/script>/s)[1]);
  assert.deepEqual(Object.keys(catalog).sort(), ['en', 'zh-CN']);
  for (const translations of Object.values(catalog)) {
    for (const platform of cards) assert(translations[platform] && translations[platform + 'Text']);
    for (const key of ['android', 'androidText', 'ios', 'iosText']) assert(!(key in translations));
  }
  for (const key of ['version', 'platformTitle', 'verification', 'scopeThree']) {
    const match = html.match(new RegExp(`<[^>]+data-i18n="${key}"[^>]*>([^<]*)<`));
    assert.equal(match?.[1], catalog.en[key], key);
  }
  assert(!html.includes('.platform:last-child{grid-column:span 2}'));
});

test('current setup guides no longer offer mobile build or signing instructions', async () => {
  for (const path of ['README.md', 'README.zh-CN.md', 'docs/packaging.md', 'docs/packaging-en.md', 'docs/user-guide-en.md', 'docs/user-guide-zh.md']) {
    const text = await read(path);
    assert(!/build:ios|build:android|IOS_CERTIFICATE|XY_ANDROID_KEYSTORE_PATH|^## (?:Android|iOS)/m.test(text), path);
    for (const platform of ['Windows', 'macOS', 'Linux']) assert(text.includes(platform), path);
  }
});


test('website acceptance uses a disposable isolated browser without weakening checks', async () => {
  const workflow = await read('.github/workflows/build.yml');
  const runner = await read('scripts/verify-website-headless.mjs');
  const verifier = await read('scripts/verify-website-ui.mjs');
  assert(workflow.includes('node scripts/verify-website-headless.mjs'));
  for (const required of ["process.env.RUNNER_ENVIRONMENT, 'github-hosted'", "process.platform, 'linux'", "'--remote-debugging-address=127.0.0.1'", "'--user-data-dir=' + profile", "mkdtemp(join(tmpdir()", "method: 'Browser.close'", "packagedApplicationExecuted: false"]) {
    assert(runner.includes(required), required);
  }
  assert(!/--no-sandbox|--ignore-certificate-errors|--disable-web-security/.test(runner));
  assert(verifier.includes('assert.equal(state.remoteResources.length,0'));
  assert(verifier.includes("'-state.json'"));
});
