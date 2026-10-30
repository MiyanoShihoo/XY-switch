import assert from 'node:assert/strict';
import test from 'node:test';
import { Script } from 'node:vm';
import { navigateFreshDocument } from './document-navigation.mjs';

const target = 'http://127.0.0.1:1431/#diagnostics/qa-p2';
const oldFrame = { id: 'main', loaderId: 'old-document' };
const newFrame = { id: 'main', loaderId: 'new-document' };
function harness({ frames = [oldFrame, newFrame], navigation = { frameId: 'main', loaderId: 'new-document' }, readiness = [true] } = {}) {
  const calls = [], evaluations = [];
  let time = 0, frameIndex = 0, readyIndex = 0, currentFrame;
  const transport = {
    async call(method, params = {}) {
      calls.push({ method, params });
      if (method === 'Page.getFrameTree') {
        currentFrame = frames[Math.min(frameIndex++, frames.length - 1)];
        return { frameTree: { frame: currentFrame } };
      }
      if (method === 'Page.navigate') {
        if (navigation instanceof Error) throw navigation;
        return navigation;
      }
      assert.equal(method, 'Page.reload');
      return {};
    },
    async evaluate(expression) {
      new Script(expression);
      evaluations.push({ expression, frame: currentFrame });
      const value = readiness[Math.min(readyIndex++, readiness.length - 1)];
      if (value instanceof Error) throw value;
      return typeof value === 'function' ? value(expression) : value;
    },
  };
  const options = { timeout: 240, now: () => time, sleep: async milliseconds => { time += milliseconds; } };
  return { transport, options, calls, evaluations };
}
const navigate = h => navigateFreshDocument(h.transport, target, h.options);
const reloads = h => h.calls.filter(call => call.method === 'Page.reload');

test('cross-document navigation waits for the new loader without an extra reload', async () => {
  const h = harness();
  assert.deepEqual(await navigate(h), { url: target, loaderId: 'new-document', reloaded: false });
  assert.equal(reloads(h).length, 0);
  assert.deepEqual(h.calls.find(call => call.method === 'Page.navigate').params, { url: target });
});
test('fragment-only navigation reloads once and cannot reuse the old fixture', async () => {
  const h = harness({ navigation: { frameId: 'main' }, frames: [oldFrame, oldFrame, newFrame] });
  assert.equal((await navigate(h)).reloaded, true);
  assert.equal(reloads(h).length, 1);
  assert.equal(h.evaluations.length, 1);
  assert.equal(h.evaluations[0].frame.loaderId, 'new-document');
});
test('same-URL navigation without a loader also requires a new document', async () => {
  const h = harness({ navigation: { frameId: 'main' } });
  const result = await navigateFreshDocument(h.transport, 'http://127.0.0.1:1431/', h.options);
  assert.equal(result.reloaded, true);
  assert.equal(reloads(h).length, 1);
});
test('an already complete old document is never used as readiness evidence', async () => {
  const h = harness({ frames: [oldFrame, oldFrame, oldFrame, newFrame] });
  await navigate(h);
  assert.equal(h.evaluations.length, 1);
  assert.equal(h.evaluations[0].frame.loaderId, 'new-document');
});
test('new-loader readiness is polled until the renderer is complete', async () => {
  const h = harness({ readiness: [false, false, true] });
  await navigate(h);
  assert.equal(h.evaluations.length, 3);
  assert.equal(reloads(h).length, 0);
});
test('DOM readiness must match the exact target URL including its route', async () => {
  const completeAt = href => expression => new Script(expression).runInNewContext({
    document: { readyState: 'complete', querySelector: () => ({}) }, location: { href },
  });
  const h = harness({ readiness: [completeAt('http://127.0.0.1:1431/'), completeAt(target)] });
  await navigate(h);
  assert.equal(h.evaluations.length, 2);
});
test('a replaced main frame is accepted only with a fresh nonempty loader', async () => {
  const h = harness({ frames: [oldFrame, { id: 'replacement-main', loaderId: 'replacement-document' }] });
  assert.equal((await navigate(h)).loaderId, 'replacement-document');
});
test('waiting for an unchanged loader is bounded and does not inspect stale DOM', async () => {
  const h = harness({ navigation: { frameId: 'main' }, frames: [oldFrame] });
  await assert.rejects(navigate(h), /Timed out waiting for a fresh ready renderer document/);
  assert.equal(reloads(h).length, 1);
  assert.equal(h.evaluations.length, 0);
});
test('waiting for an incomplete fresh renderer is also bounded', async () => {
  const h = harness({ readiness: [false] });
  await assert.rejects(navigate(h), /Timed out waiting for a fresh ready renderer document/);
  assert.equal(h.evaluations.length, 4);
});
for (const message of ['Execution context was destroyed.', 'Cannot find context with specified id', 'Cannot find default execution context']) {
  test('commit-time context transition is retried: ' + message, async () => {
    const h = harness({ readiness: [new Error(message), true] });
    await navigate(h);
    assert.equal(h.evaluations.length, 2);
  });
}
test('unrelated browser evaluation failures remain failures', async () => {
  const h = harness({ readiness: [new Error('Unexpected renderer exception')] });
  await assert.rejects(navigate(h), /Unexpected renderer exception/);
  assert.equal(h.evaluations.length, 1);
});
test('navigation errors are not masked by reloading', async () => {
  const h = harness({ navigation: { frameId: 'main', errorText: 'net::ERR_CONNECTION_REFUSED' } });
  await assert.rejects(navigate(h), /net::ERR_CONNECTION_REFUSED/);
  assert.equal(reloads(h).length, 0);
});
test('download navigations fail instead of being treated as a fixture reset', async () => {
  const h = harness({ navigation: { frameId: 'main', isDownload: true } });
  await assert.rejects(navigate(h), /must not download a file/);
  assert.equal(reloads(h).length, 0);
});
test('CDP transport errors are preserved', async () => {
  const h = harness({ navigation: new Error('CDP connection closed') });
  await assert.rejects(navigate(h), /CDP connection closed/);
});
test('missing initial loader metadata fails before navigation', async () => {
  const h = harness({ frames: [{ id: 'main' }] });
  await assert.rejects(navigate(h), /Missing main-frame loader/);
  assert.equal(h.calls.some(call => call.method === 'Page.navigate'), false);
});
