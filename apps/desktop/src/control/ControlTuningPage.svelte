<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { ConnectionInfo } from "../connection/types";
  import { subscribeRefresh } from "../refreshScheduler";
  import { selectParameters } from "../parameters/state";
  import { createParameterEditor } from "../parameters/editor";
  import { modifiedParameterIds } from "../parameters/persistence";
  import { initializeMotion, motionState, updateMotion, waitMotionUpdates } from "../motion/store";
  import {
    MOTION_ACCEL, MOTION_DECEL, MOTION_MAX_SPEED, MOTION_MODE, TARGET_POSITION, TARGET_SPEED,
    TARGET_TORQUE, TORQUE_RAMP, modeFromParameter, modeParameterValue, motionParameterCodec,
  } from "../motion/parameters";
  import type { MotionMode, MotionState } from "../motion/types";
  import ExperimentWaveform from "./ExperimentWaveform.svelte";
  import { readTuningExperimentSnapshot, startTuningExperiment, stopTuningExperiment,
    tuningExperimentDefaults, tuningExperimentStatus } from "./tuningExperiment";
  import type { TuningExperimentSnapshot, TuningExperimentState, TuningExperimentSelection } from "./tuningExperiment";

  type Props = { connection: ConnectionInfo | undefined; motorState: number | null; onError?: (error: unknown) => void };
  type LoopSpec = { title: string; bandwidth: string; source: string; gains: string[] };
  const MOTOR_DISABLED = 0;
  const MOTOR_ENABLED = 1;
  const MOTOR_RUN = 2;
  const CURRENT_BW = "PARAM_CTRL_CURRENT_BW_HZ";
  const CURRENT_SOURCE = "PARAM_CTRL_CURRENT_SOURCE";
  const ID_KP = "PARAM_CTRL_ID_KP";
  const ID_KI = "PARAM_CTRL_ID_KI";
  const IQ_KP = "PARAM_CTRL_IQ_KP";
  const IQ_KI = "PARAM_CTRL_IQ_KI";
  const SPEED_BW = "PARAM_CTRL_SPEED_BW_HZ";
  const SPEED_SOURCE = "PARAM_CTRL_SPEED_SOURCE";
  const SPEED_KP = "PARAM_CTRL_SPEED_KP";
  const SPEED_KI = "PARAM_CTRL_SPEED_KI";
  const POSITION_KP = "PARAM_CTRL_POSITION_KP";
  const ESO_BW = "PARAM_CTRL_MECH_ESO_BW_HZ";
  const LOOP_SPECS: LoopSpec[] = [
    { title: "Current Loop", bandwidth: CURRENT_BW, source: CURRENT_SOURCE, gains: [ID_KP, ID_KI, IQ_KP, IQ_KI] },
    { title: "Speed Loop", bandwidth: SPEED_BW, source: SPEED_SOURCE, gains: [SPEED_KP, SPEED_KI] },
  ];
  const SYMBOLS = [CURRENT_BW, CURRENT_SOURCE, ID_KP, ID_KI, IQ_KP, IQ_KI, SPEED_BW, SPEED_SOURCE,
    SPEED_KP, SPEED_KI, POSITION_KP, ESO_BW, MOTION_MODE, MOTION_MAX_SPEED, MOTION_ACCEL, MOTION_DECEL,
    TARGET_POSITION, TARGET_SPEED, TARGET_TORQUE, TORQUE_RAMP];
  const motionModeLabels: Record<MotionMode, string> = {
    position: "Position", speed: "Speed", "sensorless-speed": "Sensorless Speed", torque: "Torque",
  };
  let { connection, motorState, onError = () => undefined }: Props = $props();
  const parameters = selectParameters(SYMBOLS);
  const edits = createParameterEditor(parameters, motionParameterCodec);
  let metadata = $derived($parameters.metadata);
  let values = $derived($parameters.values);
  let drafts = $derived($edits.drafts);
  let writing = $derived($edits.writing);
  let loading = $derived($parameters.loading);
  let motionActionBusy = $state(false);
  let stopActionBusy = $state(false);
  let copyAccelToDecel = $state(false);
  let experimentState = $state<TuningExperimentState>("IDLE");
  let experimentMessage = $state("");
  let experimentSnapshot = $state<TuningExperimentSnapshot | undefined>(undefined);
  let experimentRefreshBusy = false;
  let generation = 0;
  let waveformTab = $state<"waveform" | "channels" | "scale">("waveform");
  let tuningSelections = $state<Record<number, "fast" | "normal">>({});
  let displayMultipliers = $state<Record<number, number>>({});
  let tuningSelectionMode = $state<MotionMode | undefined>(undefined);
  let tuningTimePerDiv = $state(20e-3);
  let incrementalDraft = $state("1");

  onMount(() => {
    let disposed = false;
    void initializeMotion().then(() => {
      if (!disposed) incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn);
    }).catch(onError);
    void refreshExperiment();
    const stop = subscribeRefresh(50, () => void refreshExperiment());
    return () => { disposed = true; ++generation; stop(); };
  });
  $effect(() => {
    const activeConnection = connection;
    ++generation;
    tuningSelectionMode = undefined;
    if (!activeConnection) {
      tuningSelections = {}; displayMultipliers = {}; experimentState = "IDLE";
      experimentMessage = ""; experimentSnapshot = undefined;
    }
  });
  $effect(() => {
    if (!connection) return;
    const mode = modeFromParameter(metadata[MOTION_MODE], values[MOTION_MODE]);
    if (!mode || tuningSelectionMode === mode) return;
    tuningSelectionMode = mode;
    untrack(() => void loadTuningDefaults());
  });

  function formatHostNumber(value: number): string { return Number(value).toPrecision(9).replace(/(?:\.0+|(\.\d+?)0+)$/, "$1"); }
  function label(symbol: string): string { return metadata[symbol]?.label ?? "Unavailable"; }
  function unit(symbol: string): string { return metadata[symbol]?.unit ?? ""; }
  function locked(symbol: string): boolean {
    return $parameters.loading || $parameters.saving || experimentActive() || motorState === MOTOR_RUN || writing.has(symbol) || !metadata[symbol]?.access.includes("w");
  }
  function dirty(symbol: string): boolean { return $edits.dirty.has(symbol); }
  function hasDirtyDraft(): boolean { return $edits.dirty.size > 0 || incrementalDraft !== formatHostNumber($motionState.incrementalDeltaTurn); }
  function enumValue(symbol: string, enumSymbol: string): number | null {
    const meta = metadata[symbol];
    const index = meta?.allowedSymbols.indexOf(enumSymbol) ?? -1;
    return index < 0 ? null : meta.allowed[index] ?? index;
  }
  function sourceText(symbol: string): string {
    const parameter = values[symbol];
    if (!parameter || parameter.type === "position") return "—";
    const value = Number(parameter.value);
    if (value === enumValue(symbol, "CTRL_TUNE_BANDWIDTH")) return "Bandwidth";
    if (value === enumValue(symbol, "CTRL_TUNE_MANUAL")) return "Manual";
    return String(value);
  }
  async function setSource(symbol: string, mode: string) {
    const value = enumValue(symbol, mode === "Manual" ? "CTRL_TUNE_MANUAL" : "CTRL_TUNE_BANDWIDTH");
    if (value === null || locked(symbol)) return;
    try { await edits.select(symbol, { type: "u8", value }); } catch (error) { onError(error); }
  }
  function keydown(event: KeyboardEvent, symbol: string) { if (!locked(symbol)) edits.keydown(event, symbol, onError); }
  function activeMotionMode(): MotionMode | undefined { return modeFromParameter(metadata[MOTION_MODE], values[MOTION_MODE]); }
  function motionModeSupported(mode: MotionMode): boolean {
    const caps = connection?.motion;
    if (!caps) return false;
    if (mode === "position") return caps.position;
    if (mode === "speed") return caps.speed;
    if (mode === "sensorless-speed") return caps.sensorlessSpeed;
    return caps.torque;
  }
  async function setMotionMode(mode: MotionMode) {
    const value = modeParameterValue(metadata[MOTION_MODE], mode);
    if (value === undefined || motorState !== MOTOR_DISABLED || locked(MOTION_MODE)) return;
    try { await edits.select(MOTION_MODE, { type: "u8", value }); } catch (error) { onError(error); }
  }
  function updateMotionField<K extends keyof MotionState>(key: K, value: MotionState[K]) {
    void updateMotion(key, value).catch(onError);
  }
  async function commitIncremental() {
    const text = incrementalDraft.trim();
    const value = Number(text);
    if (!text || !Number.isFinite(value)) { incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn); onError("Delta position must be finite."); return; }
    try { await updateMotion("incrementalDeltaTurn", value); }
    catch (error) { onError(error); }
    finally { incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn); }
  }
  function incrementalKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") { event.preventDefault(); void commitIncremental(); (event.currentTarget as HTMLInputElement).blur(); }
    else if (event.key === "Escape") { incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn); (event.currentTarget as HTMLInputElement).blur(); }
  }
  async function motionParameterKeydown(event: KeyboardEvent, symbol: string) {
    if (event.key !== "Enter" || symbol !== MOTION_ACCEL || !copyAccelToDecel || !metadata[MOTION_DECEL]) { keydown(event, symbol); return; }
    if (locked(MOTION_ACCEL) || locked(MOTION_DECEL)) return;
    event.preventDefault();
    const input = event.currentTarget as HTMLInputElement;
    // Capture both commands before blur/readback; the first write cannot erase the second.
    const text = drafts[MOTION_ACCEL] ?? "";
    input.blur();
    try { await edits.commitText(MOTION_ACCEL, text); await edits.commitText(MOTION_DECEL, text); }
    catch (error) { onError(error); }
  }
  function editAcceleration(text: string) {
    edits.edit(MOTION_ACCEL, text);
    if (copyAccelToDecel) edits.edit(MOTION_DECEL, text);
  }
  function discardAcceleration() { edits.discard(MOTION_ACCEL); if (copyAccelToDecel) edits.discard(MOTION_DECEL); }
  function toggleCopyAccelToDecel(checked: boolean) {
    copyAccelToDecel = checked;
    if (checked) edits.edit(MOTION_DECEL, drafts[MOTION_ACCEL] ?? "");
    else edits.discard(MOTION_DECEL);
  }

  function selectedTuningEntries(): TuningExperimentSelection[] {
    return Object.entries(tuningSelections).map(([id, rate]) => ({ id: Number(id), rate })).filter((entry) => Number.isFinite(entry.id));
  }
  function selectedFastCount(): number { return selectedTuningEntries().filter((entry) => entry.rate === "fast").length; }
  function selectedNormalCount(): number { return selectedTuningEntries().filter((entry) => entry.rate === "normal").length; }
  function selectionWithinLimits(): boolean { return !!connection && selectedFastCount() <= connection.fastMaxChannels && selectedNormalCount() <= connection.normalMaxChannels; }
  async function loadTuningDefaults() {
    const token = generation;
    const mode = tuningSelectionMode;
    try {
      const defaults = await tuningExperimentDefaults();
      if (token !== generation || mode !== tuningSelectionMode) return;
      const next: Record<number, "fast" | "normal"> = {};
      const nextMultipliers = { ...displayMultipliers };
      for (const selection of defaults) { next[selection.id] = selection.rate; if (!(selection.id in nextMultipliers)) nextMultipliers[selection.id] = 1; }
      tuningSelections = next; displayMultipliers = nextMultipliers;
    } catch (error) { if (token === generation) onError(error); }
  }
  function toggleTuningChannel(id: number, checked: boolean) {
    if (experimentActive()) return;
    const next = { ...tuningSelections };
    if (checked) {
      const channel = connection?.channels.find((item) => item.id === id);
      if (!channel) return;
      next[id] = channel.supportsFast ? "fast" : "normal";
      if (!(id in displayMultipliers)) displayMultipliers = { ...displayMultipliers, [id]: 1 };
    } else delete next[id];
    tuningSelections = next;
  }
  function setTuningRate(id: number, rate: "fast" | "normal") {
    const channel = connection?.channels.find((item) => item.id === id);
    if (!channel || experimentActive() || (rate === "fast" && !channel.supportsFast) || (rate === "normal" && !channel.supportsNormal)) return;
    tuningSelections = { ...tuningSelections, [id]: rate };
  }
  function setDisplayMultiplier(id: number, value: number) {
    if (Number.isFinite(value) && value > 0) displayMultipliers = { ...displayMultipliers, [id]: value };
  }
  function resetDisplayMultipliers() {
    const next = { ...displayMultipliers };
    for (const id of Object.keys(tuningSelections)) next[Number(id)] = 1;
    displayMultipliers = next;
  }
  function experimentActive(): boolean { return experimentState === "PREPARING" || experimentState === "RUNNING" || experimentState === "STOPPING"; }
  function motionLocked(): boolean { return $parameters.loading || $parameters.saving || motorState === MOTOR_RUN || motionActionBusy || stopActionBusy || experimentActive(); }
  async function refreshExperiment() {
    if (!connection || experimentRefreshBusy) return;
    const token = generation;
    experimentRefreshBusy = true;
    try {
      const previousState = experimentState;
      const status = await tuningExperimentStatus();
      if (token !== generation) return;
      experimentState = status.state; experimentMessage = status.message ?? "";
      const active = status.state === "PREPARING" || status.state === "RUNNING" || status.state === "STOPPING";
      const finalTransition = (status.state === "COMPLETED" || status.state === "FAILED") && (previousState !== status.state || !experimentSnapshot);
      if (active || finalTransition) {
        const snapshot = await readTuningExperimentSnapshot(tuningTimePerDiv * 10, 0);
        if (token === generation) experimentSnapshot = snapshot;
      }
    } catch (error) { if (token === generation && experimentState !== "IDLE") onError(error); }
    finally { experimentRefreshBusy = false; }
  }
  function canRunMotion(): boolean {
    return !!connection && motorState === MOTOR_ENABLED && !motionActionBusy && !experimentActive()
      && !$parameters.saving && writing.size === 0 && !hasDirtyDraft() && connection.motion.run
      && selectedTuningEntries().length > 0 && selectionWithinLimits() && !!activeMotionMode() && motionModeSupported(activeMotionMode()!);
  }
  function canStopMotion(): boolean { return !!connection && !stopActionBusy && connection.motion.stop; }
  async function runTuningMotion() {
    if (!canRunMotion()) return;
    motionActionBusy = true;
    try {
      await waitMotionUpdates();
      const status = await startTuningExperiment(selectedTuningEntries());
      experimentState = status.state; experimentMessage = status.message ?? ""; experimentSnapshot = undefined; waveformTab = "waveform";
      await refreshExperiment();
    } catch (error) { onError(error); } finally { motionActionBusy = false; }
  }
  async function requestExperimentWindow(windowSeconds: number, endOffsetSeconds: number, maxPoints = 3000) {
    if (!connection || experimentRefreshBusy) return;
    const token = generation;
    experimentRefreshBusy = true;
    try {
      const snapshot = await readTuningExperimentSnapshot(windowSeconds, endOffsetSeconds, maxPoints);
      if (token === generation) experimentSnapshot = snapshot;
    } catch (error) { if (token === generation) onError(error); } finally { experimentRefreshBusy = false; }
  }
  async function stopTuningMotion() {
    if (!canStopMotion()) return;
    stopActionBusy = true;
    try {
      const status = await stopTuningExperiment();
      experimentState = status.state; experimentMessage = status.message ?? "";
      await refreshExperiment();
    } catch (error) { onError(error); } finally { stopActionBusy = false; }
  }
