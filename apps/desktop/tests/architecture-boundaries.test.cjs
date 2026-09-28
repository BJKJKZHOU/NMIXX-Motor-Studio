const { test } = require("node:test");
const assert = require("node:assert/strict");
const { existsSync, readFileSync } = require("node:fs");
const { join, resolve } = require("node:path");

const desktop = resolve(__dirname, "..");
const repo = resolve(desktop, "../..");

function source(path) {
  return readFileSync(join(desktop, "src", path), "utf8");
}

test("business pages do not invoke Tauri directly", () => {
  for (const page of [
    "motor/MotorPage.svelte",
    "encoder/EncoderPage.svelte",
    "limits/LimitsPage.svelte",
    "control/ControlPage.svelte",
    "control/ControlTuningPage.svelte",
    "motion/MotionPage.svelte",
    "parameters/ParameterTablePage.svelte",
    "analysis/scope/ScopeEchartsPage.svelte",
  ]) {
    const text = source(page);
    assert.doesNotMatch(text, /@tauri-apps\/api\/core/);
    assert.doesNotMatch(text, /\binvoke\s*\(/);
  }
});

test("global Stop remains connection-scoped, not motor-state gated", () => {
  const shell = source("App.svelte");
  const command = shell.match(/async function stopCurrentMotorOperation\(\) \{[\s\S]*?\n  \}/)?.[0] ?? "";
  assert.match(command, /if \(!connection \|\| globalStopBusy\) return;/);
  assert.doesNotMatch(command, /motorState/);
  assert.match(shell, /stop-action" disabled=\{!connection \|\| globalStopBusy\}/);
});

test("Motor and Encoder pages use semantic capabilities rather than firmware Action symbols", () => {
  for (const page of ["motor/MotorPage.svelte", "encoder/EncoderPage.svelte"]) {
    const text = source(page);
    assert.doesNotMatch(text, /\bACTION_[A-Z0-9_]+\b/);
    assert.doesNotMatch(text, /\blistActions\b|\bActionMetadata\b/);
  }
});

test("formal CLI keeps the existing expert action start surface", () => {
  const cli = readFileSync(join(repo, "crates/nmixx-cli/src/main.rs"), "utf8");
  const app = readFileSync(join(repo, "crates/nmixx-app/src/application.rs"), "utf8");
  assert.match(cli, /enum ActionCommand[\s\S]*?\bStart\s*\{/);
  assert.match(cli, /app\.expert_action_start\(&action\.symbol\)/);
  assert.match(app, /pub fn expert_action_start\(/);
});

test("desktop does not expose generic Action discovery or execution IPC", () => {
  const tauri = readFileSync(join(desktop, "src-tauri/src/main.rs"), "utf8");
  assert.doesNotMatch(tauri, /fn action_list\b|fn action_start\b|fn action_start_immediate\b/);
});

test("legacy fast-only ScopeSession implementation stays removed", () => {
  assert.equal(existsSync(join(repo, "crates/nmixx-app/src/scope.rs")), false);
  const lib = readFileSync(join(repo, "crates/nmixx-app/src/lib.rs"), "utf8");
  assert.doesNotMatch(lib, /mod scope;/);
});

test("Control Tuning reuses the shared Scope ECharts viewer", () => {
  const experiment = source("control/ExperimentWaveform.svelte");
  assert.match(experiment, /ScopeEchartsView/);
  assert.doesNotMatch(experiment, /from "echarts\//);
});


test("Limits displays firmware Effective values instead of deriving an active source", () => {
  const limits = source("limits/LimitsPage.svelte");
  assert.match(limits, /PARAM_LIMIT_I_EFFECTIVE/);
  assert.match(limits, /PARAM_LIMIT_WM_EFFECTIVE/);
  assert.doesNotMatch(limits, /activeSource\s*\(/);
  assert.doesNotMatch(limits, /user\s*<=\s*hardware/);
});


test("Events and Problems use the shared Application problem projection", () => {
  const shell = source("App.svelte");
  const events = source("events/EventsPage.svelte");
  assert.match(shell, /EventsPage/);
  assert.match(shell, /problemSnapshot/);
  assert.doesNotMatch(shell, /Problems service is not implemented yet/);
  assert.doesNotMatch(events, /@tauri-apps\/api\/core|\binvoke\s*\(/);
  assert.match(events, /recheckProblems/);
  assert.match(events, /clearProtection/);
});
