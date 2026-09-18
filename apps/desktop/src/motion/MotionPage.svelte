<script lang="ts">
  import { motionState } from "./store";
  import type { MotionMode, MotionState, SCurveMode, TrajectoryType } from "./types";

  const modeLabels: Record<MotionMode, string> = {
    position: "Position",
    speed: "Speed",
    "sensorless-speed": "Sensorless Speed",
    torque: "Torque",
    mit: "MIT",
  };

  const trajectoryLabels: Record<TrajectoryType, string> = {
    trapezoidal: "T / Trapezoidal",
    "s-curve": "S-curve",
    filtered: "Filtered",
  };

  function numberValue(event: Event): number {
    return Number((event.currentTarget as HTMLInputElement).value);
  }

  function update<K extends keyof MotionState>(key: K, value: MotionState[K]) {
    motionState.update((state) => ({ ...state, [key]: value }));
  }

  function hasTrajectory(): boolean {
    return $motionState.mode === "position" || $motionState.mode === "speed" || $motionState.mode === "sensorless-speed";
  }

  function previewPath(type: TrajectoryType): string {
    if (type === "s-curve" && $motionState.sCurveMode === "peak-accel") {
      return "M24 172 C72 172 86 158 110 126 C140 88 156 52 194 38 L286 38 C324 52 340 88 370 126 C394 158 408 172 456 172";
    }
    if (type === "s-curve") {
      return "M24 172 C50 172 70 150 90 116 C110 82 134 48 180 38 L300 38 C346 48 370 82 390 116 C410 150 430 172 456 172";
    }
    if (type === "filtered") {
      return "M24 172 C82 172 98 156 116 126 C138 92 156 54 204 42 C250 30 310 40 340 64 C372 90 390 132 408 152 C424 168 438 172 456 172";
    }
    return "M24 172 L116 172 L188 38 L292 38 L364 172 L456 172";
  }

  function trajectoryNote(): string {
    if ($motionState.trajectory === "trapezoidal") {
      return "Acc / Dec are constant acceleration and deceleration magnitudes.";
    }
    if ($motionState.trajectory === "filtered") {
      return "Acc / Dec define the base ramp; Filter Time smooths the command edges.";
    }
    if ($motionState.sCurveMode === "peak-accel") {
      return "Acc / Dec are the maximum S-curve acceleration magnitudes. Total accel/decel time is longer than T-profile with the same values.";
    }
    return "Acc / Dec preserve the T-profile accel/decel time. Internal S-curve peak acceleration is therefore higher.";
  }

  function previewLabel(): string {
    if ($motionState.mode === "position") return `${$motionState.positionTargetTurn.toFixed(3)} turn`;
    if ($motionState.mode === "speed") return `${$motionState.speedTarget.toFixed(2)} rad/s`;
    if ($motionState.mode === "sensorless-speed") return `${$motionState.sensorlessSpeedTarget.toFixed(2)} rad/s`;
    if ($motionState.mode === "torque") return `${$motionState.torqueTargetNm.toFixed(3)} N·m`;
    return `q ${$motionState.mitPositionRef.toFixed(3)} · dq ${$motionState.mitVelocityRef.toFixed(2)}`;
  }
</script>

<section class="page-toolbar">
  <div class="page-title">MOTION</div>
</section>

