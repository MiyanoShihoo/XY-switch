import assert from 'node:assert/strict';

const pause = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
const contextChanging = error => /Execution context was destroyed|Cannot find context with specified id|Cannot find default execution context/.test(error.message);

// Fixture resets require a new document, not a fragment-only route transition.
// Wait for its main-frame loader before inspecting DOM readiness: the old page
// can still report complete while Page.navigate or Page.reload is committing.
export async function navigateFreshDocument({ call, evaluate }, url, {
  timeout = 10000,
  now = Date.now,
  sleep = pause,
} = {}) {
  const expectedUrl = new URL(url).href;
  const deadline = now() + timeout;
  const before = (await call('Page.getFrameTree')).frameTree?.frame;
  assert(before?.id && typeof before.loaderId === 'string', 'Missing main-frame loader before navigation');

  const navigation = await call('Page.navigate', { url: expectedUrl });
  if (navigation.errorText) throw new Error('Renderer navigation failed: ' + navigation.errorText);
  assert(!navigation.isDownload, 'Renderer navigation must not download a file');
  const reloaded = !navigation.loaderId;
  if (reloaded) await call('Page.reload');

  const ready = 'document.readyState === "complete" && Boolean(document.querySelector("h1")) && location.href === ' + JSON.stringify(expectedUrl);
  while (now() < deadline) {
    const frame = (await call('Page.getFrameTree')).frameTree?.frame;
    if (frame?.loaderId && (frame.id !== before.id || frame.loaderId !== before.loaderId)) {
      try {
        if (await evaluate(ready)) return { url: expectedUrl, loaderId: frame.loaderId, reloaded };
      } catch (error) {
        if (!contextChanging(error)) throw error;
      }
    }
    await sleep(Math.min(60, Math.max(0, deadline - now())));
  }
  throw new Error('Timed out waiting for a fresh ready renderer document: ' + expectedUrl);
}
