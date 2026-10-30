import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";

// This fixture never opens a device. Use the SDK's event mock rather than
// implementing a competing Tauri transport. The runner checks every command.
const fixture = new URLSearchParams(location.search);
window.__nmixxStartupRequests = [];
window.__nmixxStartupArgs = [];
window.__nmixxUIErrors = [];
let mockTask = {
  taskId: 0, state: "IDLE", name: "", exitCode: null, message: null,
  firstSequence: 1, nextSequence: 1, logs: [], effects: [],
};
let motorState = 0;
const motorMeta = {
  id: 1, symbol: "PARAM_MOTOR_STATE", label: "Motor State",
  typeName: "u8", access: "ro", allowed: [0, 1, 2],
  allowedSymbols: ["DISABLED", "ENABLED", "RUN"],
  persistent: false, description: "", unit: null, default: 0, writeState: null,
};
const controlMetadata = [
  [0x0a08, "PARAM_CTRL_CURRENT_CONTROLLER", "Current Controller", "u8", "disabled", ["CURRENT_CTRL_PI", "CURRENT_CTRL_LADRC", "CURRENT_CTRL_DPCC"]],
  [0x0a16, "PARAM_CTRL_SPEED_CONTROLLER", "Speed Controller", "u8", "disabled", ["SPEED_CTRL_PI", "SPEED_CTRL_LADRC"]],
  [0x0a09, "PARAM_CTRL_LADRC_BW_HZ", "LADRC Current Gain", "f32", "not_running"],
  [0x0a0a, "PARAM_CTRL_LADRC_ESO_BW_HZ", "LADRC LESO Gain", "f32", "not_running"],
  [0x0a17, "PARAM_CTRL_SPEED_LADRC_BW_HZ", "LADRC Speed Gain", "f32", "disabled"],
  [0x0a11, "PARAM_CTRL_SPEED_KP", "Manual Speed Kp", "f32", "not_running"],
  [0x0a01, "PARAM_CTRL_ID_KP", "Manual Id Kp", "f32", "not_running"],
].map(([id, symbol, label, typeName, writeState, allowedSymbols = []]) => ({
  ...motorMeta, id, symbol, label, typeName, writeState, allowedSymbols,
  access: "rw", allowed: [], unit: typeName === "f32" ? "Hz" : null,
}));
const controlValues = new Map(controlMetadata.map((meta) => [meta.id, {
  type: meta.typeName, value: meta.id === 0x0a08 ? 2 : meta.id === 0x0a16 ? 1 : 50,
}]));
const encoderMetadata = [
  [0x0301, "PARAM_ENCODER_PROTOCOL", "Protocol", "u8", ["ENC_PROTOCOL_NONE", "ENC_PROTOCOL_SPI", "ENC_PROTOCOL_ABZ"]],
  [0x030a, "PARAM_ENCODER_SPI_TYPE", "SPI Encoder", "u8", ["ENC_SPI_NONE", "ENC_SPI_MT6816", "ENC_SPI_MT6835"]],
  [0x0401, "PARAM_PHASE_I_SEARCH", "Search current", "f32", []],
  [0x0402, "PARAM_CAL_VALID", "Calibration State", "u8", []],
].map(([id, symbol, label, typeName, allowedSymbols]) => ({ ...motorMeta,
  id, symbol, label, typeName, allowedSymbols, allowed: [], writeState: "disabled",
  access: symbol === "PARAM_CAL_VALID" ? "ro" : "rw",
}));
for (const meta of encoderMetadata) controlValues.set(meta.id, { type: meta.typeName,
  value: meta.symbol === "PARAM_CAL_VALID" ? 1 : meta.typeName === "f32" ? 0.5 : 1 });
