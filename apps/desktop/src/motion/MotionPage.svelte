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

  function previewPath(type: TrajectoryType): string {
    if (type === "s-curve" && $motionState.sCurveMode === "peak-accel") {
      return "M20 150 C58 150 68 140 88 118 C116 88 132 54 166 42 L234 42 C268 54 284 88 312 118 C332 140 342 150 380 150";
    }
    if (type === "s-curve") {
      return "M20 150 C44 150 60 132 78 102 C94 74 112 50 150 42 L250 42 C288 50 306 74 322 102 C340 132 356 150 380 150";
    }
    if (type === "filtered") return "M20 150 C68 150 82 136 96 112 C114 82 128 54 164 44 C202 33 248 40 274 58 C302 78 316 116 330 134 C343 148 356 150 380 150";
    return "M20 150 L92 150 L150 42 L252 42 L310 150 L380 150";
  }

  function trajectoryNote(): string {
    if ($motionState.trajectory === "trapezoidal") {
      return "Acceleration and deceleration are constant ramp magnitudes.";
    }
    if ($motionState.trajectory === "filtered") {
      return "Acceleration and deceleration define the base ramp; filter time smooths the command edges.";
    }
    if ($motionState.sCurveMode === "peak-accel") {
      return "Acc / Dec are the maximum S-curve acceleration magnitudes. Because acceleration ramps in and out smoothly, total acceleration and deceleration time is longer than a trapezoidal profile with the same values.";
    }
    return "Acc / Dec define the equivalent trapezoidal ramp timing. The S-curve keeps the same acceleration and deceleration duration, so its internal peak acceleration is higher than the trapezoidal profile.";
  }

  function targetLabel(): string {
    if ($motionState.mode === "position") return `${$motionState.positionTargetTurn.toFixed(3)} turn`;
    if ($motionState.mode === "speed") return `${$motionState.speedTarget.toFixed(2)} rad/s`;
    if ($motionState.mode === "sensorless-speed") return `${$motionState.sensorlessSpeedTarget.toFixed(2)} rad/s`;
    if ($motionState.mode === "torque") return `${$motionState.torqueTargetNm.toFixed(3)} N·m`;
    return `q ${$motionState.mitPositionRef.toFixed(3)} · dq ${$motionState.mitVelocityRef.toFixed(2)}`;
  }
</script>

<section class="page-toolbar">
  <div class="page-title">MOTION</div>
  <div class="toolbar-actions motion-toolbar-note">Shared with Control Tuning</div>
</section>

<div class="motion-page">
  <div class="motion-column motion-editor">
    <section class="motion-card">
      <div class="motion-card-title">Operating mode</div>
      <div class="motion-card-body">
        <label class="motion-field">
          <span>Mode</span>
          <select value={$motionState.mode} onchange={(event) => update("mode", (event.currentTarget as HTMLSelectElement).value as MotionMode)}>
            {#each Object.entries(modeLabels) as [value, label]}
              <option value={value}>{label}</option>
            {/each}
          </select>
        </label>
      </div>
    </section>

    {#if $motionState.mode === "position" || $motionState.mode === "speed" || $motionState.mode === "sensorless-speed"}
    <section class="motion-card">
      <div class="motion-card-title">Trajectory</div>
      <div class="motion-card-body motion-grid">
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
            <span>S-curve semantics</span>
            <div class="motion-segment">
              <button class:active={$motionState.sCurveMode === "peak-accel"} onclick={() => update("sCurveMode", "peak-accel" as SCurveMode)}>Peak Accel</button>
              <button class:active={$motionState.sCurveMode === "matched-time"} onclick={() => update("sCurveMode", "matched-time" as SCurveMode)}>Matched Time</button>
            </div>
          </div>
        {/if}

        <label class="motion-field">
          <span>{$motionState.trajectory === "s-curve" && $motionState.sCurveMode === "matched-time" ? "Equivalent acceleration" : "Acceleration"}</span>
          <div class="unit-field"><input type="number" value={$motionState.acceleration} oninput={(e) => update("acceleration", numberValue(e))} /><em>rad/s²</em></div>
        </label>
        <label class="motion-field">
          <span>{$motionState.trajectory === "s-curve" && $motionState.sCurveMode === "matched-time" ? "Equivalent deceleration" : "Deceleration"}</span>
          <div class="unit-field"><input type="number" value={$motionState.deceleration} oninput={(e) => update("deceleration", numberValue(e))} /><em>rad/s²</em></div>
        </label>

        {#if $motionState.trajectory === "filtered"}
          <label class="motion-field span-2">
            <span>Filter time</span>
            <div class="unit-field"><input type="number" value={$motionState.filterTimeMs} oninput={(e) => update("filterTimeMs", numberValue(e))} /><em>ms</em></div>
          </label>
        {/if}

        <div class="trajectory-note span-2">{trajectoryNote()}</div>

        <label class="motion-check span-2">
          <input type="checkbox" checked={$motionState.repeat} onchange={(e) => update("repeat", (e.currentTarget as HTMLInputElement).checked)} />
          <span>Repeat command</span>
        </label>
      </div>
    </section>
    {/if}

    <section class="motion-card motion-command-card">
      <div class="motion-card-title">{modeLabels[$motionState.mode]} command</div>
      <div class="motion-card-body motion-grid">
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
  </div>

  <div class="motion-column motion-preview-column">
    <section class="motion-card motion-preview-card">
      <div class="motion-card-title">Trajectory preview</div>
      <div class="motion-preview">
        <div class="preview-heading">
          <div>
            <strong>{modeLabels[$motionState.mode]}</strong>
            <span>{trajectoryLabels[$motionState.trajectory]}</span>
          </div>
          <div class="preview-target">{targetLabel()}</div>
        </div>
        <svg viewBox="0 0 400 190" role="img" aria-label="Trajectory profile preview">
          <line x1="20" y1="150" x2="380" y2="150" class="preview-axis" />
          <line x1="20" y1="20" x2="20" y2="150" class="preview-axis" />
          <path d={previewPath($motionState.trajectory)} class="preview-curve" />
        </svg>
        <div class="preview-caption">{trajectoryNote()}</div>
        <div class="preview-caption preview-secondary">Preview describes command shaping only. Live response belongs to Analysis / Control Tuning.</div>
      </div>
    </section>

    <section class="motion-card">
      <div class="motion-card-title">Execution</div>
      <div class="motion-execution">
        <div class="execution-row"><span>Run</span><strong>Uses selected trajectory</strong></div>
        <div class="execution-row"><span>Stop</span><strong>Controlled stop using trajectory / deceleration</strong></div>
        <div class="execution-row"><span>Disable</span><strong>Immediate drive disable · separate safety action</strong></div>
        <div class="motion-actions">
          <button class="motion-run" disabled title="Application Motion API is not connected yet"><i class="codicon codicon-debug-start"></i> Run</button>
          <button disabled title="Application Motion API is not connected yet"><i class="codicon codicon-debug-stop"></i> Stop</button>
        </div>
        <div class="motion-api-note">GUI state is ready for the shared Motion model; device execution remains intentionally disabled until the Application API owns Run / Stop.</div>
      </div>
    </section>
  </div>
</div>
