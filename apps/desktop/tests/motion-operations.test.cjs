// Executes the real Svelte command handlers against a fake Tauri IPC/device.
// This exercises user-facing guards, the IPC command name, acceptance feedback,
// duplicate-click handling and response/error propagation without physical hardware.
const { test } = require("node:test");
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const vm = require("node:vm");
const ts = require("typescript");

const desktop = join(__dirname, "..");
function source(path) { return readFileSync(join(desktop, "src", path), "utf8"); }
function extract(source, name) {
  const match = source.match(new RegExp(`^  (?:async )?function ${name}\\([^\\n]*?\\)\\s*(?::[^\\n{]+)?\\s*\\{[\\s\\S]*?^  \\}`, "m"));
  assert.ok(match, `Expected actual ${name} handler`);
  return match[0];
}
function compile(code) {
  return ts.transpileModule(code, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
}
function createMotionHarness() {
  const calls = [];
  const errors = [];
  let ipcResult = { kind: "runAccepted", handle: { txn: 1, actionId: 0x1002 } };
  let ipcError;
  let defer;
  const apiContext = vm.createContext({
    exports: {},
    require(name) {
      assert.equal(name, "@tauri-apps/api/core");
      return { invoke: async (operation, args) => {
        calls.push({ operation, args });
        if (defer) await defer;
        if (ipcError) throw ipcError;
        return operation === "motion_run" ? ipcResult : undefined;
      }};
    },
  });
  vm.runInContext(compile(source("motion/api.ts")), apiContext);
  const page = source("motion/MotionPage.svelte");
  const code = [
    'let actionBusy = false; let stopBusy = false; let runFeedback = "";',
    'let uncommittedMotion = false;',
    ...["canRun", "canStop", "run", "stop"].map((name) => extract(page, name)),
  ].join("\n");
  const context = vm.createContext({
    capabilities: { run: true, stop: true, position: true, speed: true },
    motorState: "ENABLED", activeMode: "position",
    $motionState: { repeat: false },
    $parameters: { saving: false },
    writing: new Set(),
    modeSupported: () => true,
    onError(error) { errors.push(String(error)); },
    executeMotion: () => apiContext.exports.runMotion(),
    stopMotionExecution: () => apiContext.exports.stopMotion(),
  });
  vm.runInContext(compile(code), context);
  const get = (expression) => vm.runInContext(expression, context);
  const set = (expression) => vm.runInContext(expression, context);
  return { calls, errors, get, set, setResult: (value) => ipcResult = value,
    setError: (value) => ipcError = value, setDeferred: (promise) => defer = promise,
    run: () => get("run()"), stop: () => get("stop()") };
}

test("editing Delta without Enter blocks Run and sends no firmware command", async () => {
  const h = createMotionHarness();
  // The user's text is dirty while the backend still owns the previous value.
  h.set("uncommittedMotion = true");
  assert.equal(h.get("canRun()"), false);
  await h.run(); // even a programmatic button invocation must be rejected
  assert.equal(h.calls.length, 0);
  assert.match(h.get("runFeedback"), /Commit or discard/);
  assert.match(h.errors[0], /Commit or discard/);
});

test("first Position Run invokes Tauri and displays Action acceptance, not motion completion", async () => {
  const h = createMotionHarness();
  assert.equal(h.get("canRun()"), true);
  await h.run();
  assert.deepEqual(h.calls.map(({operation}) => operation), ["motion_run"]);
  assert.match(h.get("runFeedback"), /Run accepted by device/);
  assert.doesNotMatch(h.get("runFeedback"), /complete/i);
  h.set('motorState = "RUN"');
  assert.equal(h.get("canRun()"), false);
});

test("manual Repeat target-only write has a separate truthful UI acknowledgement", async () => {
  const h = createMotionHarness();
  h.set('motorState = "RUN"; $motionState.repeat = true');
  h.setResult({ kind: "targetUpdated" });
  assert.equal(h.get("canRun()"), true);
  await h.run();
  assert.equal(h.calls[0].operation, "motion_run");
  assert.match(h.get("runFeedback"), /Position target updated/);
});

test("device Run rejection becomes a visible UI error, not an accepted status", async () => {
  const h = createMotionHarness();
  h.setError(new Error("ERR_STATE"));
  await h.run();
  assert.match(h.get("runFeedback"), /Run failed.*ERR_STATE/);
  assert.match(h.errors[0], /ERR_STATE/);
});

test("pending Run cannot be submitted twice", async () => {
  const h = createMotionHarness();
  let release;
  h.setDeferred(new Promise((resolve) => { release = resolve; }));
  const first = h.run();
  assert.equal(h.get("actionBusy"), true);
  await h.run();
  assert.equal(h.calls.length, 1);
  release();
  await first;
  assert.equal(h.get("actionBusy"), false);
});

test("Stop is dispatched through Tauri and acknowledges request, not stopped state", async () => {
  const h = createMotionHarness();
  await h.stop();
  assert.deepEqual(h.calls.map(({operation}) => operation), ["motion_stop"]);
  assert.match(h.get("runFeedback"), /Stop accepted; waiting/);
});

function createTuningHarness() {
  const events = [], errors = [];
  let status = { state: "PREPARING", message: null };
  const page = source("control/ControlTuningPage.svelte");
  const context = vm.createContext({
    connection: { motion: { run: true, stop: true } },
    motorState: "ENABLED", motionActionBusy: false, stopActionBusy: false,
    experimentState: "IDLE", experimentMessage: "", experimentSnapshot: undefined,
    waveformTab: "channels",
    $parameters: { saving: false }, writing: new Set(),
    hasDirtyDraft: () => false, experimentActive: () => ["PREPARING","RUNNING","STOPPING"].includes(vm.runInContext("experimentState", context)),
    selectedTuningEntries: () => [{ id: 0x11, rate: "fast" }],
    selectionWithinLimits: () => true,
    activeMotionMode: () => "position", motionModeSupported: () => true,
    waitMotionUpdates: async () => { events.push("wait-host-writes"); },
    startTuningExperiment: async (selections) => { events.push(["tuning_experiment_start", selections]); return status; },
    stopTuningExperiment: async () => { events.push("tuning_experiment_stop"); return { state: "STOPPING", message: null }; },
    refreshExperiment: async () => { events.push("refresh-status-and-waveform"); },
    onError: (error) => errors.push(String(error)),
  });
  const handlers = ["canRunMotion","canStopMotion","runTuningMotion","stopTuningMotion"].map((name) => extract(page, name)).join("\n");
  vm.runInContext(compile(handlers), context);
  const get = (expr) => vm.runInContext(expr, context);
  return { events, errors, get, set: get, setStatus: (value) => status = value,
    run: () => get("runTuningMotion()"), stop: () => get("stopTuningMotion()") };
}

test("Tuning Run starts one captured experiment and UI follows PREPARING/RUNNING state", async () => {
  const h = createTuningHarness();
  assert.equal(h.get("canRunMotion()"), true);
  await h.run();
  assert.equal(h.events[0], "wait-host-writes");
  assert.equal(h.events[1][0], "tuning_experiment_start");
  assert.equal(h.events[1][1][0].id, 0x11);
  assert.equal(h.get("experimentState"), "PREPARING");
  assert.equal(h.get("canRunMotion()"), false);
  h.set('experimentState = "RUNNING"');
  assert.equal(h.get("canStopMotion()"), true);
  await h.stop();
  assert.equal(h.get("experimentState"), "STOPPING");
  assert(h.events.includes("tuning_experiment_stop"));
});

test("Tuning dirty input disables Run, no experiment or motor command is dispatched", async () => {
  const h = createTuningHarness();
  h.set('hasDirtyDraft = () => true');
  assert.equal(h.get("canRunMotion()"), false);
  await h.run();
  assert.equal(h.events.length, 0);
});
