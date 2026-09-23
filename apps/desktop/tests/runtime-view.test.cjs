// Execute the actual Scope component's callbacks with mocked IPC. This is not
// a WebView test; cargo's acquisition tests cover actual transport demand.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const ts = require('typescript');

function scope() {
  const file = path.join(__dirname, '../src/analysis/scope/ScopeEchartsPage.svelte');
  let script = fs.readFileSync(file, 'utf8').match(/<script[^>]*>([\s\S]*?)<\/script>/)[1];
  script = script.replace(/^\s*import[\s\S]*?;\s*$/gm, '').replace(/export let/g, 'let');
  script += `
    globalThis.unit = {
      setup(value) { connection = value; active = true; channels = value.channels; running = false; resetScope(); },
      selected() { return Array.from(selectedIds); },
      snapshot() { return snapshot; },
      markRunning() { running = true; },
      defaultRate, selectedRateCount, setSelected, hotReconfigure, toggleRun,
    };
  `;
  const calls = [], ticks = [], mounts = [];
  const sandbox = {
    console, Map, Set, Number, Array, Math,
    onMount: (fn) => mounts.push(fn), onDestroy: () => {},
    subscribeRefresh: (_ms, fn) => { ticks.push(fn); return () => {}; },
    setTimeout: () => 1, clearTimeout: () => {},
    configureScope: async (items) => { calls.push(['configure', JSON.parse(JSON.stringify(items))]); },
    startScope: async () => { calls.push(['scope-start']); },
    stopScope: async () => { calls.push(['scope-stop']); },
    readScopeSnapshot: async () => { calls.push(['snapshot']); return { state: 'LIVE', recordedSeconds: 1, lostFrames: 0, series: [] }; },
  };
  vm.createContext(sandbox);
  vm.runInContext(ts.transpile(script, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None }), sandbox);
  mounts.forEach((fn) => fn());
  const connection = {
    runtimeChannelIds: [4, 17, 1281], fastMaxChannels: 8, normalMaxChannels: 15,
    channels: [
      { id: 4, label: 'Vbus', unit: 'V', supportsFast: false, supportsNormal: true },
      { id: 17, label: 'Iq', unit: 'A', supportsFast: true, supportsNormal: true },
      { id: 1281, label: 'Position', unit: 'turn', supportsFast: false, supportsNormal: true },
    ],
  };
  sandbox.unit.setup(connection);
  return { unit: sandbox.unit, calls, ticks, connection };
}

test('baseline traces start at NORMAL even when Iq supports FAST', () => {
  const { unit, connection } = scope();
  assert.equal(unit.defaultRate(connection.channels[1]), 'normal');
  assert.deepEqual(Array.from(unit.selected()), connection.runtimeChannelIds);
});

test('a first-visible Scope obtains existing history without Run/configure', async () => {
  const { unit, calls, ticks } = scope();
  ticks.forEach((tick) => tick());
  await new Promise(setImmediate);
  assert.equal(unit.snapshot().state, 'LIVE');
  assert.deepEqual(calls, [['snapshot']]);
});

test('all baseline traces may be hidden; no minimum selected-channel restriction', async () => {
  const { unit, calls, connection } = scope();
  for (const id of connection.runtimeChannelIds) unit.setSelected(id);
  await unit.hotReconfigure();
  assert.deepEqual(Array.from(unit.selected()), []);
  assert.deepEqual(calls, [['configure', []]]);
});

test('visibility counts merge hidden baseline without charging it twice', () => {
  const { unit } = scope();
  assert.equal(unit.selectedRateCount(new Set([17]), new Map([[17, 'normal']]), 'normal'), 3);
  assert.equal(unit.selectedRateCount(new Set([17]), new Map([[17, 'fast']]), 'normal'), 2);
  assert.equal(unit.selectedRateCount(new Set([17]), new Map([[17, 'fast']]), 'fast'), 1);
});

test('Scope Stop calls only the acquisition stop adapter', async () => {
  const { unit, calls } = scope();
  unit.markRunning();
  await unit.toggleRun();
  assert.deepEqual(calls, [['scope-stop'], ['snapshot']]);
});