<div class="motion-simple-page">
  <div class="motion-mode-row">
    <label>
      <span>Mode</span>
      <select value={$motionState.mode} onchange={(event) => update("mode", (event.currentTarget as HTMLSelectElement).value as MotionMode)}>
        {#each Object.entries(modeLabels) as [value, label]}
          <option value={value}>{label}</option>
        {/each}
      </select>
    </label>
  </div>

  <div class="motion-simple-body">
    <div class="motion-simple-left">
      <section class="motion-panel">
        <div class="motion-panel-title">Command</div>
        <div class="motion-panel-body motion-grid">
          {#if $motionState.mode === "position"}
            <div class="motion-segment span-2">
              <button class:active={$motionState.positionCommand === "absolute"} onclick={() => update("positionCommand", "absolute")}>Absolute</button>
              <button class:active={$motionState.positionCommand === "incremental"} onclick={() => update("positionCommand", "incremental")}>Incremental</button>
            </div>
            <label class="motion-field">
              <span>Target position</span>
              <div class="unit-field"><input type="number" value={$motionState.positionTargetTurn} oninput={(e) => update("positionTargetTurn", numberValue(e))} /><em>turn</em></div>
            </label>
            <label class="motion-field">
              <span>Max speed</span>
              <div class="unit-field"><input type="number" value={$motionState.positionMaxSpeed} oninput={(e) => update("positionMaxSpeed", numberValue(e))} /><em>rad/s</em></div>
            </label>
          {:else if $motionState.mode === "speed"}
            <label class="motion-field span-2">
              <span>Target speed</span>
              <div class="unit-field"><input type="number" value={$motionState.speedTarget} oninput={(e) => update("speedTarget", numberValue(e))} /><em>rad/s</em></div>
            </label>
          {:else if $motionState.mode === "sensorless-speed"}
            <label class="motion-field span-2">
              <span>Target speed</span>
              <div class="unit-field"><input type="number" value={$motionState.sensorlessSpeedTarget} oninput={(e) => update("sensorlessSpeedTarget", numberValue(e))} /><em>rad/s</em></div>
            </label>
            <label class="motion-field">
              <span>Startup current</span>
              <div class="unit-field"><input type="number" value={$motionState.sensorlessStartupCurrent} oninput={(e) => update("sensorlessStartupCurrent", numberValue(e))} /><em>A</em></div>
            </label>
            <label class="motion-field">
              <span>Entry speed</span>
              <div class="unit-field"><input type="number" value={$motionState.sensorlessEntrySpeed} oninput={(e) => update("sensorlessEntrySpeed", numberValue(e))} /><em>rad/s</em></div>
            </label>
          {:else if $motionState.mode === "torque"}
            <label class="motion-field">
              <span>Target torque</span>
              <div class="unit-field"><input type="number" value={$motionState.torqueTargetNm} oninput={(e) => update("torqueTargetNm", numberValue(e))} /><em>N·m</em></div>
            </label>
            <label class="motion-field">
              <span>Torque ramp</span>
              <div class="unit-field"><input type="number" value={$motionState.torqueRampNmPerS} oninput={(e) => update("torqueRampNmPerS", numberValue(e))} /><em>N·m/s</em></div>
            </label>
          {:else}
            <label class="motion-field">
              <span>Position ref</span>
              <div class="unit-field"><input type="number" value={$motionState.mitPositionRef} oninput={(e) => update("mitPositionRef", numberValue(e))} /><em>turn</em></div>
            </label>
            <label class="motion-field">
              <span>Velocity ref</span>
              <div class="unit-field"><input type="number" value={$motionState.mitVelocityRef} oninput={(e) => update("mitVelocityRef", numberValue(e))} /><em>rad/s</em></div>
            </label>
            <label class="motion-field"><span>Kp</span><input type="number" value={$motionState.mitKp} oninput={(e) => update("mitKp", numberValue(e))} /></label>
            <label class="motion-field"><span>Kd</span><input type="number" value={$motionState.mitKd} oninput={(e) => update("mitKd", numberValue(e))} /></label>
            <label class="motion-field span-2">
              <span>Torque feedforward</span>
              <div class="unit-field"><input type="number" value={$motionState.mitTorqueFeedforward} oninput={(e) => update("mitTorqueFeedforward", numberValue(e))} /><em>N·m</em></div>
            </label>
          {/if}
        </div>
      </section>

      {#if hasTrajectory()}
        <section class="motion-panel">
          <div class="motion-panel-title">Trajectory</div>
          <div class="motion-panel-body motion-grid">
            <label class="motion-field span-2">
              <span>Profile</span>
              <select value={$motionState.trajectory} onchange={(event) => update("trajectory", (event.currentTarget as HTMLSelectElement).value as TrajectoryType)}>
                {#each Object.entries(trajectoryLabels) as [value, label]}
                  <option value={value}>{label}</option>
                {/each}
              </select>
            </label>

            {#if $motionState.trajectory === "s-curve"}
              <div class="motion-field span-2">
                <span>S-curve mode</span>
                <div class="motion-segment">
                  <button class:active={$motionState.sCurveMode === "peak-accel"} onclick={() => update("sCurveMode", "peak-accel" as SCurveMode)}>Peak Accel</button>
                  <button class:active={$motionState.sCurveMode === "matched-time"} onclick={() => update("sCurveMode", "matched-time" as SCurveMode)}>Matched Time</button>
                </div>
              </div>
            {/if}

            <label class="motion-field">
              <span>{$motionState.trajectory === "s-curve" && $motionState.sCurveMode === "matched-time" ? "Equivalent Acc" : "Acceleration"}</span>
              <div class="unit-field"><input type="number" value={$motionState.acceleration} oninput={(e) => update("acceleration", numberValue(e))} /><em>rad/s²</em></div>
            </label>
            <label class="motion-field">
              <span>{$motionState.trajectory === "s-curve" && $motionState.sCurveMode === "matched-time" ? "Equivalent Dec" : "Deceleration"}</span>
              <div class="unit-field"><input type="number" value={$motionState.deceleration} oninput={(e) => update("deceleration", numberValue(e))} /><em>rad/s²</em></div>
            </label>

            {#if $motionState.trajectory === "filtered"}
              <label class="motion-field span-2">
                <span>Filter time</span>
                <div class="unit-field"><input type="number" value={$motionState.filterTimeMs} oninput={(e) => update("filterTimeMs", numberValue(e))} /><em>ms</em></div>
              </label>
            {/if}

            <label class="motion-check span-2">
              <input type="checkbox" checked={$motionState.repeat} onchange={(e) => update("repeat", (e.currentTarget as HTMLInputElement).checked)} />
              <span>Repeat</span>
            </label>

            <div class="trajectory-note span-2">{trajectoryNote()}</div>
          </div>
        </section>
      {/if}

      <div class="motion-runbar">
        <button class="motion-run" disabled title="Application Motion API is not connected yet"><i class="codicon codicon-debug-start"></i> Run</button>
        <button disabled title="Application Motion API is not connected yet"><i class="codicon codicon-debug-stop"></i> Stop</button>
      </div>
    </div>

    <div class="motion-simple-right">
      <div class="motion-plot-header">
        <div>
          <strong>{modeLabels[$motionState.mode]}</strong>
          {#if hasTrajectory()}<span>{trajectoryLabels[$motionState.trajectory]}</span>{/if}
        </div>
        <div class="preview-target">{previewLabel()}</div>
      </div>

      <div class="motion-plot-area">
        {#if hasTrajectory()}
          <svg viewBox="0 0 480 220" role="img" aria-label="Trajectory profile preview">
            <line x1="24" y1="172" x2="456" y2="172" class="preview-axis" />
            <line x1="24" y1="24" x2="24" y2="172" class="preview-axis" />
            <path d={previewPath($motionState.trajectory)} class="preview-curve" />
          </svg>
          <div class="motion-plot-note">{trajectoryNote()}</div>
        {:else}
          <div class="motion-plot-empty">This mode has no position/speed trajectory preview.</div>
        {/if}
      </div>
    </div>
  </div>
</div>