</script>

<div class="tuning-root">
  <section class="page-toolbar"><div class="page-title">CONTROL TUNING</div>{#if hasDirtyDraft()}<div class="dirty-note">Uncommitted edits</div>{/if}</section>
  <section class="tuning-content">
    {#if !connection}<div class="empty-state"><i class="codicon codicon-plug"></i><div>Connect a device to tune control loops.</div></div>
    {:else}
      <div class="tuning-layout"><div class="experiment-column">
        <section class="waveform-panel">
          <div class="waveform-heading"><div class="section-title">Experiment Waveform</div><div class="experiment-status" class:failed={experimentState === "FAILED"}>{experimentState}{#if experimentSnapshot}<span>· loss {experimentSnapshot.snapshot.lostFrames}</span>{/if}</div></div>
          {#if experimentMessage}<div class="experiment-message">{experimentMessage}</div>{/if}
          <div class="waveform-tabs" role="tablist" aria-label="Experiment waveform views">
            <button class:active={waveformTab === "waveform"} onclick={() => waveformTab = "waveform"}>Waveform</button>
            <button class:active={waveformTab === "channels"} onclick={() => waveformTab = "channels"}>Channels {selectedTuningEntries().length}</button>
            <button class:active={waveformTab === "scale"} onclick={() => waveformTab = "scale"}>Scale</button>
          </div>
          <div class="waveform-tab-content">
            {#if waveformTab === "waveform"}
              <div class="waveform-tools"><span>Time/div</span><strong>{tuningTimePerDiv >= 1 ? `${tuningTimePerDiv.toFixed(2)} s` : tuningTimePerDiv >= 1e-3 ? `${(tuningTimePerDiv * 1e3).toFixed(tuningTimePerDiv < 10e-3 ? 2 : 1)} ms` : `${Math.round(tuningTimePerDiv * 1e6)} µs`}</strong></div>
              <ExperimentWaveform result={experimentSnapshot} multipliers={displayMultipliers} bind:timePerDiv={tuningTimePerDiv}
                onViewRequest={(windowSeconds, endOffsetSeconds, maxPoints) => void requestExperimentWindow(windowSeconds, endOffsetSeconds, maxPoints)} />
            {:else if waveformTab === "channels"}
              <div class="tuning-channel-list">
                {#each connection.channels as channel (channel.id)}
                  <div class="tuning-channel-row"><label>
                    <input type="checkbox" checked={tuningSelections[channel.id] !== undefined} disabled={experimentActive()} onchange={(event) => toggleTuningChannel(channel.id, event.currentTarget.checked)} />
                    <span>{channel.label}</span><small>{channel.unit ?? ""}</small>
                  </label>
                  {#if tuningSelections[channel.id] !== undefined}
                    <select class="compact-select" value={tuningSelections[channel.id]} disabled={experimentActive()} onchange={(event) => setTuningRate(channel.id, event.currentTarget.value as "fast" | "normal")}>
                      {#if channel.supportsFast}<option value="fast">FAST</option>{/if}{#if channel.supportsNormal}<option value="normal">NORMAL</option>{/if}
                    </select>
                  {/if}</div>
                {/each}
                <div class="tuning-channel-limits" class:invalid={!selectionWithinLimits()}><span>FAST {selectedFastCount()} / {connection.fastMaxChannels}</span><span>NORMAL {selectedNormalCount()} / {connection.normalMaxChannels}</span></div>
              </div>
            {:else}
              <div class="tuning-scale-list">
                {#each connection.channels.filter((channel) => tuningSelections[channel.id] !== undefined) as channel (channel.id)}
                  <label class="tuning-scale-row"><span>{channel.label}</span><span class="scale-editor"><span>×</span>
                    <input class="compact-input mono" type="number" min="0.000001" step="any" value={displayMultipliers[channel.id] ?? 1} onchange={(event) => setDisplayMultiplier(channel.id, Number(event.currentTarget.value))} />
                  </span></label>
                {/each}
                <div class="tuning-scale-actions"><button onclick={resetDisplayMultipliers}>Reset ×1</button></div>
              </div>
            {/if}
          </div>
        </section>
        <section class="motion-panel"><div class="section-title motion-title">Motion Command</div>
          <div class="motion-toolbar"><label class="motion-mode"><span>Mode</span>
            <select class="compact-select motion-mode-select" value={activeMotionMode() ?? ""}
              class:ramModified={!!metadata[MOTION_MODE] && $modifiedParameterIds.has(metadata[MOTION_MODE].id)}
              disabled={motionLocked() || motorState !== MOTOR_DISABLED || locked(MOTION_MODE)}
              title={motorState === MOTOR_DISABLED ? "Select motor mode" : "Disable the motor before changing mode"}
              onchange={(event) => void setMotionMode(event.currentTarget.value as MotionMode)}>
              {#each Object.entries(motionModeLabels) as [value, labelText]}{#if motionModeSupported(value as MotionMode)}<option value={value}>{labelText}</option>{/if}{/each}
            </select>
          </label>
          {#if activeMotionMode() === "position"}
            <div class="position-mode-options" aria-label="Position command mode">
              <label><input type="radio" name="tuning-position-command" checked={$motionState.positionCommand === "incremental"} disabled={motionLocked()} onchange={() => updateMotionField("positionCommand", "incremental")} />Incremental</label>
              <label><input type="radio" name="tuning-position-command" checked={$motionState.positionCommand === "absolute"} disabled={motionLocked()} onchange={() => updateMotionField("positionCommand", "absolute")} />Absolute</label>
              <label><input type="checkbox" checked={$motionState.repeat} disabled={motionLocked()} onchange={(event) => updateMotionField("repeat", event.currentTarget.checked)} />Repeat</label>
            </div>
          {/if}</div>
          {#if activeMotionMode() === "position"}
            <div class="motion-grid">
              <label><span>{$motionState.positionCommand === "absolute" ? "Position" : "Delta"}</span><span class="motion-editor">
                {#if $motionState.positionCommand === "absolute"}
<input class:ramModified={!!metadata[TARGET_POSITION] && $modifiedParameterIds.has(metadata[TARGET_POSITION].id)} class:dirty={dirty(TARGET_POSITION)} class="compact-input mono" value={drafts[TARGET_POSITION] ?? ""} disabled={motionLocked() || locked(TARGET_POSITION)} oninput={(event) => edits.edit(TARGET_POSITION, event.currentTarget.value)} onkeydown={(event) => motionParameterKeydown(event, TARGET_POSITION)} onblur={() => edits.discard(TARGET_POSITION)} />
                {:else}
                  <input class:dirty={incrementalDraft !== formatHostNumber($motionState.incrementalDeltaTurn)} class="compact-input mono" value={incrementalDraft} disabled={motionLocked()}
                    oninput={(event) => incrementalDraft = event.currentTarget.value} onkeydown={incrementalKeydown} onblur={() => incrementalDraft = formatHostNumber($motionState.incrementalDeltaTurn)} />
                {/if}<span class="unit">turn</span>
              </span></label>
<label><span>Max Speed</span><span class="motion-editor"><input class:ramModified={!!metadata[MOTION_MAX_SPEED] && $modifiedParameterIds.has(metadata[MOTION_MAX_SPEED].id)} class:dirty={dirty(MOTION_MAX_SPEED)} class="compact-input mono" value={drafts[MOTION_MAX_SPEED] ?? ""} disabled={motionLocked() || locked(MOTION_MAX_SPEED)} oninput={(event) => edits.edit(MOTION_MAX_SPEED, event.currentTarget.value)} onkeydown={(event) => motionParameterKeydown(event, MOTION_MAX_SPEED)} onblur={() => edits.discard(MOTION_MAX_SPEED)} /><span class="unit">rad/s</span></span></label>
<label><span>Accel</span><span class="motion-editor"><input class:ramModified={!!metadata[MOTION_ACCEL] && $modifiedParameterIds.has(metadata[MOTION_ACCEL].id)} class:dirty={dirty(MOTION_ACCEL)} class="compact-input mono" value={drafts[MOTION_ACCEL] ?? ""} disabled={motionLocked() || locked(MOTION_ACCEL)} oninput={(event) => editAcceleration(event.currentTarget.value)} onkeydown={(event) => motionParameterKeydown(event, MOTION_ACCEL)} onblur={() => discardAcceleration()} /><span class="unit">rad/s²</span></span></label>
<label><span>Decel</span><span class="motion-editor"><input class:ramModified={!!metadata[MOTION_DECEL] && $modifiedParameterIds.has(metadata[MOTION_DECEL].id)} class:dirty={dirty(MOTION_DECEL)} class="compact-input mono" value={drafts[MOTION_DECEL] ?? ""} disabled={motionLocked() || locked(MOTION_DECEL) || copyAccelToDecel} oninput={(event) => edits.edit(MOTION_DECEL, event.currentTarget.value)} onkeydown={(event) => motionParameterKeydown(event, MOTION_DECEL)} onblur={() => edits.discard(MOTION_DECEL)} /><span class="unit">rad/s²</span></span></label>

            </div>
          {:else if activeMotionMode() === "speed" || activeMotionMode() === "sensorless-speed"}
            <div class="motion-grid">
<label><span>Target Speed</span><span class="motion-editor"><input class:ramModified={!!metadata[TARGET_SPEED] && $modifiedParameterIds.has(metadata[TARGET_SPEED].id)} class:dirty={dirty(TARGET_SPEED)} class="compact-input mono" value={drafts[TARGET_SPEED] ?? ""} disabled={motionLocked() || locked(TARGET_SPEED)} oninput={(event) => edits.edit(TARGET_SPEED, event.currentTarget.value)} onkeydown={(event) => motionParameterKeydown(event, TARGET_SPEED)} onblur={() => edits.discard(TARGET_SPEED)} /><span class="unit">rad/s</span></span></label>
<div></div>
<label><span>Accel</span><span class="motion-editor"><input class:ramModified={!!metadata[MOTION_ACCEL] && $modifiedParameterIds.has(metadata[MOTION_ACCEL].id)} class:dirty={dirty(MOTION_ACCEL)} class="compact-input mono" value={drafts[MOTION_ACCEL] ?? ""} disabled={motionLocked() || locked(MOTION_ACCEL)} oninput={(event) => editAcceleration(event.currentTarget.value)} onkeydown={(event) => motionParameterKeydown(event, MOTION_ACCEL)} onblur={() => discardAcceleration()} /><span class="unit">rad/s²</span></span></label>
<label><span>Decel</span><span class="motion-editor"><input class:ramModified={!!metadata[MOTION_DECEL] && $modifiedParameterIds.has(metadata[MOTION_DECEL].id)} class:dirty={dirty(MOTION_DECEL)} class="compact-input mono" value={drafts[MOTION_DECEL] ?? ""} disabled={motionLocked() || locked(MOTION_DECEL) || copyAccelToDecel} oninput={(event) => edits.edit(MOTION_DECEL, event.currentTarget.value)} onkeydown={(event) => motionParameterKeydown(event, MOTION_DECEL)} onblur={() => edits.discard(MOTION_DECEL)} /><span class="unit">rad/s²</span></span></label>

            </div>
          {:else if activeMotionMode() === "torque"}
            <div class="motion-grid">
<label><span>Torque</span><span class="motion-editor"><input class:ramModified={!!metadata[TARGET_TORQUE] && $modifiedParameterIds.has(metadata[TARGET_TORQUE].id)} class:dirty={dirty(TARGET_TORQUE)} class="compact-input mono" value={drafts[TARGET_TORQUE] ?? ""} disabled={motionLocked() || locked(TARGET_TORQUE)} oninput={(event) => edits.edit(TARGET_TORQUE, event.currentTarget.value)} onkeydown={(event) => motionParameterKeydown(event, TARGET_TORQUE)} onblur={() => edits.discard(TARGET_TORQUE)} /><span class="unit">N·m</span></span></label>
{#if metadata[TORQUE_RAMP]}
<label><span>Ramp</span><span class="motion-editor"><input class:ramModified={!!metadata[TORQUE_RAMP] && $modifiedParameterIds.has(metadata[TORQUE_RAMP].id)} class:dirty={dirty(TORQUE_RAMP)} class="compact-input mono" value={drafts[TORQUE_RAMP] ?? ""} disabled={motionLocked() || locked(TORQUE_RAMP)} oninput={(event) => edits.edit(TORQUE_RAMP, event.currentTarget.value)} onkeydown={(event) => motionParameterKeydown(event, TORQUE_RAMP)} onblur={() => edits.discard(TORQUE_RAMP)} /><span class="unit">N·m/s</span></span></label>
{/if}
            </div>
          {/if}
          <div class="motion-footer">
            {#if activeMotionMode() === "position" || activeMotionMode() === "speed" || activeMotionMode() === "sensorless-speed"}
              <label class="copy-decel"><input type="checkbox" checked={copyAccelToDecel} disabled={motionLocked()} onchange={(event) => toggleCopyAccelToDecel(event.currentTarget.checked)} />Copy Accel to Decel</label>
            {:else}<span></span>{/if}
            <div class="motion-actions"><vscode-button disabled={!canRunMotion()}
              title={hasDirtyDraft() ? "Commit or discard tuning edits first" : experimentActive() ? "Tuning experiment is already running" : motorState !== MOTOR_ENABLED ? "Enable motor first" : "Run tuning experiment"}
              onclick={() => void runTuningMotion()}>Run</vscode-button>
              <vscode-button secondary disabled={!canStopMotion()} title="Stop the active tuning experiment and retain its waveform" onclick={() => void stopTuningMotion()}>Stop</vscode-button>
            </div>
          </div>
        </section>
      </div>
      <aside class="parameter-column">
        {#each LOOP_SPECS as spec}
          <section class="tuning-section"><div class="section-title">{spec.title}</div><div class="field-grid">
            <div class="field-label">{label(spec.source)}</div>
            {#if metadata[spec.source]}
              <select class:ramModified={$modifiedParameterIds.has(metadata[spec.source].id)} class="compact-select" disabled={locked(spec.source)} value={sourceText(spec.source)} onchange={(event) => void setSource(spec.source, event.currentTarget.value)}>
                <option value="Bandwidth">Bandwidth</option><option value="Manual">Manual</option>
              </select>
            {:else}<span class="unavailable">Firmware unavailable</span>{/if}
            <div class="field-label">{label(spec.bandwidth)}</div><div class="editor">
<input class:ramModified={!!metadata[spec.bandwidth] && $modifiedParameterIds.has(metadata[spec.bandwidth].id)} class:dirty={dirty(spec.bandwidth)} class="compact-input mono" value={drafts[spec.bandwidth] ?? ""} disabled={locked(spec.bandwidth)} oninput={(event) => edits.edit(spec.bandwidth, event.currentTarget.value)} onkeydown={(event) => keydown(event, spec.bandwidth)} onblur={() => edits.discard(spec.bandwidth)} /><span class="unit">{unit(spec.bandwidth)}</span></div>
            {#each spec.gains as gain}
              <div class="field-label">{label(gain)}</div><div class="editor">
<input class:ramModified={!!metadata[gain] && $modifiedParameterIds.has(metadata[gain].id)} class:dirty={dirty(gain)} class="compact-input mono" value={drafts[gain] ?? ""} disabled={locked(gain)} oninput={(event) => edits.edit(gain, event.currentTarget.value)} onkeydown={(event) => keydown(event, gain)} onblur={() => edits.discard(gain)} /><span class="unit">{unit(gain)}</span></div>
            {/each}
          </div></section>
        {/each}
        <section class="tuning-section"><div class="section-title">Position Loop</div><div class="field-grid"><div class="field-label">{label(POSITION_KP)}</div><div class="editor"><input class:ramModified={!!metadata[POSITION_KP] && $modifiedParameterIds.has(metadata[POSITION_KP].id)} class:dirty={dirty(POSITION_KP)} class="compact-input mono" value={drafts[POSITION_KP] ?? ""} disabled={locked(POSITION_KP)} oninput={(event) => edits.edit(POSITION_KP, event.currentTarget.value)} onkeydown={(event) => keydown(event, POSITION_KP)} onblur={() => edits.discard(POSITION_KP)} /><span class="unit">{unit(POSITION_KP)}</span></div></div></section>
        <section class="tuning-section"><div class="section-title">Mechanical Observer</div><div class="field-grid"><div class="field-label">{label(ESO_BW)}</div><div class="editor"><input class:ramModified={!!metadata[ESO_BW] && $modifiedParameterIds.has(metadata[ESO_BW].id)} class:dirty={dirty(ESO_BW)} class="compact-input mono" value={drafts[ESO_BW] ?? ""} disabled={locked(ESO_BW)} oninput={(event) => edits.edit(ESO_BW, event.currentTarget.value)} onkeydown={(event) => keydown(event, ESO_BW)} onblur={() => edits.discard(ESO_BW)} /><span class="unit">{unit(ESO_BW)}</span></div></div></section>
        {#if loading}<div class="loading-note"><i class="codicon codicon-loading codicon-modifier-spin"></i> Reading tuning parameters…</div>{/if}
        {#if motorState === MOTOR_RUN}<div class="state-note">Tuning writes are locked while the motor is RUN.</div>{/if}
      </aside></div>
    {/if}
  </section>
</div>

<style>
  .tuning-root { min-width: 0; min-height: 0; display: grid; grid-template-rows: auto 1fr; }
  .tuning-content { min-width: 0; min-height: 0; overflow: auto; padding: 20px 24px 32px; }
  .dirty-note { margin-left: auto; color: var(--vscode-descriptionForeground); font-size: 11px; }
  .tuning-layout { min-width: 900px; display: grid; grid-template-columns: minmax(0, 1.7fr) minmax(320px, 0.9fr); gap: 24px; align-items: start; }
  .experiment-column { min-width: 0; display: grid; gap: 18px; }
  .waveform-panel, .tuning-section { border: 1px solid var(--vscode-panel-border); border-radius: 4px; background: color-mix(in srgb, var(--vscode-editor-background) 96%, var(--vscode-foreground) 4%); }
  .waveform-panel { padding: 14px; min-height: 430px; }
  .waveform-heading { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
  .waveform-tabs { display: flex; align-items: center; gap: 14px; margin-top: 8px; border-bottom: 1px solid var(--vscode-panel-border); }
  .waveform-tabs button { appearance: none; border: 0; border-bottom: 1px solid transparent; background: transparent; color: var(--vscode-descriptionForeground); padding: 5px 1px 6px; margin-bottom: -1px; font: inherit; font-size: 11px; cursor: pointer; }
  .waveform-tabs button:hover { color: var(--vscode-foreground); }
  .waveform-tabs button.active { color: var(--vscode-foreground); border-bottom-color: var(--vscode-focusBorder); }
  .waveform-tab-content { min-width: 0; min-height: 340px; height: 340px; padding-top: 8px; }
  .waveform-tools { height: 20px; display: flex; justify-content: flex-end; gap: 6px; align-items: baseline; color: var(--vscode-descriptionForeground); font-size: 10px; }
  .waveform-tools strong { color: var(--vscode-foreground); font-weight: 500; }
  .tuning-channel-list, .tuning-scale-list { height: 100%; overflow: auto; }
  .tuning-channel-row, .tuning-scale-row { min-height: 30px; display: grid; grid-template-columns: minmax(0, 1fr) 94px; align-items: center; gap: 10px; border-bottom: 1px solid color-mix(in srgb, var(--vscode-panel-border) 55%, transparent); font-size: 12px; }
  .tuning-channel-row label { min-width: 0; display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 7px; }
  .tuning-channel-row small { color: var(--vscode-descriptionForeground); font-size: 10px; }
  .tuning-channel-limits { display: flex; gap: 16px; padding-top: 8px; color: var(--vscode-descriptionForeground); font-size: 10px; }
  .tuning-channel-limits.invalid { color: var(--nmixx-status-errorForeground); }
  .tuning-scale-row > span:first-child { min-width: 0; }
  .scale-editor { display: grid; grid-template-columns: auto 1fr; gap: 5px; align-items: center; }
  .scale-editor input { width: 72px; }
  .tuning-scale-actions { display: flex; justify-content: flex-end; padding-top: 8px; }
  .tuning-scale-actions button { appearance: none; border: 0; background: transparent; color: var(--vscode-descriptionForeground); padding: 3px 0; font: inherit; font-size: 11px; cursor: pointer; }
  .tuning-scale-actions button:hover { color: var(--vscode-foreground); }
  .experiment-status { color: var(--vscode-descriptionForeground); font-size: 10px; font-weight: 600; }
  .experiment-status.failed, .experiment-message { color: var(--nmixx-status-errorForeground); }
  .experiment-message { margin: -3px 0 8px; font-size: 11px; }
  .motion-panel { border: 1px solid var(--vscode-panel-border); border-radius: 4px; background: color-mix(in srgb, var(--vscode-editor-background) 96%, var(--vscode-foreground) 4%); padding: 14px; }
  .motion-title { font-size: 16px; margin-bottom: 14px; }
  .motion-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 18px; margin-bottom: 12px; }
  .motion-mode { display: flex; align-items: center; gap: 10px; color: var(--vscode-descriptionForeground); font-size: 12px; }
  .motion-mode-select { min-width: 130px; }
  .position-mode-options { display: flex; align-items: center; gap: 18px; color: var(--vscode-descriptionForeground); font-size: 12px; }
  .position-mode-options label, .copy-decel { display: inline-flex; align-items: center; gap: 6px; }
  .motion-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px 22px; }
  .motion-grid > label { display: grid; grid-template-columns: 88px minmax(0, 1fr); align-items: center; gap: 10px; color: var(--vscode-descriptionForeground); font-size: 12px; }
  .motion-editor { display: grid; grid-template-columns: minmax(100px, 1fr) auto; align-items: center; gap: 8px; }
  .motion-footer { margin-top: 14px; display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .copy-decel { color: var(--vscode-descriptionForeground); font-size: 12px; }
  .motion-actions { display: flex; align-items: center; gap: 8px; }
  .parameter-column { display: grid; gap: 10px; }
  .tuning-section { padding: 11px 12px; }
  .section-title { margin-bottom: 9px; font-size: 13px; font-weight: 600; }
  .field-grid { display: grid; grid-template-columns: minmax(130px, 1fr) minmax(150px, 1.1fr); column-gap: 14px; align-items: center; }
  .field-grid > * { min-height: 32px; border-bottom: 1px solid color-mix(in srgb, var(--vscode-panel-border) 55%, transparent); }
  .field-label { display: flex; align-items: center; font-size: 12px; font-weight: 600; }
  .editor { display: grid; grid-template-columns: minmax(90px, 1fr) auto; align-items: center; gap: 7px; }
  .compact-input { width: 100%; min-width: 0; }
  .compact-input.dirty { border-color: var(--vscode-inputValidation-warningBorder, var(--vscode-focusBorder)); }
  .compact-select { height: 26px; align-self: center; border: 1px solid var(--vscode-dropdown-border, var(--vscode-input-border)); background: var(--vscode-dropdown-background, var(--vscode-input-background)); color: var(--vscode-dropdown-foreground, var(--vscode-input-foreground)); padding: 0 7px; font: inherit; font-size: 12px; }
  .unit, .unavailable, .loading-note, .state-note { color: var(--vscode-descriptionForeground); font-size: 11px; }
  .loading-note, .state-note { padding: 2px 4px; }
  @media (max-width: 980px) { .tuning-layout { min-width: 0; grid-template-columns: 1fr; } .motion-grid { grid-template-columns: 1fr; } .motion-toolbar, .motion-footer { align-items: flex-start; flex-direction: column; } }
</style>
