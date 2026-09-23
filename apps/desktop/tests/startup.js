import { mockIPC } from "@tauri-apps/api/mocks";

// This fixture never opens a device. Use the SDK's event mock rather than
// implementing a competing Tauri transport. The runner checks every command.
window.__nmixxStartupRequests = [];
mockIPC((command) => {
  window.__nmixxStartupRequests.push(command);
  if (command === "device_list" || command === "action_list") return [];
  if (command === "device_disconnect") return;
  if (command === "motion_get") throw new Error("device is not connected");
  throw new Error(`Unexpected IPC during disconnected startup: ${command}`);
}, { shouldMockEvents: true });

if (new URLSearchParams(location.search).has("preview")) {
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
