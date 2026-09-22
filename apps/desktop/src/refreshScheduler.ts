export type RefreshSubscription = {
  intervalMs: number;
  callback: () => void;
};

type RuntimeSubscription = RefreshSubscription & {
  lastRun: number;
};

const subscriptions = new Set<RuntimeSubscription>();
let frameHandle: number | undefined;

function runFrame(now: number) {
  for (const subscription of subscriptions) {
    if (now - subscription.lastRun < subscription.intervalMs) continue;
    subscription.lastRun = now;
    subscription.callback();
  }

  if (subscriptions.size > 0) {
    frameHandle = requestAnimationFrame(runFrame);
  } else {
    frameHandle = undefined;
  }
}

function ensureRunning() {
  if (frameHandle !== undefined || subscriptions.size === 0) return;
  frameHandle = requestAnimationFrame(runFrame);
}

/**
 * Shared desktop refresh scheduler.
 *
 * requestAnimationFrame is the only periodic UI clock (~60 FPS). Expensive
 * Tauri/device reads register a slower interval and are dispatched from that
 * frame clock instead of creating independent setInterval loops.
 */
export function subscribeRefresh(
  intervalMs: number,
  callback: () => void,
): () => void {
  const subscription: RuntimeSubscription = {
    intervalMs: Math.max(0, intervalMs),
    callback,
    lastRun: performance.now() - Math.max(0, intervalMs),
  };
  subscriptions.add(subscription);
  ensureRunning();

  return () => {
    subscriptions.delete(subscription);
    if (subscriptions.size === 0 && frameHandle !== undefined) {
      cancelAnimationFrame(frameHandle);
      frameHandle = undefined;
    }
  };
}
