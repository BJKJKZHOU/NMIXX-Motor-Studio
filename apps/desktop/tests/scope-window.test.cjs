// Focused tests execute the actual component script with transport/lifecycle mocks.
// They do not compile Svelte templates or replace test:startup / svelte-check.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const vm = require('node:vm');
const ts = require('typescript');

function harness() {
  const path = join(__dirname, '../src/analysis/scope/ScopeEchartsPage.svelte');
  const source = readFileSync(path, 'utf8').match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
  const lifecycle = { mount: undefined, destroy: undefined };
  const timers = new Map();
  const calls = [];
  const errors = [];
  let nextTimer = 0;
  let read = async () => ({ state: 'LIVE', recordedSeconds: 10, series: [], lostFrames: 0 });
  const api = {
    configureScope: async () => {}, startScope: async () => {}, stopScope: async () => {},
    readScopeSnapshot: (...args) => { calls.push(args); return read(...args); },
  };
  const context = vm.createContext({
    exports: {},
    require(name) {
      if (name === 'svelte') return { onMount: (cb) => lifecycle.mount = cb, onDestroy: (cb) => lifecycle.destroy = cb };
      if (name === './api') return api;
      if (name === '../../refreshScheduler') return { subscribeRefresh: () => () => {} };
      throw new Error(`Unexpected runtime import: ${name}`);
    },
    setTimeout(cb) { const id = ++nextTimer; timers.set(id, cb); return id; },
    clearTimeout(id) { timers.delete(id); },
    // These are normally declared by Svelte's legacy reactive transformation.
    channels: [], visibleChannels: [], running: false, $scopeOperation: undefined,
  });
  const output = ts.transpileModule(source, { compilerOptions: {
    target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS,
  } }).outputText;
  vm.runInContext(output, context);
  vm.runInContext('exports.onError = (error) => __errors.push(error);', Object.assign(context, { __errors: errors }));
  function evaluate(code) { return vm.runInContext(code, context); }
  function configure() {
    evaluate('exports.connection = {channels: []}; configured = true; snapshot = {recordedSeconds:10};');
  }
  async function flushTimer() {
    const entry = timers.entries().next().value;
    if (!entry) return;
    const [id, cb] = entry; timers.delete(id); cb();
    await new Promise(setImmediate);
  }
  return { evaluate, configure, calls, errors, lifecycle, timers, flushTimer, setRead(fn) { read = fn; } };
}

test('all four Scope callbacks exist before any connection', () => {
  const h = harness();
  for (const name of ['navigateToRange', 'returnToLatest', 'refreshSnapshot', 'scheduleViewRefresh']) {
    assert.equal(h.evaluate(`typeof ${name}`), 'function');
  }
});
test('disconnected Scope does not request a snapshot', async () => {
  const h = harness();
  await h.evaluate('refreshSnapshot(true)');
  h.evaluate('returnToLatest()');
  await h.flushTimer();
  assert.equal(h.calls.length, 0);
});
test('history navigation reads the requested window and offset', async () => {
  const h = harness(); h.configure();
  h.evaluate('navigateToRange([-4, -2])');
  await h.flushTimer();
  assert.deepEqual(h.calls[0], [2, 2, 2500]);
  assert.equal(h.evaluate('followingLatest'), false);
});
test('Latest preserves the span and requests zero offset', async () => {
  const h = harness(); h.configure();
  h.evaluate('navigateToRange([-4, -2]); returnToLatest()');
  await h.flushTimer();
  assert.deepEqual(h.calls[0], [2, 0, 2500]);
  assert.equal(h.evaluate('followingLatest'), true);
});
test('normal refresh respects busy; Run/Stop can explicitly refresh while busy', async () => {
  const h = harness(); h.configure(); h.evaluate('busy = true');
  await h.evaluate('refreshSnapshot()'); assert.equal(h.calls.length, 0);
  await h.evaluate('refreshSnapshot(true)'); assert.equal(h.calls.length, 1);
});
test('a late snapshot cannot overwrite newer navigation', async () => {
  const h = harness(); h.configure();
  let resolve; h.setRead(() => new Promise((r) => resolve = r));
  const pending = h.evaluate('refreshSnapshot()');
  h.evaluate('navigateToRange([-8, -7])');
  resolve({ state: 'LIVE', recordedSeconds: 20, series: [], lostFrames: 0 });
  await pending;
  assert.equal(h.evaluate('snapshot.recordedSeconds'), 10);
  assert.equal(h.evaluate('JSON.stringify(viewRange)'), '[-8,-7]');
});
test('destroy cancels timers and invalidates an in-flight snapshot', async () => {
  const h = harness(); h.configure();
  let resolve; h.setRead(() => new Promise((r) => resolve = r));
  const pending = h.evaluate('refreshSnapshot()');
  h.evaluate('scheduleViewRefresh()');
  h.lifecycle.destroy();
  resolve({ recordedSeconds: 20, series: [], lostFrames: 0 }); await pending;
  assert.equal(h.timers.size, 0);
  assert.equal(h.evaluate('snapshot.recordedSeconds'), 10);
});
test('read failure reports once and releases the snapshot busy flag', async () => {
  const h = harness(); h.configure(); h.setRead(async () => { throw new Error('snapshot unavailable'); });
  await h.evaluate('refreshSnapshot()');
  assert.equal(h.errors.length, 1);
  assert.equal(h.evaluate('snapshotBusy'), false);
});
