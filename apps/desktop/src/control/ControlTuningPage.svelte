<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { ConnectionInfo } from "../connection/types";
  import {
    listParameters,
    onParametersRefreshed,
    readCachedParameters,
    readCurrentParameters,
    readParameters,
    writeParameter,
  } from "../parameters/api";
  import type { ParameterMetadata, ParameterValue } from "../parameters/types";
  import { modifiedParameterIds } from "../parameters/persistence";
  import {
    executeMotion,
    initializeMotion,
    motionState,
    stopMotionExecution,
    updateMotion,
  } from "../motion/store";
  import type { MotionMode, MotionState } from "../motion/types";

  type Props = {
    connection: ConnectionInfo | undefined;
    motorState: number | null;
    onError?: (error: unknown) => void;
  };

  type LoopSpec = {
    title: string;
    bandwidth: string;
    source: string;
    gains: string[];
  };

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
    {
      title: "Current Loop",
      bandwidth: CURRENT_BW,
      source: CURRENT_SOURCE,
      gains: [ID_KP, ID_KI, IQ_KP, IQ_KI],
    },
    {
      title: "Speed Loop",
      bandwidth: SPEED_BW,
      source: SPEED_SOURCE,
      gains: [SPEED_KP, SPEED_KI],
    },
  ];

  const SYMBOLS = [
    CURRENT_BW, CURRENT_SOURCE, ID_KP, ID_KI, IQ_KP, IQ_KI,
    SPEED_BW, SPEED_SOURCE, SPEED_KP, SPEED_KI,
    POSITION_KP, ESO_BW,
  ];

  let { connection, motorState, onError = () => undefined }: Props = $props();

  let metadata = $state<Record<string, ParameterMetadata>>({});
  let values = $state<Record<string, ParameterValue | null>>({});
  let drafts = $state<Record<string, string>>({});
  let writing = $state<Set<string>>(new Set());
  let loading = $state(false);
  let motionActionBusy = $state(false);
  let copyAccelToDecel = $state(false);
  let generation = 0;

  onMount(() => {
    void initializeMotion().catch(onError);

    let disposed = false;
    let unlisten: (() => void) | undefined;
    onParametersRefreshed(() => void refreshFromCache())
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(onError);
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  $effect(() => {
    const activeConnection = connection;
    const token = ++generation;

    if (!activeConnection) {
      metadata = {};
      values = {};
      drafts = {};
      writing = new Set();
      loading = false;
      return;
    }

    untrack(() => void load(activeConnection, token));
  });

  function valueText(value: ParameterValue | null | undefined): string {
    if (!value) return "";
    if (value.type === "position") return `${value.value.turns}, ${value.value.theta}`;
    if (value.type === "f32") return Number(value.value).toPrecision(7).replace(/(?:\.0+|(\.\d+?)0+)$/, "$1");
    return String(value.value);
  }

  function label(symbol: string): string {
    return metadata[symbol]?.label ?? "Unavailable";
  }

  function unit(symbol: string): string {
    return metadata[symbol]?.unit ?? "";
  }

  function writable(symbol: string): boolean {
    return metadata[symbol]?.access.toLowerCase().includes("w") ?? false;
  }

  function locked(symbol: string): boolean {
    return motorState === MOTOR_RUN || writing.has(symbol) || !writable(symbol);
  }

  function dirty(symbol: string): boolean {
    return (drafts[symbol] ?? "") !== valueText(values[symbol]);
  }

  function hasDirtyDraft(): boolean {
    return SYMBOLS.some((symbol) => metadata[symbol] && dirty(symbol));
  }

  function numeric(value: ParameterValue | null | undefined): number | null {
    if (!value || value.type === "position") return null;
    return Number(value.value);
  }

  function enumValue(symbol: string, enumSymbol: string): number | null {
    const meta = metadata[symbol];
    if (!meta) return null;
    const index = meta.allowedSymbols.indexOf(enumSymbol);
    if (index < 0) return null;
    return meta.allowed[index] ?? index;
  }

  function sourceText(symbol: string): string {
    const value = numeric(values[symbol]);
    const bw = enumValue(symbol, "CTRL_TUNE_BANDWIDTH");
    const manual = enumValue(symbol, "CTRL_TUNE_MANUAL");
    if (value === bw) return "Bandwidth";
    if (value === manual) return "Manual";
    return value === null ? "—" : String(value);
  }

  function parseValue(meta: ParameterMetadata, text: string): ParameterValue {
    const parsed = Number(text.trim());
    if (!Number.isFinite(parsed)) throw new Error(`${meta.label}: value must be finite.`);

    if (meta.typeName === "f32") return { type: "f32", value: parsed };
    if (meta.typeName === "u8") {
      if (!Number.isInteger(parsed) || parsed < 0 || parsed > 255) throw new Error(`${meta.label}: expected u8.`);
      return { type: "u8", value: parsed };
    }
    throw new Error(`${meta.label}: unsupported tuning type ${meta.typeName}.`);
  }

  function applyValues(entries: ParameterMetadata[], results: Awaited<ReturnType<typeof readParameters>>) {
    const byId = new Map(results.map((item) => [item.id, item]));
    const nextValues = { ...values };
    const nextDrafts = { ...drafts };

    for (const item of entries) {
      const result = byId.get(item.id);
      nextValues[item.symbol] = result?.value ?? null;
      if (result?.value) nextDrafts[item.symbol] = valueText(result.value);
    }

    values = nextValues;
    drafts = nextDrafts;
  }

  async function load(activeConnection: ConnectionInfo, token: number) {
    loading = true;
    try {
      const registry = await listParameters();
      if (token !== generation || connection !== activeConnection) return;

      const wanted = new Set(SYMBOLS);
      const entries = registry.filter((item) => wanted.has(item.symbol));
      metadata = Object.fromEntries(entries.map((item) => [item.symbol, item]));

      const readable = entries.filter((item) => item.access.toLowerCase().includes("r"));
      const results = await readCurrentParameters(readable.map((item) => item.id));
      if (token !== generation || connection !== activeConnection) return;
      applyValues(readable, results);
    } catch (error) {
      if (token === generation) onError(error);
    } finally {
      if (token === generation) loading = false;
    }
  }

  async function refreshFromCache() {
    if (!connection || Object.keys(metadata).length === 0) return;
    try {
      const readable = Object.values(metadata).filter((item) => item.access.toLowerCase().includes("r"));
      const results = await readCachedParameters(readable.map((item) => item.id));
      applyValues(readable, results);
    } catch (error) {
      onError(error);
    }
  }

  async function refreshSymbols(symbols: string[]) {
    const entries = symbols.map((symbol) => metadata[symbol]).filter((item): item is ParameterMetadata => !!item);
    if (entries.length === 0) return;
    const results = await readParameters(entries.map((item) => item.id));
    applyValues(entries, results);
  }

  async function commit(symbol: string, refresh: string[] = [symbol]) {
    const meta = metadata[symbol];
    if (!meta || locked(symbol) || !dirty(symbol)) return;

    writing = new Set(writing).add(symbol);
    try {
      await writeParameter(meta.id, parseValue(meta, drafts[symbol] ?? ""));
      await refreshSymbols(refresh);
    } catch (error) {
      drafts = { ...drafts, [symbol]: valueText(values[symbol]) };
      onError(error);
    } finally {
      const next = new Set(writing);
      next.delete(symbol);
      writing = next;
    }
  }

  async function setSource(symbol: string, enumSymbol: "CTRL_TUNE_BANDWIDTH" | "CTRL_TUNE_MANUAL", refresh: string[]) {
    const meta = metadata[symbol];
    const value = enumValue(symbol, enumSymbol);
    if (!meta || value === null || locked(symbol)) return;

    writing = new Set(writing).add(symbol);
    try {
      await writeParameter(meta.id, { type: "u8", value });
      await refreshSymbols(refresh);
    } catch (error) {
      onError(error);
    } finally {
      const next = new Set(writing);
      next.delete(symbol);
      writing = next;
    }
  }

  function keydown(event: KeyboardEvent, symbol: string, refresh: string[] = [symbol]) {
    if (event.key === "Enter") {
      event.preventDefault();
      void commit(symbol, refresh);
      (event.currentTarget as HTMLInputElement).blur();
    } else if (event.key === "Escape") {
      drafts = { ...drafts, [symbol]: valueText(values[symbol]) };
      (event.currentTarget as HTMLInputElement).blur();
    }
  }

  function sourceSymbols(spec: LoopSpec): string[] {
    return [spec.source, spec.bandwidth, ...spec.gains];
  }

  function gainRefresh(spec: LoopSpec): string[] {
    return [spec.source, ...spec.gains, spec.bandwidth];
  }

  const motionModeLabels: Record<MotionMode, string> = {
    position: "Position",
    speed: "Speed",
    "sensorless-speed": "Sensorless Speed",
    torque: "Torque",
    mit: "MIT",
  };

  function motionModeSupported(mode: MotionMode): boolean {
    const caps = connection?.motion;
    if (!caps) return false;
    if (mode === "position") return caps.position;
    if (mode === "speed") return caps.speed;
    if (mode === "sensorless-speed") return caps.sensorlessSpeed;
    if (mode === "torque") return caps.torque;
    return caps.mit;
  }

  function motionNumber(event: Event): number {
    return Number((event.currentTarget as HTMLInputElement).value);
  }

  function updateMotionField<K extends keyof MotionState>(key: K, value: MotionState[K]) {
    void updateMotion(key, value).catch(onError);
  }

  function updateAcceleration(value: number) {
    updateMotionField("acceleration", value);
    if (copyAccelToDecel) updateMotionField("deceleration", value);
  }

  function toggleCopyAccelToDecel(checked: boolean) {
    copyAccelToDecel = checked;
    if (checked) updateMotionField("deceleration", $motionState.acceleration);
  }

  function canRunMotion(): boolean {
    return !!connection
      && motorState === MOTOR_ENABLED
      && !motionActionBusy
      && !hasDirtyDraft()
      && connection.motion.run
      && motionModeSupported($motionState.mode);
  }

  function canStopMotion(): boolean {
    return !!connection
      && motorState === MOTOR_RUN
      && !motionActionBusy
      && connection.motion.stop;
  }

  async function runTuningMotion() {
    if (!canRunMotion()) return;
    motionActionBusy = true;
    try {
      await executeMotion();
    } catch (error) {
      onError(error);
    } finally {
      motionActionBusy = false;
    }
  }

  async function stopTuningMotion() {
    if (!canStopMotion()) return;
    motionActionBusy = true;
    try {
      await stopMotionExecution();
    } catch (error) {
      onError(error);
    } finally {
      motionActionBusy = false;
    }
  }
</script>

<div class="tuning-root">
  <section class="page-toolbar">
    <div class="page-title">CONTROL TUNING</div>
    {#if hasDirtyDraft()}<div class="dirty-note">Uncommitted edits</div>{/if}
  </section>

  <section class="tuning-content">
    {#if !connection}
      <div class="empty-state"><i class="codicon codicon-plug"></i><div>Connect a device to tune control loops.</div></div>
    {:else}
      <div class="tuning-layout">
        <div class="experiment-column">
          <section class="waveform-panel">
            <div class="section-title">Experiment Waveform</div>
            <div class="reserved-panel">
              <i class="codicon codicon-graph-line"></i>
              <div>Finite tuning capture will use the shared Scope pipeline.</div>
            </div>
          </section>

          <section class="motion-panel">
            <div class="section-title motion-title">Motion Command</div>

            <div class="motion-toolbar">
              <label class="motion-mode">
                <span>Mode</span>
                <select
                  class="compact-select motion-mode-select"
                  value={$motionState.mode}
                  disabled={motorState === MOTOR_RUN || motionActionBusy}
                  onchange={(event) => updateMotionField("mode", (event.currentTarget as HTMLSelectElement).value as MotionMode)}
                >
                  {#each Object.entries(motionModeLabels) as [value, labelText]}
                    <option value={value} disabled={!motionModeSupported(value as MotionMode)}>{labelText}</option>
                  {/each}
                </select>
              </label>

              {#if $motionState.mode === "position"}
                <div class="position-mode-options" aria-label="Position command mode">
                  <label>
                    <input
                      type="radio"
                      name="tuning-position-command"
                      checked={$motionState.positionCommand === "incremental"}
                      disabled={motorState === MOTOR_RUN || motionActionBusy}
                      onchange={() => updateMotionField("positionCommand", "incremental")}
                    />
                    Incremental
                  </label>
                  <label>
                    <input
                      type="radio"
                      name="tuning-position-command"
                      checked={$motionState.positionCommand === "absolute"}
                      disabled={motorState === MOTOR_RUN || motionActionBusy}
                      onchange={() => updateMotionField("positionCommand", "absolute")}
                    />
                    Absolute
                  </label>
                  <label>
                    <input
                      type="checkbox"
                      checked={$motionState.repeat}
                      disabled={motorState === MOTOR_RUN || motionActionBusy}
                      onchange={(event) => updateMotionField("repeat", (event.currentTarget as HTMLInputElement).checked)}
                    />
                    Repeat
                  </label>
                </div>
              {/if}
            </div>

            {#if $motionState.mode === "position"}
              <div class="motion-grid">
                <label>
                  <span>Position</span>
                  <span class="motion-editor">
                    <input
                      class="compact-input mono"
                      type="number"
                      value={$motionState.positionTargetTurn}
                      disabled={motorState === MOTOR_RUN || motionActionBusy}
                      oninput={(event) => updateMotionField("positionTargetTurn", motionNumber(event))}
                    />
                    <span class="unit">turn</span>
                  </span>
                </label>
                <label>
                  <span>Max Speed</span>
                  <span class="motion-editor">
                    <input
                      class="compact-input mono"
                      type="number"
                      value={$motionState.positionMaxSpeed}
                      disabled={motorState === MOTOR_RUN || motionActionBusy}
                      oninput={(event) => updateMotionField("positionMaxSpeed", motionNumber(event))}
                    />
                    <span class="unit">rad/s</span>
                  </span>
                </label>
                <label>
                  <span>Accel</span>
                  <span class="motion-editor">
                    <input
                      class="compact-input mono"
                      type="number"
                      value={$motionState.acceleration}
                      disabled={motorState === MOTOR_RUN || motionActionBusy}
                      oninput={(event) => updateAcceleration(motionNumber(event))}
                    />
                    <span class="unit">rad/s²</span>
                  </span>
                </label>
                <label>
                  <span>Decel</span>
                  <span class="motion-editor">
                    <input
                      class="compact-input mono"
                      type="number"
                      value={$motionState.deceleration}
                      disabled={motorState === MOTOR_RUN || motionActionBusy || copyAccelToDecel}
                      oninput={(event) => updateMotionField("deceleration", motionNumber(event))}
                    />
                    <span class="unit">rad/s²</span>
                  </span>
                </label>
              </div>
            {:else if $motionState.mode === "speed" || $motionState.mode === "sensorless-speed"}
              <div class="motion-grid">
                <label>
                  <span>Target Speed</span>
                  <span class="motion-editor">
                    <input
                      class="compact-input mono"
                      type="number"
                      value={$motionState.mode === "speed" ? $motionState.speedTarget : $motionState.sensorlessSpeedTarget}
                      disabled={motorState === MOTOR_RUN || motionActionBusy}
                      oninput={(event) => $motionState.mode === "speed"
                        ? updateMotionField("speedTarget", motionNumber(event))
                        : updateMotionField("sensorlessSpeedTarget", motionNumber(event))}
                    />
                    <span class="unit">rad/s</span>
                  </span>
                </label>
                <div></div>
                <label>
                  <span>Accel</span>
                  <span class="motion-editor">
                    <input
                      class="compact-input mono"
                      type="number"
                      value={$motionState.acceleration}
                      disabled={motorState === MOTOR_RUN || motionActionBusy}
                      oninput={(event) => updateAcceleration(motionNumber(event))}
                    />
                    <span class="unit">rad/s²</span>
                  </span>
                </label>
                <label>
                  <span>Decel</span>
                  <span class="motion-editor">
                    <input
                      class="compact-input mono"
                      type="number"
                      value={$motionState.deceleration}
                      disabled={motorState === MOTOR_RUN || motionActionBusy || copyAccelToDecel}
                      oninput={(event) => updateMotionField("deceleration", motionNumber(event))}
                    />
                    <span class="unit">rad/s²</span>
                  </span>
                </label>
              </div>
            {:else if $motionState.mode === "torque"}
              <div class="motion-grid">
                <label>
                  <span>Torque</span>
                  <span class="motion-editor">
                    <input
                      class="compact-input mono"
                      type="number"
                      value={$motionState.torqueTargetNm}
                      disabled={motorState === MOTOR_RUN || motionActionBusy}
                      oninput={(event) => updateMotionField("torqueTargetNm", motionNumber(event))}
                    />
                    <span class="unit">N·m</span>
                  </span>
                </label>
                <label>
                  <span>Ramp</span>
                  <span class="motion-editor">
                    <input
                      class="compact-input mono"
                      type="number"
                      value={$motionState.torqueRampNmPerS}
                      disabled={motorState === MOTOR_RUN || motionActionBusy}
                      oninput={(event) => updateMotionField("torqueRampNmPerS", motionNumber(event))}
                    />
                    <span class="unit">N·m/s</span>
                  </span>
                </label>
              </div>
            {:else}
              <div class="motion-grid">
                <label>
                  <span>Position</span>
                  <span class="motion-editor">
                    <input class="compact-input mono" type="number" value={$motionState.mitPositionRef} disabled={motorState === MOTOR_RUN || motionActionBusy} oninput={(event) => updateMotionField("mitPositionRef", motionNumber(event))} />
                    <span class="unit">turn</span>
                  </span>
                </label>
                <label>
                  <span>Velocity</span>
                  <span class="motion-editor">
                    <input class="compact-input mono" type="number" value={$motionState.mitVelocityRef} disabled={motorState === MOTOR_RUN || motionActionBusy} oninput={(event) => updateMotionField("mitVelocityRef", motionNumber(event))} />
                    <span class="unit">rad/s</span>
                  </span>
                </label>
                <label>
                  <span>Kp</span>
                  <span class="motion-editor"><input class="compact-input mono" type="number" value={$motionState.mitKp} disabled={motorState === MOTOR_RUN || motionActionBusy} oninput={(event) => updateMotionField("mitKp", motionNumber(event))} /></span>
                </label>
                <label>
                  <span>Kd</span>
                  <span class="motion-editor"><input class="compact-input mono" type="number" value={$motionState.mitKd} disabled={motorState === MOTOR_RUN || motionActionBusy} oninput={(event) => updateMotionField("mitKd", motionNumber(event))} /></span>
                </label>
              </div>
            {/if}

            <div class="motion-footer">
              {#if $motionState.mode === "position" || $motionState.mode === "speed" || $motionState.mode === "sensorless-speed"}
                <label class="copy-decel">
                  <input
                    type="checkbox"
                    checked={copyAccelToDecel}
                    disabled={motorState === MOTOR_RUN || motionActionBusy}
                    onchange={(event) => toggleCopyAccelToDecel((event.currentTarget as HTMLInputElement).checked)}
                  />
                  Copy Accel to Decel
                </label>
              {:else}
                <span></span>
              {/if}

              <div class="motion-actions">
                <vscode-button
                  disabled={!canRunMotion()}
                  title={hasDirtyDraft()
                    ? "Commit or discard tuning edits first"
                    : motorState !== MOTOR_ENABLED
                      ? "Enable motor first"
                      : "Run motion"}
                  onclick={() => void runTuningMotion()}
                >Run</vscode-button>
                <vscode-button
                  secondary
                  disabled={!canStopMotion()}
                  title="Stop current motion and return to ENABLED"
                  onclick={() => void stopTuningMotion()}
                >Stop</vscode-button>
              </div>
            </div>
          </section>
        </div>

        <aside class="parameter-column">
          {#each LOOP_SPECS as spec}
            <section class="tuning-section">
              <div class="section-title">{spec.title}</div>

              <div class="field-grid">
                <div class="field-label">{label(spec.source)}</div>
                {#if metadata[spec.source]}
                  <select
                    class:ramModified={$modifiedParameterIds.has(metadata[spec.source].id)}
                    class="compact-select"
                    disabled={locked(spec.source)}
                    value={sourceText(spec.source)}
                    onchange={(event) => {
                      const next = (event.currentTarget as HTMLSelectElement).value;
                      void setSource(
                        spec.source,
                        next === "Manual" ? "CTRL_TUNE_MANUAL" : "CTRL_TUNE_BANDWIDTH",
                        sourceSymbols(spec),
                      );
                    }}
                  >
                    <option value="Bandwidth">Bandwidth</option>
                    <option value="Manual">Manual</option>
                  </select>
                {:else}
                  <span class="unavailable">Firmware unavailable</span>
                {/if}

                <div class="field-label">{label(spec.bandwidth)}</div>
                <div class="editor">
                  <input
                    class:ramModified={!!metadata[spec.bandwidth] && $modifiedParameterIds.has(metadata[spec.bandwidth].id)}
                    class:dirty={dirty(spec.bandwidth)}
                    class="compact-input mono"
                    value={drafts[spec.bandwidth] ?? ""}
                    disabled={!metadata[spec.bandwidth] || locked(spec.bandwidth)}
                    oninput={(event) => drafts = { ...drafts, [spec.bandwidth]: event.currentTarget.value }}
                    onkeydown={(event) => keydown(event, spec.bandwidth, gainRefresh(spec))}
                  />
                  <span class="unit">{unit(spec.bandwidth)}</span>
                </div>

                {#each spec.gains as gain}
                  <div class="field-label">{label(gain)}</div>
                  <div class="editor">
                    <input
                      class:ramModified={!!metadata[gain] && $modifiedParameterIds.has(metadata[gain].id)}
                      class:dirty={dirty(gain)}
                      class="compact-input mono"
                      value={drafts[gain] ?? ""}
                      disabled={!metadata[gain] || locked(gain)}
                      oninput={(event) => drafts = { ...drafts, [gain]: event.currentTarget.value }}
                      onkeydown={(event) => keydown(event, gain, gainRefresh(spec))}
                    />
                    <span class="unit">{unit(gain)}</span>
                  </div>
                {/each}
              </div>
            </section>
          {/each}

          <section class="tuning-section">
            <div class="section-title">Position Loop</div>
            <div class="field-grid">
              <div class="field-label">{label(POSITION_KP)}</div>
              <div class="editor">
                <input
                  class:ramModified={!!metadata[POSITION_KP] && $modifiedParameterIds.has(metadata[POSITION_KP].id)}
                  class:dirty={dirty(POSITION_KP)}
                  class="compact-input mono"
                  value={drafts[POSITION_KP] ?? ""}
                  disabled={!metadata[POSITION_KP] || locked(POSITION_KP)}
                  oninput={(event) => drafts = { ...drafts, [POSITION_KP]: event.currentTarget.value }}
                  onkeydown={(event) => keydown(event, POSITION_KP)}
                />
                <span class="unit">{unit(POSITION_KP)}</span>
              </div>
            </div>
          </section>

          <section class="tuning-section">
            <div class="section-title">Mechanical Observer</div>
            <div class="field-grid">
              <div class="field-label">{label(ESO_BW)}</div>
              <div class="editor">
                <input
                  class:ramModified={!!metadata[ESO_BW] && $modifiedParameterIds.has(metadata[ESO_BW].id)}
                  class:dirty={dirty(ESO_BW)}
                  class="compact-input mono"
                  value={drafts[ESO_BW] ?? ""}
                  disabled={!metadata[ESO_BW] || locked(ESO_BW)}
                  oninput={(event) => drafts = { ...drafts, [ESO_BW]: event.currentTarget.value }}
                  onkeydown={(event) => keydown(event, ESO_BW)}
                />
                <span class="unit">{unit(ESO_BW)}</span>
              </div>
            </div>
          </section>

          {#if loading}<div class="loading-note"><i class="codicon codicon-loading codicon-modifier-spin"></i> Reading tuning parameters…</div>{/if}
          {#if motorState === MOTOR_RUN}<div class="state-note">Tuning writes are locked while the motor is RUN.</div>{/if}
        </aside>
      </div>
    {/if}
  </section>
</div>

<style>
  .tuning-root {
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-rows: auto 1fr;
  }

  .tuning-content {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: 20px 24px 32px;
  }

  .dirty-note {
    margin-left: auto;
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
  }

  .tuning-layout {
    min-width: 900px;
    display: grid;
    grid-template-columns: minmax(0, 1.7fr) minmax(320px, 0.9fr);
    gap: 24px;
    align-items: start;
  }

  .experiment-column {
    min-width: 0;
    display: grid;
    gap: 18px;
  }

  .waveform-panel,
  .tuning-section {
    border: 1px solid var(--vscode-panel-border);
    border-radius: 4px;
    background: color-mix(in srgb, var(--vscode-editor-background) 96%, var(--vscode-foreground) 4%);
  }

  .waveform-panel {
    padding: 14px;
  }

  .waveform-panel {
    min-height: 430px;
  }

  .reserved-panel {
    min-height: 370px;
    display: grid;
    place-items: center;
    align-content: center;
    gap: 10px;
    color: var(--vscode-descriptionForeground);
    border: 1px dashed var(--vscode-panel-border);
  }

  .reserved-panel .codicon {
    font-size: 24px;
  }

  .motion-panel {
    border: 1px solid var(--vscode-panel-border);
    border-radius: 4px;
    background: color-mix(in srgb, var(--vscode-editor-background) 96%, var(--vscode-foreground) 4%);
    padding: 14px;
  }

  .motion-title {
    font-size: 16px;
    margin-bottom: 14px;
  }

  .motion-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    margin-bottom: 12px;
  }

  .motion-mode {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--vscode-descriptionForeground);
    font-size: 12px;
  }

  .motion-mode-select {
    min-width: 130px;
  }

  .position-mode-options {
    display: flex;
    align-items: center;
    gap: 18px;
    color: var(--vscode-descriptionForeground);
    font-size: 12px;
  }

  .position-mode-options label,
  .copy-decel {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .motion-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px 22px;
  }

  .motion-grid > label {
    display: grid;
    grid-template-columns: 88px minmax(0, 1fr);
    align-items: center;
    gap: 10px;
    color: var(--vscode-descriptionForeground);
    font-size: 12px;
  }

  .motion-editor {
    display: grid;
    grid-template-columns: minmax(100px, 1fr) auto;
    align-items: center;
    gap: 8px;
  }

  .motion-footer {
    margin-top: 14px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .copy-decel {
    color: var(--vscode-descriptionForeground);
    font-size: 12px;
  }

  .motion-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .parameter-column {
    display: grid;
    gap: 10px;
  }

  .tuning-section {
    padding: 11px 12px;
  }

  .section-title {
    margin-bottom: 9px;
    font-size: 13px;
    font-weight: 600;
  }

  .field-grid {
    display: grid;
    grid-template-columns: minmax(130px, 1fr) minmax(150px, 1.1fr);
    column-gap: 14px;
    align-items: center;
  }

  .field-grid > * {
    min-height: 32px;
    border-bottom: 1px solid color-mix(in srgb, var(--vscode-panel-border) 55%, transparent);
  }

  .field-label {
    display: flex;
    align-items: center;
    font-size: 12px;
    font-weight: 600;
  }

  .editor {
    display: grid;
    grid-template-columns: minmax(90px, 1fr) auto;
    align-items: center;
    gap: 7px;
  }

  .compact-input {
    width: 100%;
    min-width: 0;
  }

  .compact-input.dirty {
    border-color: var(--vscode-inputValidation-warningBorder, var(--vscode-focusBorder));
  }

  .compact-select {
    height: 26px;
    align-self: center;
    border: 1px solid var(--vscode-dropdown-border, var(--vscode-input-border));
    background: var(--vscode-dropdown-background, var(--vscode-input-background));
    color: var(--vscode-dropdown-foreground, var(--vscode-input-foreground));
    padding: 0 7px;
    font: inherit;
    font-size: 12px;
  }

  .unit,
  .unavailable,
  .loading-note,
  .state-note {
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
  }

  .loading-note,
  .state-note {
    padding: 2px 4px;
  }

  @media (max-width: 980px) {
    .tuning-layout {
      min-width: 0;
      grid-template-columns: 1fr;
    }

    .motion-grid {
      grid-template-columns: 1fr;
    }

    .motion-toolbar,
    .motion-footer {
      align-items: flex-start;
      flex-direction: column;
    }
  }
</style>
