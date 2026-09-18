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
  let generation = 0;

  onMount(() => {
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
    return metadata[symbol]?.name ?? symbol;
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
    if (!Number.isFinite(parsed)) throw new Error(`${meta.symbol}: value must be finite.`);

    if (meta.typeName === "f32") return { type: "f32", value: parsed };
    if (meta.typeName === "u8") {
      if (!Number.isInteger(parsed) || parsed < 0 || parsed > 255) throw new Error(`${meta.symbol}: expected u8.`);
      return { type: "u8", value: parsed };
    }
    throw new Error(`${meta.symbol}: unsupported tuning type ${meta.typeName}.`);
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
            <div class="section-title">Motion Command</div>
            <div class="reserved-motion">
              <span>Experiment motion and finite Run workflow are not implemented yet.</span>
              <vscode-button disabled title={hasDirtyDraft() ? "Commit or discard edited values first" : "Tuning experiment engine is not implemented yet"}>Run</vscode-button>
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
  .motion-panel,
  .tuning-section {
    border: 1px solid var(--vscode-panel-border);
    border-radius: 4px;
    background: color-mix(in srgb, var(--vscode-editor-background) 96%, var(--vscode-foreground) 4%);
  }

  .waveform-panel,
  .motion-panel {
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

  .reserved-motion {
    min-height: 72px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    color: var(--vscode-descriptionForeground);
    font-size: 12px;
  }

  .parameter-column {
    display: grid;
    gap: 14px;
  }

  .tuning-section {
    padding: 14px;
  }

  .section-title {
    margin-bottom: 12px;
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
    min-height: 38px;
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
  }
</style>
