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
mockIPC((command, args) => {
  window.__nmixxStartupRequests.push(command);
  window.__nmixxStartupArgs.push({ command, args });
  if (command === "device_list" || command === "action_list") return [];
  if (command === "device_disconnect") return;
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
  await import("@vscode-elements/elements/dist/bundled.js");
  await import("../src/theme.css");
  const { mount } = await import("svelte");
  const { default: AutomationPage } = await import("../src/automation/AutomationPage.svelte");
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
