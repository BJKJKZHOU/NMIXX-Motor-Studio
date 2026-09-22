<script lang="ts">
  import { onMount } from "svelte";
  import type { MotionCapabilities } from "../connection/types";
  import { selectParameters } from "../parameters/state";
  import { createParameterEditor } from "../parameters/editor";
  import { modifiedParameterIds } from "../parameters/persistence";
  import {
    MOTION_ACCEL, MOTION_DECEL, MOTION_MAX_SPEED, MOTION_MODE, MOTION_PARAMETER_SYMBOLS,
    SENSORLESS_ENTRY_SPEED, SENSORLESS_STARTUP_CURRENT, TARGET_POSITION, TARGET_SPEED,
    TARGET_TORQUE, TORQUE_RAMP, modeFromParameter, modeParameterValue, motionParameterText, motionParameterCodec,
  } from "./parameters";
  import {
    executeMotion, initializeMotion, motionPreview, motionPreviewError, motionState,
    observeMotionPreview, stopMotionExecution, updateMotion,
  } from "./store";
  import MotionTrajectoryPlot from "./MotionTrajectoryPlot.svelte";
  import type { MotionMode } from "./types";

  export let capabilities: MotionCapabilities | undefined;
  export let motorState: number | null = null;
  export let onError: (error: unknown) => void = () => undefined;
  const MOTOR_DISABLED = 0;
  const MOTOR_ENABLED = 1;
  const modeLabels: Record<MotionMode, string> = {
    position: "Position", speed: "Speed", "sensorless-speed": "Sensorless Speed", torque: "Torque",
  };
  const parameters = selectParameters(MOTION_PARAMETER_SYMBOLS);
  const edits = createParameterEditor(parameters, motionParameterCodec);
  $: metadata = $parameters.metadata;
  $: values = $parameters.values;
  $: drafts = $edits.drafts;
  $: writing = $edits.writing;
  $: activeMode = modeFromParameter(metadata[MOTION_MODE], values[MOTION_MODE]);
  $: hasTrajectory = activeMode === "position" || activeMode === "speed" || activeMode === "sensorless-speed";
  let actionBusy = false;
  let stopBusy = false;
  let incrementalDraft = "1";
  onMount(() => {
    let disposed = false;
    void initializeMotion().then(() => {
      if (!disposed) incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn);
    }).catch(onError);
    const stopPreview = observeMotionPreview();
    return () => { disposed = true; stopPreview(); };
  });
  function formatHostNumber(value: number): string { return Number(value).toPrecision(9).replace(/(?:\.0+|(\.\d+?)0+)$/, "$1"); }
  function locked(symbol: string): boolean { return $parameters.loading || $parameters.saving || !metadata[symbol]?.access.includes("w") || writing.has(symbol); }
  function dirty(symbol: string): boolean { return $edits.dirty.has(symbol); }
  function resetDraft(symbol: string) { edits.discard(symbol); }
  function parameterKeydown(event: KeyboardEvent, symbol: string) { if (!locked(symbol)) edits.keydown(event, symbol, onError); }
  async function setMode(mode: MotionMode) {
    const value = modeParameterValue(metadata[MOTION_MODE], mode);
    if (value === undefined || motorState !== MOTOR_DISABLED || locked(MOTION_MODE)) return;
    try { await edits.select(MOTION_MODE, { type: "u8", value }); } catch (error) { onError(error); }
  }
  function modeSupported(mode: MotionMode): boolean {
    if (!capabilities) return false;
    if (mode === "position") return capabilities.position;
    if (mode === "speed") return capabilities.speed;
    if (mode === "sensorless-speed") return capabilities.sensorlessSpeed;
    return capabilities.torque;
  }
  async function setPositionCommand(command: "absolute" | "incremental") {
    try { await updateMotion("positionCommand", command); incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn); }
    catch (error) { onError(error); }
  }
  async function setRepeat(checked: boolean) {
    try { await updateMotion("repeat", checked); } catch (error) { onError(error); }
  }
  async function commitIncremental() {
    const text = incrementalDraft.trim();
    const parsed = Number(text);
    if (!text || !Number.isFinite(parsed)) {
      incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn);
      onError("Delta position must be finite."); return;
    }
    try { await updateMotion("incrementalDeltaTurn", parsed); }
    catch (error) { onError(error); }
    finally { incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn); }
  }
  function incrementalKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") { event.preventDefault(); void commitIncremental(); (event.currentTarget as HTMLInputElement).blur(); }
    else if (event.key === "Escape") { event.preventDefault(); incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn); (event.currentTarget as HTMLInputElement).blur(); }
  }
  function canRun(): boolean {
    return !!capabilities?.run && !$parameters.saving && writing.size === 0 && motorState === MOTOR_ENABLED && !!activeMode && modeSupported(activeMode);
  }
  function canStop(): boolean { return !!capabilities?.stop && !stopBusy; }
  async function run() {
    if (!canRun() || actionBusy) return;
    actionBusy = true;
    try { await executeMotion(); } catch (error) { onError(error); } finally { actionBusy = false; }
  }
  async function stop() {
    if (!canStop()) return;
    stopBusy = true;
    try { await stopMotionExecution(); } catch (error) { onError(error); } finally { stopBusy = false; }
  }
  function previewLabel(): string {
    if (activeMode === "position") return $motionState.positionCommand === "incremental"
      ? `${$motionState.incrementalDeltaTurn.toFixed(3)} turn delta`
      : `${motionParameterText(metadata[TARGET_POSITION], values[TARGET_POSITION]) || "—"} turn`;
    const symbol = activeMode === "torque" ? TARGET_TORQUE : TARGET_SPEED;
    const value = values[symbol];
    if (!value || value.type === "position" || !activeMode) return "—";
    return activeMode === "torque" ? `${Number(value.value).toFixed(3)} N·m` : `${Number(value.value).toFixed(2)} rad/s`;
  }
