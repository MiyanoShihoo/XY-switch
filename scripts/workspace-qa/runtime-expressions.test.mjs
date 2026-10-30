import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { Script } from 'node:vm';
import ts from 'typescript';
import { predicateExpression } from './runtime-predicate.mjs';

// Compile static browser expressions without executing them or launching a browser.
// Node --check alone cannot see syntax errors inside Runtime.evaluate strings.
for (const name of ['navigation', 'workspace', 'i18n', 'sessions', 'governance', 'sync']) {
  const file = {
    navigation: 'verify-workspace-navigation-ui.mjs',
    workspace: 'verify-workspace-ui.mjs',
    i18n: 'verify-i18n-ui.mjs',
    sessions: 'verify-session-favorites-ui.mjs',
    governance: 'verify-governance-ui.mjs',
    sync: 'verify-encrypted-sync-ui.mjs',
  }[name];
  test(name + ': static browser expressions have valid JavaScript syntax', async () => {
    const source = await readFile(new URL('../' + file, import.meta.url), 'utf8');
    const parsed = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
    const expressions = [];
    function visit(node) {
      if (ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression) &&
          ['evaluate', 'waitFor'].includes(node.expression.name.text) &&
          ts.isStringLiteralLike(node.arguments[0])) {
        const expression = node.arguments[0];
        expressions.push({ value: expression.text, predicate: node.expression.name.text === 'waitFor', line: parsed.getLineAndCharacterOfPosition(expression.getStart()).line + 1 });
      }
      ts.forEachChild(node, visit);
    }
    visit(parsed);
    assert(expressions.length > 0, file + ' must contain browser expressions to validate');
    for (const { value, predicate, line } of expressions) {
      assert.doesNotThrow(() => new Script(predicate ? predicateExpression(value) : value), file + ':' + line + ' contains an invalid browser expression');
    }
  });
}
