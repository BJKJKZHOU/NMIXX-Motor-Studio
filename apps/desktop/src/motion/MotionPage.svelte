<script lang="ts">
  import { onMount } from "svelte";
  import type { MotionCapabilities } from "../connection/types";
  import {
    listParameters, onParametersRefreshed, readCachedParameters, readCurrentParameters,
    readParameter, writeParameter,
  } from "../parameters/api";
  import { modifiedParameterIds } from "../parameters/persistence";
  import type { ParameterMetadata, ParameterReadResult, ParameterValue } from "../parameters/types";
  import {
    MOTION_ACCEL, MOTION_DECEL, MOTION_MAX_SPEED, MOTION_MODE, MOTION_PARAMETER_SYMBOLS,
    SENSORLESS_ENTRY_SPEED, SENSORLESS_STARTUP_CURRENT, TARGET_POSITION, TARGET_SPEED,
    TARGET_TORQUE, TORQUE_RAMP, modeFromParameter, modeParameterValue,
    motionParameterText, parseMotionParameter,
  } from "./parameters";
  import {
    executeMotion, initializeMotion, motionPreview, motionState, refreshMotionPreview,
    stopMotionExecution, updateMotion,
  } from "./store";
  import MotionTrajectoryPlot from "./MotionTrajectoryPlot.svelte";
  import type { MotionMode } from "./types";

  export let capabilities: MotionCapabilities | undefined;
  export let motorState: number | null = null;
  export let onError: (error: unknown) => void = () => undefined;

  const MOTOR_DISABLED = 0;
  const MOTOR_ENABLED = 1;
  const MOTOR_RUN = 2;

  const modeLabels: Record<MotionMode, string> = {
    position: "Position",
    speed: "Speed",
    "sensorless-speed": "Sensorless Speed",
    torque: "Torque",
  };

  let metadata: Record<string, ParameterMetadata> = {};
  let values: Record<string, ParameterValue | null> = {};
  let drafts: Record<string, string> = {};
  let writing = new Set<string>();
  let actionBusy = false;
  let incrementalDraft = "1";

  $: activeMode = modeFromParameter(metadata[MOTION_MODE], values[MOTION_MODE]);
  $: hasTrajectory = activeMode === "position" || activeMode === "speed" || activeMode === "sensorless-speed";

  onMount(() => {
    let unlisten: (() => void) | undefined;
    void initialize().catch(onError);
    onParametersRefreshed(() => void refreshFromCache())
      .then((stop) => unlisten = stop)
      .catch(onError);
    return () => unlisten?.();
  });

  async function initialize() {
    await initializeMotion();
    incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn);
    await loadDeviceParameters();
  }

  function formatHostNumber(value: number): string {
    return Number(value).toPrecision(9).replace(/(?:\.0+|(\.\d+?)0+)$/, "$1");
  }

  function applyResults(results: ParameterReadResult[]) {
    const byId = new Map(results.map((item) => [item.id, item]));
    const nextValues = { ...values };
    const nextDrafts = { ...drafts };
    for (const meta of Object.values(metadata)) {
      const result = byId.get(meta.id);
      if (!result) continue;
      nextValues[meta.symbol] = result.value ?? null;
      if (result.value) nextDrafts[meta.symbol] = motionParameterText(meta, result.value);
    }
    values = nextValues;
    drafts = nextDrafts;
  }

  async function loadDeviceParameters() {
    const registry = await listParameters();
    const wanted = new Set<string>(MOTION_PARAMETER_SYMBOLS);
    const entries = registry.filter((item) => wanted.has(item.symbol));
    metadata = Object.fromEntries(entries.map((item) => [item.symbol, item]));
    const readable = entries.filter((item) => item.access.toLowerCase().includes("r"));
    applyResults(await readCurrentParameters(readable.map((item) => item.id)));
    await refreshMotionPreview();
  }

  async function refreshFromCache() {
    const readable = Object.values(metadata).filter((item) => item.access.toLowerCase().includes("r"));
    if (readable.length === 0) return;
    applyResults(await readCachedParameters(readable.map((item) => item.id)));
    await refreshMotionPreview();
  }

  function writable(symbol: string): boolean {
    return metadata[symbol]?.access.toLowerCase().includes("w") ?? false;
  }

  function dirty(symbol: string): boolean {
    const meta = metadata[symbol];
    return !!meta && (drafts[symbol] ?? "") !== motionParameterText(meta, values[symbol]);
  }

  function resetDraft(symbol: string) {
    const meta = metadata[symbol];
    if (!meta) return;
    drafts = { ...drafts, [symbol]: motionParameterText(meta, values[symbol]) };
  }

  async function commitParameter(symbol: string) {
    const meta = metadata[symbol];
    if (!meta || !writable(symbol) || writing.has(symbol) || !dirty(symbol)) return;
    writing = new Set(writing).add(symbol);
    try {
      await writeParameter(meta.id, parseMotionParameter(meta, drafts[symbol] ?? ""));
      const result = await readParameter(meta.id);
      values = { ...values, [symbol]: result.value };
      drafts = { ...drafts, [symbol]: motionParameterText(meta, result.value) };
      await refreshMotionPreview();
    } catch (error) {
      resetDraft(symbol);
      onError(error);
    } finally {
      const next = new Set(writing);
      next.delete(symbol);
      writing = next;
    }
  }

  function parameterKeydown(event: KeyboardEvent, symbol: string) {
    if (event.key === "Enter") {
      event.preventDefault();
      void commitParameter(symbol);
      (event.currentTarget as HTMLInputElement).blur();
    } else if (event.key === "Escape") {
      event.preventDefault();
      resetDraft(symbol);
      (event.currentTarget as HTMLInputElement).blur();
    }
  }

  async function setMode(mode: MotionMode) {
    const meta = metadata[MOTION_MODE];
    const value = modeParameterValue(meta, mode);
    if (!meta || value === undefined || motorState !== MOTOR_DISABLED || writing.has(MOTION_MODE)) return;
    writing = new Set(writing).add(MOTION_MODE);
    try {
      await writeParameter(meta.id, { type: "u8", value });
      const result = await readParameter(meta.id);
      values = { ...values, [MOTION_MODE]: result.value };
      drafts = { ...drafts, [MOTION_MODE]: motionParameterText(meta, result.value) };
      await refreshMotionPreview();
    } catch (error) {
      onError(error);
    } finally {
      const next = new Set(writing);
      next.delete(MOTION_MODE);
      writing = next;
    }
  }

  function modeSupported(mode: MotionMode): boolean {
    if (!capabilities) return false;
    if (mode === "position") return capabilities.position;
    if (mode === "speed") return capabilities.speed;
    if (mode === "sensorless-speed") return capabilities.sensorlessSpeed;
    return capabilities.torque;
  }

  async function setPositionCommand(command: "absolute" | "incremental") {
    try {
      await updateMotion("positionCommand", command);
      incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn);
    } catch (error) { onError(error); }
  }

  async function setRepeat(checked: boolean) {
    try { await updateMotion("repeat", checked); }
    catch (error) { onError(error); }
  }

  async function commitIncremental() {
    const parsed = Number(incrementalDraft.trim());
    if (!Number.isFinite(parsed)) {
      incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn);
      onError("Delta position must be finite.");
      return;
    }
    try {
      await updateMotion("incrementalDeltaTurn", parsed);
      incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn);
    } catch (error) {
      incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn);
      onError(error);
    }
  }

  function incrementalKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      void commitIncremental();
      (event.currentTarget as HTMLInputElement).blur();
    } else if (event.key === "Escape") {
      event.preventDefault();
      incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn);
      (event.currentTarget as HTMLInputElement).blur();
    }
  }

  function numericValue(symbol: string): number | null {
    const value = values[symbol];
    if (!value || value.type === "position") return null;
    return Number(value.value);
  }

  function canRun(): boolean {
    return !!capabilities?.run && motorState === MOTOR_ENABLED && !!activeMode && modeSupported(activeMode);
  }

  function canStop(): boolean {
    return !!capabilities?.stop && motorState === MOTOR_RUN;
  }

  async function run() {
    if (!canRun() || actionBusy) return;
    actionBusy = true;
    try { await executeMotion(); }
    catch (error) { onError(error); }
    finally { actionBusy = false; }
  }

  async function stop() {
    if (!canStop() || actionBusy) return;
    actionBusy = true;
    try { await stopMotionExecution(); }
    catch (error) { onError(error); }
    finally { actionBusy = false; }
  }

  function previewLabel(): string {
    if (activeMode === "position") {
      return $motionState.positionCommand === "incremental"
        ? \`\${$motionState.incrementalDeltaTurn.toFixed(3)} turn delta\`
        : \`\${drafts[TARGET_POSITION] || "—"} turn\`;
    }
    if (activeMode === "speed" || activeMode === "sensorless-speed") {
      const value = numericValue(TARGET_SPEED);
      return value === null ? "—" : \`\${value.toFixed(2)} rad/s\`;
    }
    if (activeMode === "torque") {
      const value = numericValue(TARGET_TORQUE);
      return value === null ? "—" : \`\${value.toFixed(3)} N·m\`;
    }
    return "—";
  }
</script>

<div class="motion-simple-page">
  <div class="motion-mode-row">
    <label>
      <span>Mode</span>
      <select
        value={activeMode ?? ""}
        disabled={motorState !== MOTOR_DISABLED || writing.has(MOTION_MODE)}
        title={motorState === MOTOR_DISABLED ? "Select motor mode" : "Disable the motor before changing mode"}
        onchange={(event) => void setMode((event.currentTarget as HTMLSelectElement).value as MotionMode)}
      >
        {#each Object.entries(modeLabels) as [value, label]}
          {#if modeSupported(value as MotionMode)}
            <option value={value}>{label}</option>
          {/if}
        {/each}
      </select>
    </label>
  </div>

  <div class="motion-simple-body">
    <div class="motion-simple-left">
      <section class="motion-panel">
        <div class="motion-panel-title">Command</div>
        <div class="motion-panel-body motion-grid">
          {#if activeMode === "position"}
            <div class="motion-segment span-2">
              <button class:active={$motionState.positionCommand === "absolute"} onclick={() => void setPositionCommand("absolute")}>Absolute</button>
              <button class:active={$motionState.positionCommand === "incremental"} onclick={() => void setPositionCommand("incremental")}>Incremental</button>
            </div>
            <label class="motion-field">
              <span>{$motionState.positionCommand === "absolute" ? "Target position" : "Delta position"}</span>
              <div class="unit-field">
                {#if $motionState.positionCommand === "absolute"}
                  <input
                    class:ramModified={!!metadata[TARGET_POSITION] && $modifiedParameterIds.has(metadata[TARGET_POSITION].id)}
                    class:dirty={dirty(TARGET_POSITION)}
                    value={drafts[TARGET_POSITION] ?? ""}
                    disabled={!metadata[TARGET_POSITION] || writing.has(TARGET_POSITION)}
                    oninput={(event) => drafts = { ...drafts, [TARGET_POSITION]: event.currentTarget.value }}
                    onkeydown={(event) => parameterKeydown(event, TARGET_POSITION)}
                    onblur={() => resetDraft(TARGET_POSITION)}
                  />
                {:else}
                  <input
                    class:dirty={incrementalDraft !== formatHostNumber($motionState.incrementalDeltaTurn)}
                    value={incrementalDraft}
                    oninput={(event) => incrementalDraft = event.currentTarget.value}
                    onkeydown={incrementalKeydown}
                    onblur={() => incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn)}
                  />
                {/if}
                <em>turn</em>
              </div>
            </label>
            <label class="motion-field">
              <span>Max speed</span>
              <div class="unit-field">
                <input
                  class:ramModified={!!metadata[MOTION_MAX_SPEED] && $modifiedParameterIds.has(metadata[MOTION_MAX_SPEED].id)}
                  class:dirty={dirty(MOTION_MAX_SPEED)}
                  value={drafts[MOTION_MAX_SPEED] ?? ""}
                  disabled={!metadata[MOTION_MAX_SPEED] || writing.has(MOTION_MAX_SPEED)}
                  oninput={(event) => drafts = { ...drafts, [MOTION_MAX_SPEED]: event.currentTarget.value }}
                  onkeydown={(event) => parameterKeydown(event, MOTION_MAX_SPEED)}
                  onblur={() => resetDraft(MOTION_MAX_SPEED)}
                />
                <em>rad/s</em>
              </div>
            </label>
            <label class="motion-check span-2">
              <input type="checkbox" checked={$motionState.repeat} onchange={(event) => void setRepeat(event.currentTarget.checked)} />
              <span>Repeat</span>
            </label>
          {:else if activeMode === "speed" || activeMode === "sensorless-speed"}
            <label class="motion-field span-2">
              <span>Target speed</span>
              <div class="unit-field">
                <input
                  class:ramModified={!!metadata[TARGET_SPEED] && $modifiedParameterIds.has(metadata[TARGET_SPEED].id)}
                  class:dirty={dirty(TARGET_SPEED)}
                  value={drafts[TARGET_SPEED] ?? ""}
                  disabled={!metadata[TARGET_SPEED] || writing.has(TARGET_SPEED)}
                  oninput={(event) => drafts = { ...drafts, [TARGET_SPEED]: event.currentTarget.value }}
                  onkeydown={(event) => parameterKeydown(event, TARGET_SPEED)}
                  onblur={() => resetDraft(TARGET_SPEED)}
                />
                <em>rad/s</em>
              </div>
            </label>
            {#if activeMode === "sensorless-speed" && metadata[SENSORLESS_STARTUP_CURRENT]}
              <label class="motion-field">
                <span>Startup current</span>
                <div class="unit-field">
                  <input
                    class:ramModified={$modifiedParameterIds.has(metadata[SENSORLESS_STARTUP_CURRENT].id)}
                    class:dirty={dirty(SENSORLESS_STARTUP_CURRENT)}
                    value={drafts[SENSORLESS_STARTUP_CURRENT] ?? ""}
                    disabled={writing.has(SENSORLESS_STARTUP_CURRENT)}
                    oninput={(event) => drafts = { ...drafts, [SENSORLESS_STARTUP_CURRENT]: event.currentTarget.value }}
                    onkeydown={(event) => parameterKeydown(event, SENSORLESS_STARTUP_CURRENT)}
                    onblur={() => resetDraft(SENSORLESS_STARTUP_CURRENT)}
                  />
                  <em>A</em>
                </div>
              </label>
            {/if}
            {#if activeMode === "sensorless-speed" && metadata[SENSORLESS_ENTRY_SPEED]}
              <label class="motion-field">
                <span>Entry speed</span>
                <div class="unit-field">
                  <input
                    class:ramModified={$modifiedParameterIds.has(metadata[SENSORLESS_ENTRY_SPEED].id)}
                    class:dirty={dirty(SENSORLESS_ENTRY_SPEED)}
                    value={drafts[SENSORLESS_ENTRY_SPEED] ?? ""}
                    disabled={writing.has(SENSORLESS_ENTRY_SPEED)}
                    oninput={(event) => drafts = { ...drafts, [SENSORLESS_ENTRY_SPEED]: event.currentTarget.value }}
                    onkeydown={(event) => parameterKeydown(event, SENSORLESS_ENTRY_SPEED)}
                    onblur={() => resetDraft(SENSORLESS_ENTRY_SPEED)}
                  />
                  <em>rad/s</em>
                </div>
              </label>
            {/if}
          {:else if activeMode === "torque"}
            <label class="motion-field">
              <span>Target torque</span>
              <div class="unit-field">
                <input
                  class:ramModified={!!metadata[TARGET_TORQUE] && $modifiedParameterIds.has(metadata[TARGET_TORQUE].id)}
                  class:dirty={dirty(TARGET_TORQUE)}
                  value={drafts[TARGET_TORQUE] ?? ""}
                  disabled={!metadata[TARGET_TORQUE] || writing.has(TARGET_TORQUE)}
                  oninput={(event) => drafts = { ...drafts, [TARGET_TORQUE]: event.currentTarget.value }}
                  onkeydown={(event) => parameterKeydown(event, TARGET_TORQUE)}
                  onblur={() => resetDraft(TARGET_TORQUE)}
                />
                <em>N·m</em>
              </div>
            </label>
            {#if metadata[TORQUE_RAMP]}
              <label class="motion-field">
                <span>Torque ramp</span>
                <div class="unit-field">
                  <input
                    class:ramModified={$modifiedParameterIds.has(metadata[TORQUE_RAMP].id)}
                    class:dirty={dirty(TORQUE_RAMP)}
                    value={drafts[TORQUE_RAMP] ?? ""}
                    disabled={writing.has(TORQUE_RAMP)}
                    oninput={(event) => drafts = { ...drafts, [TORQUE_RAMP]: event.currentTarget.value }}
                    onkeydown={(event) => parameterKeydown(event, TORQUE_RAMP)}
                    onblur={() => resetDraft(TORQUE_RAMP)}
                  />
                  <em>N·m/s</em>
                </div>
              </label>
            {/if}
          {/if}
        </div>
      </section>

      {#if hasTrajectory}
        <section class="motion-panel">
          <div class="motion-panel-title">Trajectory · T / Trapezoidal</div>
          <div class="motion-panel-body motion-grid">
            <label class="motion-field">
              <span>Acceleration</span>
              <div class="unit-field">
                <input
                  class:ramModified={!!metadata[MOTION_ACCEL] && $modifiedParameterIds.has(metadata[MOTION_ACCEL].id)}
                  class:dirty={dirty(MOTION_ACCEL)}
                  value={drafts[MOTION_ACCEL] ?? ""}
                  disabled={!metadata[MOTION_ACCEL] || writing.has(MOTION_ACCEL)}
                  oninput={(event) => drafts = { ...drafts, [MOTION_ACCEL]: event.currentTarget.value }}
                  onkeydown={(event) => parameterKeydown(event, MOTION_ACCEL)}
                  onblur={() => resetDraft(MOTION_ACCEL)}
                />
                <em>rad/s²</em>
              </div>
            </label>
            <label class="motion-field">
              <span>Deceleration</span>
              <div class="unit-field">
                <input
                  class:ramModified={!!metadata[MOTION_DECEL] && $modifiedParameterIds.has(metadata[MOTION_DECEL].id)}
                  class:dirty={dirty(MOTION_DECEL)}
                  value={drafts[MOTION_DECEL] ?? ""}
                  disabled={!metadata[MOTION_DECEL] || writing.has(MOTION_DECEL)}
                  oninput={(event) => drafts = { ...drafts, [MOTION_DECEL]: event.currentTarget.value }}
                  onkeydown={(event) => parameterKeydown(event, MOTION_DECEL)}
                  onblur={() => resetDraft(MOTION_DECEL)}
                />
                <em>rad/s²</em>
              </div>
            </label>
          </div>
        </section>
      {/if}

      <div class="motion-runbar">
        <button class="motion-run" disabled={!canRun() || actionBusy} title="Run motion" onclick={run}>
          <i class="codicon codicon-debug-start"></i> Run
        </button>
        <button disabled={!canStop() || actionBusy} title="Stop motor motion" onclick={stop}>
          <i class="codicon codicon-debug-stop"></i> Stop
        </button>
      </div>
    </div>

    <div class="motion-simple-right">
      <div class="motion-plot-header">
        <div>
          <strong>{activeMode ? modeLabels[activeMode] : "—"}</strong>
          {#if hasTrajectory}<span>T / Trapezoidal</span>{/if}
        </div>
        <div class="preview-target">{previewLabel()}</div>
      </div>
      <div class="motion-plot-area">
        {#if $motionPreview && $motionPreview.times.length > 1}
          <MotionTrajectoryPlot preview={$motionPreview} />
        {:else}
          <div class="motion-plot-empty">No trajectory preview for this command mode.</div>
        {/if}
      </div>
    </div>
  </div>
</div>
