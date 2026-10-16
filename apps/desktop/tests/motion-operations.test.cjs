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
  const beginning = source.match(new RegExp(`^  (?:async )?function ${name}\\([^\\n]*?\\)[^\\n]*`, "m"));
  assert.ok(beginning, `Expected actual ${name} handler`);
  // Simple guard methods have a complete one-line body. Do not swallow the
  // following async handler when seeking a separate closing-brace line.
  if (beginning[0].trimEnd().endsWith("}")) return beginning[0];
  const match = source.match(new RegExp(`^  (?:async )?function ${name}\\([^\\n]*?\\)\\s*(?::[^\\n{]+)?\\s*\\{[\\s\\S]*?^  \\}`, "m"));
  assert.ok(match, `Expected complete ${name} handler`);
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
        if (defer && operation === "motion_run") await defer;
        if (ipcError) throw ipcError;
        return operation === "motion_run" ? ipcResult : undefined;
      }};
    },
  });
  vm.runInContext(compile(source("motion/api.ts")), apiContext);
  const page = source("motion/MotionPage.svelte");
  const code = [
    'let actionBusy = false; let stopBusy = false; let runFeedback = ""; let feedbackRevision = 0;',
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
    motorState: "ENABLED", motionActionBusy: false, stopActionBusy: false, experimentCommandRevision: 0,
    experimentState: "IDLE", experimentMessage: "", experimentSnapshot: undefined,
    waveformTab: "channels",
    $parameters: { saving: false }, writing: new Set(),
    $edits: { dirty: new Set() }, $motionState: { incrementalDeltaTurn: 1 }, incrementalDraft: "1",
    selectedTuningEntries: () => [{ id: 0x11, rate: "fast" }],
    selectionWithinLimits: () => true,
    activeMotionMode: () => "position", motionModeSupported: () => true,
    waitMotionUpdates: async () => { events.push("wait-host-writes"); },
    startTuningExperiment: async (selections) => { events.push(["tuning_experiment_start", selections]); return status; },
    stopTuningExperiment: async () => { events.push("tuning_experiment_stop"); return { state: "STOPPING", message: null }; },
    refreshExperiment: async () => { events.push("refresh-status-and-waveform"); },
    onError: (error) => errors.push(String(error)),
  });
  const handlers = ["formatHostNumber","hasDirtyDraft","experimentActive","canRunMotion","canStopMotion","runTuningMotion","stopTuningMotion"].map((name) => extract(page, name)).join("\n");
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
  // Simulate a user editing the Delta field to 3 without pressing Enter.
  h.set('incrementalDraft = "3"');
  assert.equal(h.get("canRunMotion()"), false);
  await h.run();
  assert.equal(h.events.length, 0);
});

test("Tuning Copy Accel to Decel does not manufacture a draft on checkbox toggle",()=>{
  const page=source("control/ControlTuningPage.svelte");
  const calls=[],dirtySymbols=new Set();
  const context=vm.createContext({
    copyAccelToDecel:false,
    MOTION_ACCEL:"PARAM_MOTION_ACCEL",MOTION_DECEL:"PARAM_MOTION_DECEL",
    drafts:{PARAM_MOTION_ACCEL:"100",PARAM_MOTION_DECEL:"40"},
    dirty:symbol=>dirtySymbols.has(symbol),
    edits:{
      edit(symbol,text){calls.push(["edit",symbol,text]);dirtySymbols.add(symbol);},
      discard(symbol){calls.push(["discard",symbol]);dirtySymbols.delete(symbol);},
    },
  });
  vm.runInContext(compile(["toggleCopyAccelToDecel","editAcceleration"].map(n=>extract(page,n)).join("\n")),context);
  vm.runInContext("toggleCopyAccelToDecel(true)",context);
  assert.deepEqual(calls,[["discard","PARAM_MOTION_DECEL"]]);
  assert.equal(dirtySymbols.size,0);
  vm.runInContext('editAcceleration("120")',context);
  assert.deepEqual(calls.slice(1),[
    ["edit","PARAM_MOTION_ACCEL","120"],
    ["edit","PARAM_MOTION_DECEL","120"],
  ]);
  assert(dirtySymbols.has("PARAM_MOTION_DECEL"));
});

test("Motion Stop feedback cannot be overwritten by a slower earlier Run reply",async()=>{
  const h=createMotionHarness();
  let releaseRun;
  h.setDeferred(new Promise(resolve=>{releaseRun=resolve;}));
  const started=h.run();
  assert.equal(h.get("actionBusy"),true);
  await h.stop();
  assert.match(h.get("runFeedback"),/Stop accepted/);
  releaseRun();
  await started;
  assert.match(h.get("runFeedback"),/Stop accepted/);
  assert.deepEqual(h.calls.map(x=>x.operation),["motion_run","motion_stop"]);
});
test("Tuning Stop prevents a late Run preparation reply from overwriting the latest state",async()=>{
  const h=createTuningHarness();
  let finishStart;
  h.set('startTuningExperiment = async () => new Promise(resolve => { finishStart = resolve; })');
  // Closures created in the sandbox hold the completion callback there.
  const start=h.run();
  await new Promise(resolve=>setImmediate(resolve));
  await h.stop();
  assert.equal(h.get("experimentState"),"STOPPING");
  h.set('finishStart({state:"PREPARING",message:null})');
  await start;
  assert.equal(h.get("experimentState"),"STOPPING");
});
