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


test("status bar runtime values come from shared baseline telemetry", () => {
  const shell = source("App.svelte");
  assert.match(shell, /runtimeTelemetry/);
  assert.match(shell, /startRuntimeTelemetryTracking/);
  assert.match(shell, /Vbus/);
  assert.match(shell, /const GLOBAL_SYMBOLS = \["PARAM_MOTOR_STATE"\]/);
  assert.match(shell, /const POLLED_SYMBOLS = \["PARAM_MOTOR_STATE"\]/);
  assert.doesNotMatch(shell, /PARAM_RUN_POSITION"\] as const/);
});

test("Scope keeps baseline preview separate from recording state", () => {
  const scope = source("analysis/scope/ScopeEchartsPage.svelte");
  assert.match(scope, /snapshot\?\.preview/);
  assert.match(scope, /running = snapshot\?\.state === "LIVE"/);
  assert.match(scope, /runtimeChannelIds/);
  assert.match(scope, /return "normal"/);
});


test("HostSchema persistence metadata owns RAM-modified eligibility", () => {
  const types = source("parameters/types.ts");
  const state = source("parameters/state.ts");
  const persistence = source("parameters/persistence.ts");
  assert.match(types, /persistent: boolean/);
  assert.match(state, /meta\.persistent/);
  assert.match(persistence, /tracked\.has\(result\.id\)/);
});


test("Identification Apply is scoped by the Application candidate kind", () => {
  const app = readFileSync(join(repo, "crates/nmixx-app/src/application.rs"), "utf8");
  const motor = source("motor/MotorPage.svelte");
  const automation = readFileSync(join(repo, "crates/nmixx-app/src/automation/session_api.rs"), "utf8");
  assert.match(app, /identification_apply_candidate/);
  assert.match(app, /pub fn identification_apply\(&self\)/);
  assert.match(app, /pub fn identification_apply_kind\(&self, kind: IdentificationKind\)/);
  assert.match(motor, /identificationApplyCandidate/);
  assert.match(motor, /applyIdentificationAction\(identKey\)/);
  assert.match(automation, /identification_apply_kind\(kind\)/);
});

test("Commissioning preflight checks blocking Protection masks", () => {
  const preflight = readFileSync(join(repo, "crates/nmixx-app/src/preflight.rs"), "utf8");
  assert.match(preflight, /PARAM_EVENT_ERROR/);
  assert.match(preflight, /PARAM_EVENT_TRIP/);
  assert.match(preflight, /PreflightDomain::Protection/);
});


test("Phase Search motor-state orchestration stays inside MotorActionService", () => {
  const motorActions = readFileSync(join(repo, "crates/nmixx-app/src/motor_actions.rs"), "utf8");
  const start = motorActions.indexOf("pub fn phase_search_start");
  const end = motorActions.indexOf("fn start_action", start);
  assert.notEqual(start, -1);
  assert.notEqual(end, -1);
  const phase = motorActions.slice(start, end);
  assert.match(phase, /MOTOR_STATE/);
  assert.match(phase, /MOTOR_DISABLE/);
  assert.match(phase, /MOTOR_ENABLE/);
  assert.match(phase, /PHASE_SEARCH_MODE/);
  assert.match(phase, /current_state == running/);
  assert.doesNotMatch(source("encoder/EncoderPage.svelte"), /disableMotor|enableMotor|PARAM_MOTOR_MODE/);
});


test("Motion preview may use runtime Plot position while Run keeps exact Position reads", () => {
  const app = readFileSync(join(repo, "crates/nmixx-app/src/application.rs"), "utf8");
  const motion = readFileSync(join(repo, "crates/nmixx-app/src/motion.rs"), "utf8");
  assert.match(app, /motion_preview[\s\S]*runtime_telemetry[\s\S]*position_turns/);
  assert.match(motion, /current_position_turn_override/);
  assert.match(motion, /run_checked[\s\S]*read_position\(parameters, "PARAM_RUN_POSITION"\)/);
});