const motionMetadata = [
  [0x0701, "PARAM_MOTOR_MODE", "Mode", "u8", "disabled", ["TORQUE", "SPEED", "POSITION", "OPEN_LOOP", "IDENT", "SENSORLESS_SPEED", "ENCODER_CAL"]],
  [0x0704, "PARAM_TARGET_POSITION", "Position Target", "position", null, []],
  [0x0703, "PARAM_TARGET_SPEED", "Speed Target", "f32", null, []],
  [0x0702, "PARAM_TARGET_TORQUE", "Torque Target", "f32", null, []],
  [0x0602, "PARAM_MOTION_WM_ACC", "Acceleration", "f32", null, []],
  [0x0603, "PARAM_MOTION_WM_DEC", "Deceleration", "f32", null, []],
  [0x0605, "PARAM_MOTION_PROFILE", "Profile", "u8", "not_running", ["MOTION_TRAPEZOID", "MOTION_S_TIME", "MOTION_S_PEAK"]],
].map(([id, symbol, label, typeName, writeState, allowedSymbols]) => ({ ...motorMeta,
  id, symbol, label, typeName, writeState, allowedSymbols, allowed: [], access: "rw",
}));
for (const meta of motionMetadata) controlValues.set(meta.id, { type: meta.typeName,
  value: meta.typeName === "position" ? { turns: 0, theta: 0 } : meta.id === 0x0701 ? 2 : meta.typeName === "u8" ? 0 : 10 });
let motionConfig = { positionCommand: "incremental", incrementalDeltaTurn: 1, repeat: false };
function fixtureMetadata() {
  return [motorMeta, ...(fixture.has("controllers") ? controlMetadata : fixture.has("encoder") ? encoderMetadata : fixture.has("motion") ? motionMetadata : [])];
}
let encoderTask;
let encoderTxn = 10;
window.__completeEncoderTask = async (ok = true) => {
  const task = encoderTask;
  encoderTask = undefined;
  motorState = 0;
  controlValues.set(0x0402, { type: "u8", value: ok ? task.operation === "encoder.calibration" ? 2 : 1 : 0 });
  await emit("action-completed", { ...task, status: ok ? "Ok" : "ErrConfig", ok });
};
function motorReadback(ids = [1]) {
  return ids.filter((id) => id === 1).map((id) => ({
    id, value: { type: "u8", value: motorState }, error: null,
  }));
}
function deviceReadback(ids) {
  const metadata = fixtureMetadata();
  return (ids ?? metadata.map((meta) => meta.id)).flatMap((id) => id === 1
    ? motorReadback([id]) : controlValues.has(id) ? [{ id, value: controlValues.get(id), error: null }] : []);
}

