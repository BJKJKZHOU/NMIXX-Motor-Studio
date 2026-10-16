import { mockIPC } from "@tauri-apps/api/mocks";

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
function motorReadback(ids = [1]) {
  return ids.filter((id) => id === 1).map((id) => ({
    id, value: { type: "u8", value: motorState }, error: null,
  }));
}

mockIPC((command, args) => {
  window.__nmixxStartupRequests.push(command);
  window.__nmixxStartupArgs.push({ command, args });
  if (command === "device_list") return fixture.has("motor-controls") ? ["COM7"] : [];
  if (command === "action_list") return [];
  if (command === "device_disconnect") return;
  if (fixture.has("motor-controls")) {
    if (command === "device_connect") return {
      port: "COM7", fastMaxChannels: 4, normalMaxChannels: 8,
      fastBlockSamples: 5, fastRateHz: 20000, normalRateHz: 1000,
      channels: [], runtimeChannelIds: [],
      motion: { run: true, stop: true, enable: true, disable: true,
        position: true, speed: true, sensorlessSpeed: false, torque: true },
      commissioning: { identificationRsLs: true, identificationFlux: true, identificationJb: true, identificationApply: true },
    };
    if (command === "identification_apply_candidate") return null;
    if (command === "identification_start")
      return { status: "blocked", issues: [{ reason: "Current limit must be configured", parameterId: null, suggestedDomain: "limits" }] };
    if (command === "parameter_list") return [motorMeta];
    if (command === "parameter_cached_many" || command === "parameter_read_many")
      return motorReadback(args.ids);
    if (command === "parameter_refresh_all") return motorReadback();
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
    if (command === "tuning_experiment_status") return { state: "COMPLETED", message: null };
    if (command === "tuning_experiment_snapshot") return {
      status: { state: "COMPLETED" },
      config: { channels: [{ id: 17, label: "Iq", rate: "fast", unit: "A" }] },
      snapshot: {
        state: "STOPPED", recordedSeconds: 1, lostFrames: 0, series: [
          { id: 17, times: [-1, -0.5, 0], values: [0, 0.5, 0.2] },
        ],
      },
      recordedSeconds: 1, windowSeconds: 0.2, endOffsetSeconds: 0,
    };
  }
  if (command === "motion_get") throw new Error("device is not connected");
  throw new Error(`Unexpected IPC during disconnected startup: ${command}`);
}, { shouldMockEvents: true });

if (fixture.has("tuning-tabs")) {
  await import("@vscode-elements/elements/dist/bundled.js");
  await import("../src/theme.css");
  const { mount } = await import("svelte");
  const { default: ControlTuningPage } = await import("../src/control/ControlTuningPage.svelte");
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
