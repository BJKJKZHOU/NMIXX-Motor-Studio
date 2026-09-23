// Isolated component command tests, not a browser/WebView mount test.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const vm = require('node:vm');

function harness(operation) {
  const source = readFileSync(join(__dirname, '../src/connection/ConnectionPage.svelte'), 'utf8');
  const match = source.match(/  async function disconnect\(\) \{[\s\S]*?\n  \}/);
  assert.ok(match, 'disconnect command exists');
  const errors = [];
  let disconnected = 0;
  let calls = 0;
  const context = vm.createContext({
    disconnectDevice: () => { calls++; return operation(); },
    onDisconnected: () => { disconnected++; },
    onError: (error) => errors.push(String(error)),
  });
  vm.runInContext('let busy = false;\n' + match[0], context);
  return {
    disconnect: () => vm.runInContext('disconnect()', context),
    busy: () => vm.runInContext('busy', context),
    disconnected: () => disconnected,
    calls: () => calls,
    errors,
  };
}

test('successful backend close clears the view once', async () => {
  const h = harness(() => Promise.resolve());
  await h.disconnect();
  assert.equal(h.disconnected(), 1);
  assert.equal(h.busy(), false);
  assert.deepEqual(h.errors, []);
});
test('failed backend close keeps the connected view and its safety controls', async () => {
  const h = harness(() => Promise.reject(new Error('motor is still RUN')));
  await h.disconnect();
  assert.equal(h.disconnected(), 0);
  assert.equal(h.busy(), false);
  assert.deepEqual(h.errors, ['Error: motor is still RUN']);
});
test('an in-flight disconnect cannot be submitted twice', async () => {
  let resolve;
  const operation = new Promise((done) => { resolve = done; });
  const h = harness(() => operation);
  const pending = h.disconnect();
  assert.equal(h.busy(), true);
  await h.disconnect();
  assert.equal(h.calls(), 1);
  assert.equal(h.disconnected(), 0);
  resolve();
  await pending;
  assert.equal(h.disconnected(), 1);
  assert.equal(h.busy(), false);
});