mockIPC((command, args) => {
  window.__nmixxStartupRequests.push(command);
  window.__nmixxStartupArgs.push({ command, args });
  if (command === "default_schema_path") return "/firmware/build/host/axdr-host-schema.toml";
  if (command === "plugin:dialog|open") return "/selected/axdr-host-schema.toml";
  if (command === "device_list" && fixture.has("ports")) return window.__nmixxPorts ?? ["/dev/serial/by-id/usb-MICROLINK_MICROLINK_CMSIS-DAP_012345ABCDEF-if01"];
  if (command === "device_list") return fixture.has("motor-controls") || fixture.has("controllers") || fixture.has("encoder") || fixture.has("motion") ? ["COM7"] : [];
  if (command === "action_list") return [];
  if (command === "device_disconnect") return;
  if (fixture.has("motor-controls") || fixture.has("controllers") || fixture.has("encoder") || fixture.has("motion")) {
    if (command === "device_connect") return {
      port: "COM7", fastMaxChannels: 4, normalMaxChannels: 8,
      fastBlockSamples: 5, fastRateHz: 20000, normalRateHz: 1000,
      channels: [], runtimeChannelIds: [],
      motion: { run: true, stop: true, enable: true, disable: true,
        position: true, speed: true, sensorlessSpeed: false, torque: true },
      commissioning: { identificationRsLs: true, identificationFlux: true, identificationJb: true, identificationApply: true, phaseSearch: fixture.has("encoder"), encoderCalibration: fixture.has("encoder") },
    };
    if (command === "identification_apply_candidate") return null;
    if (command === "identification_start")
      return { status: "blocked", issues: [{ reason: "Current limit must be configured", parameterId: null, suggestedDomain: "limits" }] };
    if (command === "parameter_list") return fixtureMetadata();
    if (command === "parameter_cached_many" || command === "parameter_read_many")
      return deviceReadback(args.ids);
    if (command === "parameter_refresh_all") return deviceReadback();
    if (command === "parameter_write" && (fixture.has("controllers") || fixture.has("motion"))) {
      if (window.__rejectParameterWrite) throw new Error("ERR_STATE: controller write rejected");
      const meta = fixtureMetadata().find((item) => item.id === args.id);
      if (!meta || (meta.writeState === "disabled" && motorState !== 0)) throw new Error("ERR_STATE");
      const complete = () => {
        controlValues.set(args.id, args.value);
        return { id: args.id, value: args.value };
      };
      return fixture.has("motion") ? new Promise(resolve => setTimeout(() => resolve(complete()), 150)) : complete();
    }
    if (fixture.has("encoder") && ["phase_search_start", "encoder_calibration_start"].includes(command)) {
      if (window.__rejectEncoderStart) throw new Error("Encoder calibration prerequisites not met");
      if (encoderTask) throw new Error("Encoder task already running");
      const calibration = command === "encoder_calibration_start";
      motorState = 2;
      encoderTask = { txn: encoderTxn++, actionId: calibration ? 0x1401 : 0x1402,
        operation: calibration ? "encoder.calibration" : "encoder.phaseSearch" };
      return { txn: encoderTask.txn, actionId: encoderTask.actionId };
    }
    if (fixture.has("encoder") && command === "motor_stop") {
      motorState = 0;
      void (async () => { await emit("motor-stop-issued"); if (encoderTask) await window.__completeEncoderTask(false); })();
      return { txn: encoderTxn++, actionId: 0x1003 };
    }
    if (fixture.has("motion")) {
      if (command === "motion_get") return motionConfig;
      if (command === "motion_set") { motionConfig = args.config; return motionConfig; }
      if (command === "motion_preview") return { times: [0, 1], primary: [0, 1], secondary: [0, 0], primaryLabel: "Position", primaryUnit: "turn" };
      if (command === "motion_run") {
        motorState = 2;
        void emit("parameters-changed", [1]);
        return { kind: "runAccepted", handle: { txn: 20, actionId: 0x1002 } };
      }
      if (command === "motion_stop") return new Promise(resolve => setTimeout(() => {
        motorState = 1; void emit("parameters-changed", [1]); resolve();
      }, 150));
    }
    if (command === "config_save_available") return true;
    if (command === "config_save") return { txn: 4, actionId: 0x1109 };
    if (command === "motor_enable") { motorState = 1; return { txn: 1, actionId: 0x1001 }; }
    if (command === "motor_disable") { motorState = 0; return { txn: 2, actionId: 0x1002 }; }
    if (command === "motor_stop") return { txn: 3, actionId: 0x1003 };
    if (command === "runtime_telemetry")
      return { currentIq: 0.1, speedWm: 0, positionTurns: 0, vbus: 24.8 };
    if (command === "problems_snapshot") return { active: [], history: [] };
  }
  if (command === "parameter_list") return [];
  if (command === "automation_snapshot") return mockTask;
  if (command === "automation_start") {
    mockTask = { ...mockTask, taskId: 1, name: args.spec.name, state: "RUNNING" };
    return mockTask;
  }
  if (command === "automation_cancel") {
    mockTask = { ...mockTask, state: "CANCELLED" };
    return;
  }
  if (fixture.has("tuning-tabs")) {
    if (command === "motion_get") return { positionCommand: "incremental", incrementalDeltaTurn: 1, repeat: false };
    if (command === "tuning_experiment_status") return { state: window.__tuningState ?? "COMPLETED", message: null };
    if (command === "tuning_experiment_snapshot") return {
      status: { state: window.__tuningState ?? "COMPLETED" },
      config: { channels: [
        { id: 17, label: "Iq", rate: "fast", unit: "A" },
        { id: 1296, label: "Iq Ref", rate: "fast", unit: "A" },
        { id: 1282, label: "Speed", rate: "normal", unit: "rad/s" },
        { id: 1281, label: "Position", rate: "normal", unit: "turn" },
        { id: 4, label: "Vbus", rate: "normal", unit: "V" },
        { id: 1298, label: "Position Ref", rate: "normal", unit: "turn" },
      ] },
      snapshot: {
        state: "STOPPED", recordedSeconds: window.__tuningRecordedSeconds ?? 1, lostFrames: 0, series: [
          { id: 17, times: [-1, -0.5, 0].map(t => t + 1 - (window.__tuningRecordedSeconds ?? 1)), values: [0, 0.5, 0.2] },
          { id: 1296, times: [-1, -0.5, 0].map(t => t + 1 - (window.__tuningRecordedSeconds ?? 1)), values: [0, 0.6, 0.2] },
          { id: 1282, times: [-1, -0.5, 0].map(t => t + 1 - (window.__tuningRecordedSeconds ?? 1)), values: [0, 500, 0] },
          { id: 1281, times: [-1, -0.5, 0].map(t => t + 1 - (window.__tuningRecordedSeconds ?? 1)), values: [4000, 4010, 4030] },
          { id: 4, times: [-1, -0.5, 0].map(t => t + 1 - (window.__tuningRecordedSeconds ?? 1)), values: [24, 25, 24] },
          { id: 1298, times: [-1, -0.5, 0].map(t => t + 1 - (window.__tuningRecordedSeconds ?? 1)), values: [4000.5, 4011, 4030] },
        ],
      },
      recordedSeconds: window.__tuningRecordedSeconds ?? 1, windowSeconds: 0.2, endOffsetSeconds: 0,
      positionOriginTurns: 4000, positionChannelIds: [1281, 1298],
      captureId: window.__tuningCaptureId ?? 1, plannedDurationSeconds: window.__tuningPlannedSeconds ?? 10,
    };
  }
  if (command === "motion_get") throw new Error("device is not connected");
  throw new Error(`Unexpected IPC during disconnected startup: ${command}`);
}, { shouldMockEvents: true });

