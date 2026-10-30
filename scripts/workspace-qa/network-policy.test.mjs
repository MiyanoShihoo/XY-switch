import assert from 'node:assert/strict';
import test from 'node:test';
import { isLocalRendererRequest } from './cdp.mjs';

const base = 'http://127.0.0.1:1431';
const allowed = ['http://127.0.0.1:1431/', 'http://127.0.0.1:1431/src/App.tsx', 'ws://127.0.0.1:1431/?token=fixture', 'data:image/png;base64,AA==', 'blob:http://127.0.0.1:1431/fixture', 'about:blank'];
const denied = ['http://127.0.0.1:1432/', 'http://localhost:1431/', 'http://[::1]:1431/', 'https://127.0.0.1:1431/', 'wss://127.0.0.1:1431/', 'ws://127.0.0.1:9000/', 'https://api.example.test/v1', 'chrome-extension://fixture/inject.js', 'file:///tmp/fixture', 'not a URL'];
for (const value of allowed) test('allow exact renderer request: ' + value, () => assert.equal(isLocalRendererRequest(value, base), true));
for (const value of denied) test('deny other origin: ' + value, () => assert.equal(isLocalRendererRequest(value, base), false));
test('HTTPS loopback requires the matching secure WebSocket origin', () => {
  assert.equal(isLocalRendererRequest('wss://localhost:4443/', 'https://localhost:4443'), true);
  assert.equal(isLocalRendererRequest('ws://localhost:4443/', 'https://localhost:4443'), false);
});
test('public renderer bases are never accepted', () => {
  assert.equal(isLocalRendererRequest('https://example.test/app.js', 'https://example.test'), false);
  assert.equal(isLocalRendererRequest('about:blank', 'https://example.test'), false);
});
test('renderer bases must use HTTP(S), not arbitrary loopback protocols', () => {
  for (const base of ['ftp://127.0.0.1:1431/', 'ws://localhost:1431/', 'file:///tmp/app.html']) {
    assert.equal(isLocalRendererRequest(base, base), false);
  }
});
