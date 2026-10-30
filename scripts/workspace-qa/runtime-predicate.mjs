const pause = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));

// Readiness checks may return DOM nodes with circular React references. Convert
// the awaited result in the browser before CDP attempts return-by-value encoding.
export const predicateExpression = expression => 'Promise.resolve((' + expression + ')).then(Boolean)';

export async function waitForPredicate(evaluate, expression, {
  timeout = 10000,
  now = Date.now,
  sleep = pause,
} = {}) {
  const deadline = now() + timeout;
  const predicate = predicateExpression(expression);
  while (now() < deadline) {
    if (await evaluate(predicate)) return;
    await sleep(Math.min(60, Math.max(0, deadline - now())));
  }
  throw new Error('Timed out: ' + expression);
}
