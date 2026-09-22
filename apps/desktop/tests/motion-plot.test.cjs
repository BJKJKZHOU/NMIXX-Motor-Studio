// Option/lifecycle tests. Actual chart initialization under POSIX locales is
// covered separately by the real-browser startup fixture.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const vm = require('node:vm');
const ts = require('typescript');

function harness() {
  const source = readFileSync(join(__dirname, '../src/motion/MotionTrajectoryPlot.svelte'), 'utf8')
    .match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
  const options = [];
  const initializations = [];
  let onMount;
  let disposed = 0;
  let observerDisconnected = 0;
  const chart = { setOption: (...args) => options.push(args), resize() {}, dispose() { disposed++; } };
  const context = vm.createContext({
    exports: {},
    require(name) {
      if (name === 'svelte') return { onMount: (cb) => onMount = cb };
      if (name === 'echarts/charts') return { LineChart: {} };
      if (name === 'echarts/components') return { GridComponent: {} };
      if (name === 'echarts/renderers') return { CanvasRenderer: {} };
      if (name === 'echarts/core') return { use() {}, init: (...args) => { initializations.push(args); return chart; } };
      throw new Error(`Unexpected runtime import: ${name}`);
    },
    ResizeObserver: class { observe() {} disconnect() { observerDisconnected++; } },
  });
  vm.runInContext(ts.transpileModule(source, { compilerOptions: {
    target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS,
  } }).outputText, context);
  const preview = { times: [0, 0.5, 1], primary: [0, 0.5, 1], secondary: [0, 2, 0],
    primaryLabel: 'Position', primaryUnit: 'turn', secondaryLabel: 'Speed', secondaryUnit: 'rad/s' };
  context.exports.preview = preview;
  const evaluate = (code) => vm.runInContext(code, context);
  return { preview, evaluate, options, initializations, mount: () => onMount(),
    disposed: () => disposed, observerDisconnected: () => observerDisconnected };
}

test('position preview retains independent primary and secondary axes', () => {
  const h = harness(); const option = h.evaluate('makeOption(exports.preview)');
  assert.equal(option.yAxis.length, 2); assert.equal(option.series.length, 2);
  assert.equal(option.yAxis[0].name, 'Position (turn)');
  assert.equal(option.yAxis[1].name, 'Speed (rad/s)');
  assert.equal(option.series[1].yAxisIndex, 1);
  assert.equal(JSON.stringify(option.series[1].data), '[[0,0],[0.5,2],[1,0]]');
});
test('single-series preview omits the secondary axis', () => {
  const h = harness(); h.preview.secondary = [];
  const option = h.evaluate('makeOption(exports.preview)');
  assert.equal(option.yAxis.length, 1); assert.equal(option.series.length, 1);
});
test('resize reuses the chart and sets an explicit chart locale', () => {
  const h = harness(); h.evaluate('host = {getBoundingClientRect: () => ({width:800, height:480})}');
  const cleanup = h.mount(); h.evaluate('ensurePlot()');
  assert.equal(h.initializations.length, 1);
  assert.equal(h.initializations[0][2].locale, 'EN');
  h.preview.secondary = []; h.evaluate('updatePlot(exports.preview)');
  assert.equal(h.options.at(-1)[0].yAxis.length, 1);
  assert.equal(h.options.at(-1)[1].notMerge, true);
  cleanup(); assert.equal(h.disposed(), 1); assert.equal(h.observerDisconnected(), 1);
});
test('a hidden preview does not initialize a zero-sized chart', () => {
  const h = harness(); h.evaluate('host = {getBoundingClientRect: () => ({width:0, height:0})}');
  const cleanup = h.mount(); assert.equal(h.initializations.length, 0); cleanup();
});
