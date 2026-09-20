import type uPlot from "uplot";

export type ScopeNavigationPluginOptions = {
  panButton?: number;
  bounds: () => [number, number];
  wheelRange: (currentRange: number, direction: -1 | 1) => number;
  blockPan?: (u: uPlot, event: MouseEvent) => boolean;
  blockWheel?: (u: uPlot, event: WheelEvent) => boolean;
  onInteractionStart?: () => void;
  onViewChange?: (min: number, max: number, committed: boolean) => void;
};

/**
 * X-axis navigation based on uPlot's official zoom-wheel demo.
 *
 * uPlot owns the pointer/scale mechanics here. NMIXX only supplies product
 * policy: history bounds, oscilloscope timebase steps and view-state callbacks.
 *
 * Reference:
 * https://github.com/leeoniya/uPlot/blob/master/demos/zoom-wheel.html
 */
export function scopeNavigationPlugin(options: ScopeNavigationPluginOptions): uPlot.Plugin {
  const panButton = options.panButton ?? 0;

  function clamp(min: number, max: number): [number, number] {
    const [fullMin, fullMax] = options.bounds();
    const fullRange = fullMax - fullMin;
    const range = max - min;

    if (range >= fullRange) return [fullMin, fullMax];
    if (min < fullMin) return [fullMin, fullMin + range];
    if (max > fullMax) return [fullMax - range, fullMax];
    return [min, max];
  }

  return {
    hooks: {
      ready: [
        (u) => {
          const over = u.over;

          over.addEventListener("mousedown", (event) => {
            if (event.button !== panButton || options.blockPan?.(u, event)) return;

            const min0 = u.scales.x.min;
            const max0 = u.scales.x.max;
            if (min0 === undefined || max0 === undefined) return;

            event.preventDefault();
            options.onInteractionStart?.();

            const left0 = event.clientX;
            const unitsPerPx = u.posToVal(1, "x") - u.posToVal(0, "x");

            const onMove = (moveEvent: MouseEvent) => {
              moveEvent.preventDefault();
              const dx = unitsPerPx * (moveEvent.clientX - left0);
              const [min, max] = clamp(min0 - dx, max0 - dx);
              u.setScale("x", { min, max });
              options.onViewChange?.(min, max, false);
            };

            const onUp = () => {
              document.removeEventListener("mousemove", onMove);
              document.removeEventListener("mouseup", onUp);
              const min = u.scales.x.min;
              const max = u.scales.x.max;
              if (min !== undefined && max !== undefined) {
                options.onViewChange?.(min, max, true);
              }
            };

            document.addEventListener("mousemove", onMove);
            document.addEventListener("mouseup", onUp);
          });

          over.addEventListener(
            "wheel",
            (event) => {
              if (event.deltaY === 0 || options.blockWheel?.(u, event)) return;

              const min0 = u.scales.x.min;
              const max0 = u.scales.x.max;
              if (min0 === undefined || max0 === undefined) return;

              event.preventDefault();

              const rect = over.getBoundingClientRect();
              const left = event.clientX - rect.left;
              const leftPct = Math.min(Math.max(left / Math.max(rect.width, 1), 0), 1);
              const xValue = u.posToVal(left, "x");
              const currentRange = max0 - min0;
              const direction: -1 | 1 = event.deltaY < 0 ? -1 : 1;
              const nextRange = options.wheelRange(currentRange, direction);
              if (!Number.isFinite(nextRange) || nextRange <= 0 || nextRange === currentRange) return;

              let min = xValue - leftPct * nextRange;
              let max = min + nextRange;
              [min, max] = clamp(min, max);

              u.setScale("x", { min, max });
              options.onViewChange?.(min, max, true);
            },
            { passive: false },
          );
        },
      ],
    },
  };
}
