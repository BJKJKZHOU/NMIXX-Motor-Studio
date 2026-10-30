const assert = require('node:assert/strict');
const { test } = require('node:test');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const vm = require('node:vm');
const ts = require('typescript');

const context = vm.createContext({ exports: {} });
vm.runInContext(ts.transpileModule(
  readFileSync(join(__dirname, '../src/control/tuningWaveform.ts'), 'utf8'),
  { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } },
).outputText, context);
const display = (input) => JSON.parse(JSON.stringify(context.exports.tuningWaveform(input)));
function capture(origin = 4000.25) {
  return {
    positionOriginTurns: origin, positionChannelIds: [1281, 1298],
    recordedSeconds: 1, captureId: 1, plannedDurationSeconds: 10,
    config: { channels: [
      { id: 1281, label: 'Position', unit: 'turn' },
      { id: 1298, label: 'Position Ref', unit: 'turn' },
      { id: 1282, label: 'Speed', unit: 'rad/s' },
    ] },
    snapshot: { series: [
      { id: 1281, times: [-1, -.5, 0], values: [4000.25, 4010.25, 4030.25], envelopeMin: [4000, 4010, 4030], envelopeMax: [4000.5, 4010.5, 4030.5] },
      { id: 1298, times: [-1, -.5, 0], values: [4000.75, 4011.25, 4030.25] },
      { id: 1282, times: [-1, -.5, 0], values: [0, 500, 0] },
    ] },
  };
}

test('position display shares the actual origin without altering capture or tracking error', () => {
  const input = capture();
  const original = structuredClone(input);
  const result = display(input);
  assert.deepEqual(result.snapshot.series[0].values, [0, 10, 30]);
  assert.deepEqual(result.snapshot.series[1].values, [.5, 11, 30]);
  assert.deepEqual(result.snapshot.series[0].envelopeMin, [-.25, 9.75, 29.75]);
  assert.deepEqual(result.snapshot.series[0].envelopeMax, [.25, 10.25, 30.25]);
  assert.deepEqual(result.snapshot.series[2], { ...input.snapshot.series[2], times: [0, .5, 1] });
  assert.equal(result.channels[0].label, 'Δ Position');
  assert.deepEqual(input, original);
});

test('a panned window and reference-only capture retain the experiment origin', () => {
  const input = capture();
  input.config.channels = [input.config.channels[1]];
  input.snapshot.series = [{ id: 1298, times: [-.1, 0], values: [4011.25, 4030.25] }];
  assert.deepEqual(display(input).snapshot.series[0].values, [11, 30]);
});

test('new experiments reset the origin and reverse/return motion keeps its sign', () => {
  const input = capture(4030.25);
  input.snapshot.series[0].values = [4030.25, 4000.25, 4030.25];
  assert.deepEqual(display(input).snapshot.series[0].values, [0, -30, 0]);
  input.positionOriginTurns = -.25;
  input.snapshot.series[0].values = [-.25, .25, -1.25];
  assert.deepEqual(display(input).snapshot.series[0].values, [0, .5, -1]);
});

test('missing origin does not invent a zero from the current window', () => {
  const input = capture(null);
  const result = display(input);
  assert.deepEqual(result.channels, input.config.channels);
  assert.deepEqual(result.snapshot.series[0].values, input.snapshot.series[0].values);
  assert.deepEqual(result.snapshot.series[0].times, [0, .5, 1]);
});

test('planned full X extent stays fixed while old samples keep their experiment times', () => {
  const input = capture();
  const range = (x) => Array.from(context.exports.tuningTimeRange(x));
  assert.deepEqual(range(input), [0, 10]);
  assert.deepEqual(display(input).snapshot.series[0].times, [0, .5, 1]);
  input.recordedSeconds = 7;
  input.snapshot.series[0].times = [-7, -6.5, -6];
  assert.deepEqual(range(input), [0, 10]);
  assert.deepEqual(display(input).snapshot.series[0].times, [0, .5, 1]);
  input.recordedSeconds = 10.1;
  assert.deepEqual(range(input), [0, 11]);
  input.recordedSeconds = 10.2;
  assert.deepEqual(range(input), [0, 11]);
  input.plannedDurationSeconds = null;
  input.recordedSeconds = 0;
  assert.deepEqual(range(input), [0, 3]);
  input.recordedSeconds = 3.1;
  assert.deepEqual(range(input), [0, 3.5]);
});
