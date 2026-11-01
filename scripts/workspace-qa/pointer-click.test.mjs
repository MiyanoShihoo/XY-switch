import assert from 'node:assert/strict';
import test from 'node:test';
import { runInNewContext } from 'node:vm';
import { dispatchElementClick } from './pointer-click.mjs';

function harness({ disabled = false, missing = false, obscured = false, outside = false, hidden = false, activationError = null } = {}) {
  let frameCount = 0;
  let left = 600;
  const events = [], commands = [];
  let foreground = !hidden;
  const descendant = {};
  const element = {
    disabled, isConnected: true,
    scrollIntoView: () => { left = outside ? -100 : 40; },
    getBoundingClientRect: () => ({ x: left, y: 10, width: 40, height: 30 }),
    contains: hit => hit === descendant,
  };
  const globals = {
    innerWidth: 390, innerHeight: 844,
    requestAnimationFrame: callback => {
      if (!foreground) return;
      frameCount++;
      if (frameCount === 1) left = 710;
      callback();
    },
    document: {
      querySelector: () => missing ? null : element,
      elementFromPoint: (x, y) => !obscured && x === left + 20 && y === 25 ? descendant : {},
    },
  };
  return {
    events, commands,
    frameCount: () => frameCount,
    evaluate: async expression => JSON.parse(JSON.stringify(await runInNewContext(expression, globals))),
    call: async (method, params) => {
      commands.push(method);
      if (method === 'Page.bringToFront') {
        if (activationError) throw activationError;
        foreground = true;
      } else { events.push({ method, ...params }); }
    },
  };
}

const target = 'document.querySelector(".xy-shell-nav-item")';

test('pointer input uses the target revealed after a pending compact layout resize', async () => {
  const browser = harness();
  await dispatchElementClick(browser, target);
  assert(browser.frameCount() >= 2);
  assert.deepEqual(browser.events, [
    { method: 'Input.dispatchMouseEvent', type: 'mousePressed', button: 'left', clickCount: 1, x: 60, y: 25 },
    { method: 'Input.dispatchMouseEvent', type: 'mouseReleased', button: 'left', clickCount: 1, x: 60, y: 25 },
  ]);
});

test('an unrelated control covering the target is never clicked', async () => {
  const browser = harness({ obscured: true });
  await assert.rejects(dispatchElementClick(browser, target), /Click target is obscured/);
  assert.deepEqual(browser.events, []);
});

test('offscreen coordinates cannot be sent as pointer input', async () => {
  const browser = harness({ outside: true });
  await assert.rejects(dispatchElementClick(browser, target), /outside the viewport/);
  assert.deepEqual(browser.events, []);
});

for (const [name, options, error] of [
  ['missing', { missing: true }, /Missing click target/],
  ['disabled', { disabled: true }, /Disabled click target/],
]) {
  test(name + ' targets cannot receive pointer input', async () => {
    const browser = harness(options);
    await assert.rejects(dispatchElementClick(browser, target), error);
    assert.deepEqual(browser.events, []);
  });
}

test('a background page is activated before awaiting its otherwise suspended animation frames', { timeout: 1000 }, async () => {
  const browser = harness({ hidden: true });
  await dispatchElementClick(browser, target);
  assert(browser.frameCount() >= 2);
  assert.equal(browser.commands[0], 'Page.bringToFront');
  assert.equal(browser.events.length, 2);
});

test('page activation failures preserve the error and send no pointer input', async () => {
  const activationError = new Error('Target closed before activation');
  const browser = harness({ hidden: true, activationError });
  await assert.rejects(dispatchElementClick(browser, target), error => error === activationError);
  assert.equal(browser.frameCount(), 0);
  assert.deepEqual(browser.events, []);
});