</script>

<div class="motion-simple-page">
  <div class="motion-mode-row"><label><span>Mode</span>
    <select value={activeMode ?? ""} disabled={motorState !== MOTOR_DISABLED || locked(MOTION_MODE)}
      class:ramModified={!!metadata[MOTION_MODE] && $modifiedParameterIds.has(metadata[MOTION_MODE].id)}
      title={motorState === MOTOR_DISABLED ? "Select motor mode" : "Disable the motor before changing mode"}
      onchange={(event) => void setMode(event.currentTarget.value as MotionMode)}>
      {#each Object.entries(modeLabels) as [value, label]}{#if modeSupported(value as MotionMode)}<option value={value}>{label}</option>{/if}{/each}
    </select>
  </label></div>
  <div class="motion-simple-body">
    <div class="motion-simple-left">
      <section class="motion-panel"><div class="motion-panel-title">Command</div><div class="motion-panel-body motion-grid">
        {#if activeMode === "position"}
          <div class="motion-segment span-2"><button class:active={$motionState.positionCommand === "absolute"} onclick={() => void setPositionCommand("absolute")}>Absolute</button><button class:active={$motionState.positionCommand === "incremental"} onclick={() => void setPositionCommand("incremental")}>Incremental</button></div>
          <label class="motion-field"><span>{$motionState.positionCommand === "absolute" ? "Target position" : "Delta position"}</span><div class="unit-field">
            {#if $motionState.positionCommand === "absolute"}
<input class:ramModified={!!metadata[TARGET_POSITION] && $modifiedParameterIds.has(metadata[TARGET_POSITION].id)} class:dirty={dirty(TARGET_POSITION)} value={drafts[TARGET_POSITION] ?? ""} disabled={locked(TARGET_POSITION)} oninput={(event) => edits.edit(TARGET_POSITION, event.currentTarget.value)} onkeydown={(event) => parameterKeydown(event, TARGET_POSITION)} onblur={() => resetDraft(TARGET_POSITION)} />
            {:else}
              <input class:dirty={incrementalDraft !== formatHostNumber($motionState.incrementalDeltaTurn)} value={incrementalDraft}
                oninput={(event) => incrementalDraft = event.currentTarget.value} onkeydown={incrementalKeydown}
                onblur={() => incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn)} />
            {/if}<em>turn</em>
          </div></label>
<label class="motion-field"><span>Max speed</span><div class="unit-field"><input class:ramModified={!!metadata[MOTION_MAX_SPEED] && $modifiedParameterIds.has(metadata[MOTION_MAX_SPEED].id)} class:dirty={dirty(MOTION_MAX_SPEED)} value={drafts[MOTION_MAX_SPEED] ?? ""} disabled={locked(MOTION_MAX_SPEED)} oninput={(event) => edits.edit(MOTION_MAX_SPEED, event.currentTarget.value)} onkeydown={(event) => parameterKeydown(event, MOTION_MAX_SPEED)} onblur={() => resetDraft(MOTION_MAX_SPEED)} /><em>rad/s</em></div></label>

          <label class="motion-check span-2"><input type="checkbox" checked={$motionState.repeat} onchange={(event) => void setRepeat(event.currentTarget.checked)} /><span>Repeat</span></label>
        {:else if activeMode === "speed" || activeMode === "sensorless-speed"}
<label class="motion-field span-2"><span>Target speed</span><div class="unit-field"><input class:ramModified={!!metadata[TARGET_SPEED] && $modifiedParameterIds.has(metadata[TARGET_SPEED].id)} class:dirty={dirty(TARGET_SPEED)} value={drafts[TARGET_SPEED] ?? ""} disabled={locked(TARGET_SPEED)} oninput={(event) => edits.edit(TARGET_SPEED, event.currentTarget.value)} onkeydown={(event) => parameterKeydown(event, TARGET_SPEED)} onblur={() => resetDraft(TARGET_SPEED)} /><em>rad/s</em></div></label>
          {#if activeMode === "sensorless-speed" && metadata[SENSORLESS_STARTUP_CURRENT]}
<label class="motion-field"><span>Startup current</span><div class="unit-field"><input class:ramModified={!!metadata[SENSORLESS_STARTUP_CURRENT] && $modifiedParameterIds.has(metadata[SENSORLESS_STARTUP_CURRENT].id)} class:dirty={dirty(SENSORLESS_STARTUP_CURRENT)} value={drafts[SENSORLESS_STARTUP_CURRENT] ?? ""} disabled={locked(SENSORLESS_STARTUP_CURRENT)} oninput={(event) => edits.edit(SENSORLESS_STARTUP_CURRENT, event.currentTarget.value)} onkeydown={(event) => parameterKeydown(event, SENSORLESS_STARTUP_CURRENT)} onblur={() => resetDraft(SENSORLESS_STARTUP_CURRENT)} /><em>A</em></div></label>
          {/if}
          {#if activeMode === "sensorless-speed" && metadata[SENSORLESS_ENTRY_SPEED]}
<label class="motion-field"><span>Entry speed</span><div class="unit-field"><input class:ramModified={!!metadata[SENSORLESS_ENTRY_SPEED] && $modifiedParameterIds.has(metadata[SENSORLESS_ENTRY_SPEED].id)} class:dirty={dirty(SENSORLESS_ENTRY_SPEED)} value={drafts[SENSORLESS_ENTRY_SPEED] ?? ""} disabled={locked(SENSORLESS_ENTRY_SPEED)} oninput={(event) => edits.edit(SENSORLESS_ENTRY_SPEED, event.currentTarget.value)} onkeydown={(event) => parameterKeydown(event, SENSORLESS_ENTRY_SPEED)} onblur={() => resetDraft(SENSORLESS_ENTRY_SPEED)} /><em>rad/s</em></div></label>
          {/if}
        {:else if activeMode === "torque"}
<label class="motion-field"><span>Target torque</span><div class="unit-field"><input class:ramModified={!!metadata[TARGET_TORQUE] && $modifiedParameterIds.has(metadata[TARGET_TORQUE].id)} class:dirty={dirty(TARGET_TORQUE)} value={drafts[TARGET_TORQUE] ?? ""} disabled={locked(TARGET_TORQUE)} oninput={(event) => edits.edit(TARGET_TORQUE, event.currentTarget.value)} onkeydown={(event) => parameterKeydown(event, TARGET_TORQUE)} onblur={() => resetDraft(TARGET_TORQUE)} /><em>N·m</em></div></label>
          {#if metadata[TORQUE_RAMP]}
<label class="motion-field"><span>Torque ramp</span><div class="unit-field"><input class:ramModified={!!metadata[TORQUE_RAMP] && $modifiedParameterIds.has(metadata[TORQUE_RAMP].id)} class:dirty={dirty(TORQUE_RAMP)} value={drafts[TORQUE_RAMP] ?? ""} disabled={locked(TORQUE_RAMP)} oninput={(event) => edits.edit(TORQUE_RAMP, event.currentTarget.value)} onkeydown={(event) => parameterKeydown(event, TORQUE_RAMP)} onblur={() => resetDraft(TORQUE_RAMP)} /><em>N·m/s</em></div></label>
          {/if}
        {/if}
      </div></section>
      {#if hasTrajectory}
        <section class="motion-panel"><div class="motion-panel-title">Trajectory · T / Trapezoidal</div><div class="motion-panel-body motion-grid">
<label class="motion-field"><span>Acceleration</span><div class="unit-field"><input class:ramModified={!!metadata[MOTION_ACCEL] && $modifiedParameterIds.has(metadata[MOTION_ACCEL].id)} class:dirty={dirty(MOTION_ACCEL)} value={drafts[MOTION_ACCEL] ?? ""} disabled={locked(MOTION_ACCEL)} oninput={(event) => edits.edit(MOTION_ACCEL, event.currentTarget.value)} onkeydown={(event) => parameterKeydown(event, MOTION_ACCEL)} onblur={() => resetDraft(MOTION_ACCEL)} /><em>rad/s²</em></div></label>
<label class="motion-field"><span>Deceleration</span><div class="unit-field"><input class:ramModified={!!metadata[MOTION_DECEL] && $modifiedParameterIds.has(metadata[MOTION_DECEL].id)} class:dirty={dirty(MOTION_DECEL)} value={drafts[MOTION_DECEL] ?? ""} disabled={locked(MOTION_DECEL)} oninput={(event) => edits.edit(MOTION_DECEL, event.currentTarget.value)} onkeydown={(event) => parameterKeydown(event, MOTION_DECEL)} onblur={() => resetDraft(MOTION_DECEL)} /><em>rad/s²</em></div></label>
        </div></section>
      {/if}
      <div class="motion-runbar">
        <button class="motion-run" disabled={!canRun() || actionBusy} title="Run motion" onclick={run}><i class="codicon codicon-debug-start"></i> Run</button>
        <button disabled={!canStop()} title="Stop motor motion" onclick={stop}><i class={`codicon ${stopBusy ? "codicon-loading codicon-modifier-spin" : "codicon-debug-stop"}`}></i> Stop</button>
      </div>
    </div>
    <div class="motion-simple-right">
      <div class="motion-plot-header"><div><strong>{activeMode ? modeLabels[activeMode] : "—"}</strong>{#if hasTrajectory}<span>T / Trapezoidal</span>{/if}</div><div class="preview-target">{previewLabel()}</div></div>
      <div class="motion-plot-area">
        {#if $motionPreview && $motionPreview.times.length > 1}<MotionTrajectoryPlot preview={$motionPreview} />
        {:else}<div class="motion-plot-empty">{$motionPreviewError ?? "No trajectory preview for this command mode."}</div>{/if}
      </div>
    </div>
  </div>
</div>
