<script lang="ts">
  import { onMount } from "svelte";
  import { initializeMotion, motionState, updateMotion } from "../../motion/store";
  import type { MotionMode, MotionState, TrajectoryType } from "../../motion/types";

  export let onError: (error: unknown) => void = () => undefined;

  const modeLabels: Record<MotionMode, string> = {
    position: "Position",
    speed: "Speed",
    "sensorless-speed": "Sensorless Speed",
    torque: "Torque",
    mit: "MIT",
  };

  const trajectoryLabels: Record<TrajectoryType, string> = {
    trapezoidal: "T",
    "s-curve": "S",
    filtered: "Filtered",
  };

  function numberValue(event: Event): number {
    return Number((event.currentTarget as HTMLInputElement).value);
  }

  function update<K extends keyof MotionState>(key: K, value: MotionState[K]) {
    void updateMotion(key, value).catch(onError);
  }

  function hasTrajectory(): boolean {
    return $motionState.mode === "position"
      || $motionState.mode === "speed"
      || $motionState.mode === "sensorless-speed";
  }

  onMount(() => {
    void initializeMotion().catch(onError);
  });
</script>

<section class="side-section tuning-motion-section">
  <div class="section-heading">MOTION</div>
  <div class="tuning-motion-form">
    <label>
      <span>Mode</span>
      <select
        value={$motionState.mode}
        onchange={(event) => update("mode", (event.currentTarget as HTMLSelectElement).value as MotionMode)}
      >
        {#each Object.entries(modeLabels) as [value, label]}
          <option value={value}>{label}</option>
        {/each}
      </select>
    </label>

    {#if $motionState.mode === "position"}
      <label>
        <span>Target</span>
        <div class="compact-unit-field">
          <input type="number" value={$motionState.positionTargetTurn} oninput={(e) => update("positionTargetTurn", numberValue(e))} />
          <em>turn</em>
        </div>
      </label>
      <label>
        <span>Speed</span>
        <div class="compact-unit-field">
          <input type="number" value={$motionState.positionMaxSpeed} oninput={(e) => update("positionMaxSpeed", numberValue(e))} />
          <em>rad/s</em>
        </div>
      </label>
    {:else if $motionState.mode === "speed"}
      <label>
        <span>Target</span>
        <div class="compact-unit-field">
          <input type="number" value={$motionState.speedTarget} oninput={(e) => update("speedTarget", numberValue(e))} />
          <em>rad/s</em>
        </div>
      </label>
    {:else if $motionState.mode === "sensorless-speed"}
      <label>
        <span>Target</span>
        <div class="compact-unit-field">
          <input type="number" value={$motionState.sensorlessSpeedTarget} oninput={(e) => update("sensorlessSpeedTarget", numberValue(e))} />
          <em>rad/s</em>
        </div>
      </label>
    {:else if $motionState.mode === "torque"}
      <label>
        <span>Target</span>
        <div class="compact-unit-field">
          <input type="number" value={$motionState.torqueTargetNm} oninput={(e) => update("torqueTargetNm", numberValue(e))} />
          <em>N·m</em>
        </div>
      </label>
      <label>
        <span>Ramp</span>
        <div class="compact-unit-field">
          <input type="number" value={$motionState.torqueRampNmPerS} oninput={(e) => update("torqueRampNmPerS", numberValue(e))} />
          <em>N·m/s</em>
        </div>
      </label>
    {:else}
      <label>
        <span>Position</span>
        <div class="compact-unit-field">
          <input type="number" value={$motionState.mitPositionRef} oninput={(e) => update("mitPositionRef", numberValue(e))} />
          <em>turn</em>
        </div>
      </label>
      <label>
        <span>Velocity</span>
        <div class="compact-unit-field">
          <input type="number" value={$motionState.mitVelocityRef} oninput={(e) => update("mitVelocityRef", numberValue(e))} />
          <em>rad/s</em>
        </div>
      </label>
      <div class="tuning-motion-pair">
        <label><span>Kp</span><input type="number" value={$motionState.mitKp} oninput={(e) => update("mitKp", numberValue(e))} /></label>
        <label><span>Kd</span><input type="number" value={$motionState.mitKd} oninput={(e) => update("mitKd", numberValue(e))} /></label>
      </div>
      <label>
        <span>Torque FF</span>
        <div class="compact-unit-field">
          <input type="number" value={$motionState.mitTorqueFeedforward} oninput={(e) => update("mitTorqueFeedforward", numberValue(e))} />
          <em>N·m</em>
        </div>
      </label>
    {/if}

    {#if hasTrajectory()}
      <label>
        <span>Profile</span>
        <select
          value={$motionState.trajectory}
          onchange={(event) => update("trajectory", (event.currentTarget as HTMLSelectElement).value as TrajectoryType)}
        >
          {#each Object.entries(trajectoryLabels) as [value, label]}
            <option value={value}>{label}</option>
          {/each}
        </select>
      </label>

      <div class="tuning-motion-pair">
        <label>
          <span>Acc</span>
          <div class="compact-unit-field">
            <input type="number" value={$motionState.acceleration} oninput={(e) => update("acceleration", numberValue(e))} />
            <em>rad/s²</em>
          </div>
        </label>
        <label>
          <span>Dec</span>
          <div class="compact-unit-field">
            <input type="number" value={$motionState.deceleration} oninput={(e) => update("deceleration", numberValue(e))} />
            <em>rad/s²</em>
          </div>
        </label>
      </div>
    {/if}

    <div class="tuning-motion-actions">
      <button disabled title="Application Motion execution is not connected yet">Run</button>
      <button disabled title="Application Motion execution is not connected yet">Stop</button>
    </div>
  </div>
</section>