if (fixture.has("tuning-tabs")) {
  const { setUiLanguage } = await import("../src/i18n");
  setUiLanguage("en");
  await import("@vscode-elements/elements/dist/bundled.js");
  await import("../src/theme.css");
  const { mount } = await import("svelte");
  const { default: ControlTuningPage } = await import("../src/control/ControlTuningPage.svelte");
  const { getInstanceByDom } = await import("echarts/core");
  window.__tuningChart = () => getInstanceByDom(document.querySelector(".scope-echarts-view"));
  mount(ControlTuningPage, {
    target: document.getElementById("app"),
    props: {
      connection: {
        channels: [{ id: 17, label: "Iq", unit: "A", supportsFast: true, supportsNormal: true }],
        fastMaxChannels: 8, normalMaxChannels: 8,
        motion: { run: true, stop: true, position: true, speed: true, torque: true },
      },
      motorState: "DISABLED",
      onError: (error) => window.__nmixxUIErrors.push(String(error)),
    },
  });
} else if (fixture.has("automation-script")) {
  // Standalone page fixtures do not mount App.svelte, which normally restores
  // the persisted language. Set English explicitly for stable button labels.
  const { setUiLanguage } = await import("../src/i18n");
  setUiLanguage("en");
  await import("@vscode-elements/elements/dist/bundled.js");
  await import("../src/theme.css");
  const { mount } = await import("svelte");
  const { default: AutomationPage } = await import("../src/automation/AutomationPage.svelte");
  // This fixture mounts a single page without App.svelte: initialize the same
  // shared Parameter session the real Connect flow would have established.
  const { connectParameters } = await import("../src/parameters/state");
  await connectParameters();
  mount(AutomationPage, {
    target: document.getElementById("app"),
    props: { connected: true },
  });
} else if (fixture.has("preview")) {
  await import("../src/theme.css");
  const { mount } = await import("svelte");
  const { default: MotionTrajectoryPlot } = await import("../src/motion/MotionTrajectoryPlot.svelte");
  mount(MotionTrajectoryPlot, {
    target: document.getElementById("app"),
    props: {
      preview: {
        times: [0, 0.5, 1], primary: [0, 0.5, 1], secondary: [0, 2, 0],
        primaryLabel: "Position", primaryUnit: "turn",
        secondaryLabel: "Speed", secondaryUnit: "rad/s",
      },
    },
  });
} else {
  // Import the real entry point and all its static dependencies after mock setup.
  // Vite compiles the real Svelte templates; no component or chart is stubbed.
  await import("../src/main.ts");
}
