import assert from 'node:assert/strict';
import test from 'node:test';
import { runInNewContext, Script } from 'node:vm';
import { predicateExpression, waitForPredicate } from './runtime-predicate.mjs';

function harness(globals = {}, options = {}) {
  let time = 0;
  const evaluations = [], sleeps = [];
  const evaluate = async expression => {
    evaluations.push(expression);
    const result = await runInNewContext(expression, globals);
    // Model CDP's returnByValue boundary, not just JavaScript truthiness.
    return JSON.parse(JSON.stringify(result));
  };
  return {
    evaluations, sleeps,
    wait: expression => waitForPredicate(evaluate, expression, {
      timeout: 125, now: () => time,
      sleep: async milliseconds => { sleeps.push(milliseconds); time += milliseconds; },
      ...options,
    }),
  };
}

test('DOM readiness serializes a boolean rather than a circular React-owned node', async () => {
  const node = { tagName: 'DIV' };
  node.__reactFiber = { stateNode: node };
  const h = harness({ document: { querySelector: () => node } });
  await h.wait('document.querySelector(".provider-row[data-dragging=true]")');
  assert.equal(h.evaluations.length, 1);
  assert.deepEqual(h.sleeps, []);
  assert.equal(await runInNewContext(h.evaluations[0], { document: { querySelector: () => node } }), true);
});

test('a missing node is polled until the DOM readiness condition is true', async () => {
  let queries = 0;
  const h = harness({ document: { querySelector: () => ++queries < 3 ? null : { ready: true } } });
  await h.wait('document.querySelector("h1")');
  assert.equal(queries, 3);
  assert.deepEqual(h.sleeps, [60, 60]);
});

for (const expression of ['false', 'null', 'undefined', '0', '""']) {
  test('falsey readiness remains false: ' + expression, async () => {
    const h = harness();
    await assert.rejects(h.wait(expression), error => error.message === 'Timed out: ' + expression);
    assert.equal(h.evaluations.length, 3);
    assert.deepEqual(h.sleeps, [60, 60, 5]);
  });
}

test('truthy primitives and serializable collections retain readiness semantics', async () => {
  for (const expression of ['true', '1', '"ready"', '({ ready: true })', '[]']) {
    const h = harness();
    await h.wait(expression);
    assert.equal(h.evaluations.length, 1);
    assert.deepEqual(h.sleeps, []);
  }
});

test('a promise resolving false is awaited rather than accepted as a truthy object', async () => {
  const h = harness();
  await assert.rejects(h.wait('Promise.resolve(false)'), /Timed out/);
  assert.equal(h.evaluations.length, 3);
});

test('a promise resolving a circular node becomes a serializable boolean', async () => {
  const node = {};
  node.self = node;
  const h = harness({ node });
  await h.wait('Promise.resolve(node)');
  assert.equal(h.evaluations.length, 1);
});

test('a rejected predicate promise preserves its failure without retrying', async () => {
  const h = harness();
  await assert.rejects(h.wait('Promise.reject(new Error("QA predicate denied"))'), /QA predicate denied/);
  assert.equal(h.evaluations.length, 1);
  assert.deepEqual(h.sleeps, []);
});

test('syntax and runtime failures are not swallowed as readiness polling', async () => {
  for (const expression of ['(', 'missingPredicate()']) {
    const h = harness();
    await assert.rejects(h.wait(expression));
    assert.equal(h.evaluations.length, 1);
    assert.deepEqual(h.sleeps, []);
  }
});

test('CDP evaluation failures are preserved', async () => {
  const failure = new Error('CDP disconnected');
  await assert.rejects(waitForPredicate(async () => { throw failure; }, 'true'), error => error === failure);
});

test('a zero readiness deadline does not execute the expression', async () => {
  const h = harness({}, { timeout: 0 });
  await assert.rejects(h.wait('true'), /Timed out: true/);
  assert.deepEqual(h.evaluations, []);
  assert.deepEqual(h.sleeps, []);
});

test('wrapped conditional and multiline expressions remain valid JavaScript', () => {
  for (const expression of ['true ? { ready: true } : null', '(() => {\nreturn false;\n})()', 'false, true']) {
    assert.doesNotThrow(() => new Script(predicateExpression(expression)));
  }
});
