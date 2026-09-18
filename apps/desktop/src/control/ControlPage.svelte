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

  type ControlLoopPage = "current" | "speed" | "position";

  type Props = {
    connection: ConnectionInfo | undefined;
    motorState: number | null;
    loop?: ControlLoopPage;
    onError?: (error: unknown) => void;
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

  const SYMBOLS = [
    CURRENT_BW, CURRENT_SOURCE, ID_KP, ID_KI, IQ_KP, IQ_KI,
    SPEED_BW, SPEED_SOURCE, SPEED_KP, SPEED_KI,
    POSITION_KP, ESO_BW,
  ];

  let { connection, motorState, loop = "current", onError = () => undefined }: Props = $props();

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

  function label(symbol: string, fallback: string): string {
    return metadata[symbol]?.name ?? fallback;
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

  function numeric(value: ParameterValue | null | undefined): number | null {
    if (!value || value.type === "position") return null;
    const number = Number(value.value);
    return Number.isFinite(number) ? number : null;
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
    throw new Error(`${meta.symbol}: unsupported control type ${meta.typeName}.`);
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

  async function setSource(symbol: string, mode: "Bandwidth" | "Manual", refresh: string[]) {
    const meta = metadata[symbol];
    const value = enumValue(symbol, mode === "Manual" ? "CTRL_TUNE_MANUAL" : "CTRL_TUNE_BANDWIDTH");
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
</script>

<div class="control-root">
  <section class="page-toolbar">
    <div class="page-title">CONTROL ARCHITECTURE · {loop === "current" ? "CURRENT LOOP" : loop === "speed" ? "SPEED LOOP" : "POSITION LOOP"}</div>
    {#if loading}<div class="toolbar-note"><i class="codicon codicon-loading codicon-modifier-spin"></i> Reading control parameters…</div>{/if}
  </section>

  <section class="control-content">
    {#if !connection}
      <div class="empty-state"><i class="codicon codicon-plug"></i><div>Connect a device to configure control structure.</div></div>
    {:else}
      <div class="control-sheet">
        {#if loop === "current"}
        <section class="loop-section">
          <div class="loop-heading">
            <div>
              <div class="section-title">Current Loop</div>
              <div class="section-subtitle">d/q current regulation</div>
            </div>
            <label class="algorithm-select">Controller
              <select disabled><option>PI</option></select>
            </label>
          </div>

          <div class="diagram current-diagram">
            <div class="signal-chip">Id / Iq Ref</div>
            <div class="arrow">→</div>
            <div class="sum-node">Σ</div>
            <div class="arrow">→</div>
            <div class="control-block wide-block">
              <div class="block-title">PI Current Controller</div>
              {#if metadata[CURRENT_SOURCE]}
                <div class="block-field">
                  <span>{label(CURRENT_SOURCE, "Gain Source")}</span>
                  <select
                    class:ramModified={$modifiedParameterIds.has(metadata[CURRENT_SOURCE].id)}
                    class="compact-select"
                    disabled={locked(CURRENT_SOURCE)}
                    value={sourceText(CURRENT_SOURCE)}
                    onchange={(event) => void setSource(CURRENT_SOURCE, event.currentTarget.value as "Bandwidth" | "Manual", [CURRENT_SOURCE, CURRENT_BW, ID_KP, ID_KI, IQ_KP, IQ_KI])}
                  >
                    <option value="Bandwidth">Bandwidth</option>
                    <option value="Manual">Manual</option>
                  </select>
                </div>
              {/if}
              {#if metadata[CURRENT_BW]}
                <div class="block-field">
                  <span>{label(CURRENT_BW, "Bandwidth")}</span>
                  <span class="editor"><input class:ramModified={$modifiedParameterIds.has(metadata[CURRENT_BW].id)} class:dirty={dirty(CURRENT_BW)} class="compact-input mono" value={drafts[CURRENT_BW] ?? ""} disabled={locked(CURRENT_BW)} oninput={(event) => drafts = { ...drafts, [CURRENT_BW]: event.currentTarget.value }} onkeydown={(event) => keydown(event, CURRENT_BW, [CURRENT_BW, ID_KP, ID_KI, IQ_KP, IQ_KI])} /><span class="unit">{unit(CURRENT_BW)}</span></span>
                </div>
              {/if}
              <div class="gain-grid">
                {#if metadata[ID_KP]}<div><span>Id Kp</span><input class:ramModified={$modifiedParameterIds.has(metadata[ID_KP].id)} class:dirty={dirty(ID_KP)} class="compact-input mono gain-input" value={drafts[ID_KP] ?? ""} disabled={locked(ID_KP)} oninput={(event) => drafts = { ...drafts, [ID_KP]: event.currentTarget.value }} onkeydown={(event) => keydown(event, ID_KP, [CURRENT_SOURCE, CURRENT_BW, ID_KP, ID_KI, IQ_KP, IQ_KI])} /></div>{/if}
                {#if metadata[ID_KI]}<div><span>Id Ki</span><input class:ramModified={$modifiedParameterIds.has(metadata[ID_KI].id)} class:dirty={dirty(ID_KI)} class="compact-input mono gain-input" value={drafts[ID_KI] ?? ""} disabled={locked(ID_KI)} oninput={(event) => drafts = { ...drafts, [ID_KI]: event.currentTarget.value }} onkeydown={(event) => keydown(event, ID_KI, [CURRENT_SOURCE, CURRENT_BW, ID_KP, ID_KI, IQ_KP, IQ_KI])} /></div>{/if}
                {#if metadata[IQ_KP]}<div><span>Iq Kp</span><input class:ramModified={$modifiedParameterIds.has(metadata[IQ_KP].id)} class:dirty={dirty(IQ_KP)} class="compact-input mono gain-input" value={drafts[IQ_KP] ?? ""} disabled={locked(IQ_KP)} oninput={(event) => drafts = { ...drafts, [IQ_KP]: event.currentTarget.value }} onkeydown={(event) => keydown(event, IQ_KP, [CURRENT_SOURCE, CURRENT_BW, ID_KP, ID_KI, IQ_KP, IQ_KI])} /></div>{/if}
                {#if metadata[IQ_KI]}<div><span>Iq Ki</span><input class:ramModified={$modifiedParameterIds.has(metadata[IQ_KI].id)} class:dirty={dirty(IQ_KI)} class="compact-input mono gain-input" value={drafts[IQ_KI] ?? ""} disabled={locked(IQ_KI)} oninput={(event) => drafts = { ...drafts, [IQ_KI]: event.currentTarget.value }} onkeydown={(event) => keydown(event, IQ_KI, [CURRENT_SOURCE, CURRENT_BW, ID_KP, ID_KI, IQ_KP, IQ_KI])} /></div>{/if}
              </div>
            </div>
            <div class="arrow">→</div>
            <div class="signal-chip">Ud / Uq</div>
          </div>
          <div class="feedback-row"><span>Current Feedback</span><span class="feedback-line">───────────────↩</span></div>
        </section>
        {:else if loop === "speed"}
        <section class="loop-section">
          <div class="loop-heading">
            <div>
              <div class="section-title">Speed Loop</div>
              <div class="section-subtitle">mechanical speed to q-axis current reference</div>
            </div>
            <label class="algorithm-select">Controller
              <select disabled><option>PI</option></select>
            </label>
          </div>

          <div class="diagram speed-diagram">
            <div class="signal-chip">ωm Ref</div>
            <div class="arrow">→</div>
            <div class="sum-node">Σ</div>
            <div class="arrow">→</div>
            <div class="control-block">
              <div class="block-title">PI Speed Controller</div>
              {#if metadata[SPEED_SOURCE]}
                <div class="block-field">
                  <span>{label(SPEED_SOURCE, "Gain Source")}</span>
                  <select class:ramModified={$modifiedParameterIds.has(metadata[SPEED_SOURCE].id)} class="compact-select" disabled={locked(SPEED_SOURCE)} value={sourceText(SPEED_SOURCE)} onchange={(event) => void setSource(SPEED_SOURCE, event.currentTarget.value as "Bandwidth" | "Manual", [SPEED_SOURCE, SPEED_BW, SPEED_KP, SPEED_KI])}>
                    <option value="Bandwidth">Bandwidth</option>
                    <option value="Manual">Manual</option>
                  </select>
                </div>
              {/if}
              {#if metadata[SPEED_BW]}
                <div class="block-field">
                  <span>{label(SPEED_BW, "Bandwidth")}</span>
                  <span class="editor"><input class:ramModified={$modifiedParameterIds.has(metadata[SPEED_BW].id)} class:dirty={dirty(SPEED_BW)} class="compact-input mono" value={drafts[SPEED_BW] ?? ""} disabled={locked(SPEED_BW)} oninput={(event) => drafts = { ...drafts, [SPEED_BW]: event.currentTarget.value }} onkeydown={(event) => keydown(event, SPEED_BW, [SPEED_BW, SPEED_KP, SPEED_KI])} /><span class="unit">{unit(SPEED_BW)}</span></span>
                </div>
              {/if}
              <div class="gain-grid two">
                {#if metadata[SPEED_KP]}<div><span>Kp</span><input class:ramModified={$modifiedParameterIds.has(metadata[SPEED_KP].id)} class:dirty={dirty(SPEED_KP)} class="compact-input mono gain-input" value={drafts[SPEED_KP] ?? ""} disabled={locked(SPEED_KP)} oninput={(event) => drafts = { ...drafts, [SPEED_KP]: event.currentTarget.value }} onkeydown={(event) => keydown(event, SPEED_KP, [SPEED_SOURCE, SPEED_BW, SPEED_KP, SPEED_KI])} /></div>{/if}
                {#if metadata[SPEED_KI]}<div><span>Ki</span><input class:ramModified={$modifiedParameterIds.has(metadata[SPEED_KI].id)} class:dirty={dirty(SPEED_KI)} class="compact-input mono gain-input" value={drafts[SPEED_KI] ?? ""} disabled={locked(SPEED_KI)} oninput={(event) => drafts = { ...drafts, [SPEED_KI]: event.currentTarget.value }} onkeydown={(event) => keydown(event, SPEED_KI, [SPEED_SOURCE, SPEED_BW, SPEED_KP, SPEED_KI])} /></div>{/if}
              </div>
            </div>
            <div class="arrow">→</div>
            <div class="signal-chip">Iq Ref</div>
          </div>

          <div class="speed-feedback-path">
            <div class="feedback-source-note">Feedback / Observer Path</div>
            <div class="feedback-path-line">
              <div class="signal-chip feedback-signal">Motor / current signals</div>
              <div class="arrow">→</div>
              <div class="control-block observer-block">
                <div class="block-title">Mechanical ESO</div>
                {#if metadata[ESO_BW]}
                  <div class="block-field">
                    <span>{label(ESO_BW, "Observer Bandwidth")}</span>
                    <span class="editor"><input class:ramModified={$modifiedParameterIds.has(metadata[ESO_BW].id)} class:dirty={dirty(ESO_BW)} class="compact-input mono" value={drafts[ESO_BW] ?? ""} disabled={locked(ESO_BW)} oninput={(event) => drafts = { ...drafts, [ESO_BW]: event.currentTarget.value }} onkeydown={(event) => keydown(event, ESO_BW)} /><span class="unit">{unit(ESO_BW)}</span></span>
                  </div>
                {/if}
              </div>
              <div class="arrow">→</div>
              <div class="signal-chip feedback-signal">Wm feedback</div>
              <div class="feedback-return">↩ to speed error summing point</div>
            </div>
          </div>
        </section>
        {:else}
        <section class="loop-section">
          <div class="loop-heading">
            <div>
              <div class="section-title">Position Loop</div>
              <div class="section-subtitle">position error to mechanical speed reference</div>
            </div>
            <label class="algorithm-select">Controller
              <select disabled><option>P</option></select>
            </label>
          </div>

          <div class="diagram position-diagram">
            <div class="signal-chip">Position Ref</div>
            <div class="arrow">→</div>
            <div class="sum-node">Σ</div>
            <div class="arrow">→</div>
            <div class="control-block compact-block">
              <div class="block-title">P Position Controller</div>
              {#if metadata[POSITION_KP]}
                <div class="block-field">
                  <span>{label(POSITION_KP, "Kp")}</span>
                  <span class="editor"><input class:ramModified={$modifiedParameterIds.has(metadata[POSITION_KP].id)} class:dirty={dirty(POSITION_KP)} class="compact-input mono" value={drafts[POSITION_KP] ?? ""} disabled={locked(POSITION_KP)} oninput={(event) => drafts = { ...drafts, [POSITION_KP]: event.currentTarget.value }} onkeydown={(event) => keydown(event, POSITION_KP)} /><span class="unit">{unit(POSITION_KP)}</span></span>
                </div>
              {/if}
            </div>
            <div class="arrow">→</div>
            <div class="signal-chip">ωm Ref</div>
          </div>
          <div class="feedback-row"><span>Encoder Position</span><span class="feedback-line">───────────────↩</span></div>
        </section>
        {/if}

        {#if motorState === MOTOR_RUN}<div class="state-note">Control parameter writes are locked while the motor is RUN.</div>{/if}
      </div>
    {/if}
  </section>
</div>

<style>
  .control-root {
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-rows: auto 1fr;
  }

  .control-content {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: 20px 28px 36px;
  }

  .control-sheet {
    min-width: 900px;
    max-width: 1180px;
    display: grid;
    gap: 20px;
  }

  .loop-section {
    border: 1px solid var(--vscode-panel-border);
    border-radius: 4px;
    background: color-mix(in srgb, var(--vscode-editor-background) 97%, var(--vscode-foreground) 3%);
    padding: 16px 18px 14px;
  }

  .loop-heading {
    display: flex;
    justify-content: space-between;
    align-items: start;
    gap: 18px;
    margin-bottom: 16px;
  }

  .section-title {
    font-size: 13px;
    font-weight: 600;
  }

  .section-subtitle,
  .toolbar-note,
  .state-note {
    margin-top: 3px;
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
  }

  .toolbar-note {
    margin-left: auto;
    margin-top: 0;
  }

  .algorithm-select {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
  }

  .algorithm-select select,
  .compact-select {
    height: 26px;
    border: 1px solid var(--vscode-dropdown-border, var(--vscode-input-border));
    background: var(--vscode-dropdown-background, var(--vscode-input-background));
    color: var(--vscode-dropdown-foreground, var(--vscode-input-foreground));
    padding: 0 7px;
    font: inherit;
    font-size: 12px;
  }

  .diagram {
    display: grid;
    grid-template-columns: auto 28px 32px 28px minmax(300px, 1fr) 28px auto;
    align-items: center;
    gap: 5px;
    min-height: 142px;
  }

  .signal-chip {
    min-width: 94px;
    padding: 10px 12px;
    text-align: center;
    border: 1px solid var(--vscode-panel-border);
    background: var(--vscode-input-background);
    border-radius: 3px;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
  }

  .arrow {
    text-align: center;
    color: var(--vscode-descriptionForeground);
    font-size: 18px;
  }

  .sum-node {
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border: 1px solid var(--vscode-foreground);
    border-radius: 50%;
    font-size: 13px;
    font-weight: 600;
  }

  .control-block {
    min-width: 0;
    border: 1px solid color-mix(in srgb, var(--vscode-focusBorder) 52%, var(--vscode-panel-border));
    border-radius: 4px;
    padding: 11px 12px;
    background: color-mix(in srgb, var(--vscode-input-background) 88%, transparent);
  }

  .wide-block {
    min-width: 390px;
  }

  .compact-block {
    max-width: 430px;
  }

  .block-title {
    margin-bottom: 9px;
    font-size: 12px;
    font-weight: 600;
  }

  .block-field {
    min-height: 34px;
    display: grid;
    grid-template-columns: minmax(110px, 1fr) minmax(130px, 1.1fr);
    gap: 12px;
    align-items: center;
    border-top: 1px solid color-mix(in srgb, var(--vscode-panel-border) 55%, transparent);
    font-size: 11px;
  }

  .editor {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(80px, 1fr) auto;
    align-items: center;
    gap: 6px;
  }

  .compact-input {
    width: 100%;
    min-width: 0;
  }

  .compact-input.dirty {
    border-color: var(--vscode-inputValidation-warningBorder, var(--vscode-focusBorder));
  }

  .unit {
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
    white-space: nowrap;
  }

  .gain-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(82px, 1fr));
    gap: 7px;
    margin-top: 9px;
  }

  .gain-grid.two {
    grid-template-columns: repeat(2, minmax(110px, 1fr));
  }

  .gain-grid > div {
    display: grid;
    gap: 3px;
    padding: 7px 8px;
    border: 1px solid color-mix(in srgb, var(--vscode-panel-border) 70%, transparent);
    border-radius: 3px;
  }

  .gain-grid span {
    color: var(--vscode-descriptionForeground);
    font-size: 10px;
  }

  .gain-grid strong {
    font-size: 11px;
    font-weight: 500;
  }

  .gain-input {
    height: 25px;
    font-size: 11px;
  }

  .feedback-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 14px;
    min-height: 34px;
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
  }

  .speed-feedback-path {
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid color-mix(in srgb, var(--vscode-panel-border) 70%, transparent);
  }

  .feedback-source-note {
    margin-bottom: 9px;
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
    font-weight: 600;
  }

  .feedback-path-line {
    display: grid;
    grid-template-columns: minmax(120px, auto) 28px minmax(320px, 420px) 28px minmax(110px, auto) minmax(180px, 1fr);
    align-items: center;
    gap: 6px;
  }

  .feedback-signal {
    min-width: 0;
  }

  .observer-block {
    width: auto;
  }

  .feedback-return {
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
    white-space: nowrap;
  }

  .feedback-line {
    letter-spacing: 1px;
    white-space: nowrap;
  }

  .state-note {
    padding: 2px 4px;
  }

  @media (max-width: 980px) {
    .control-sheet {
      min-width: 0;
    }

    .diagram {
      grid-template-columns: 1fr;
      justify-items: stretch;
    }

    .diagram .arrow {
      transform: rotate(90deg);
    }

    .sum-node {
      justify-self: center;
    }

    .wide-block,
    .compact-block {
      min-width: 0;
      max-width: none;
      width: auto;
    }

    .feedback-row {
      padding-left: 0;
      flex-wrap: wrap;
    }

    .feedback-path-line {
      grid-template-columns: 1fr;
    }

    .feedback-path-line .arrow {
      transform: rotate(90deg);
      text-align: center;
    }

    .feedback-return {
      white-space: normal;
    }
  }
</style>
