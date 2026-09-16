<script lang="ts">
  import { untrack } from "svelte";
  import type { ConnectionInfo } from "../connection/types";
  import { listParameters, readParameters, writeParameter } from "../parameters/api";
  import type { ParameterMetadata, ParameterValue } from "../parameters/types";
  import { listActions, onActionCompleted, startAction } from "../actions/api";
  import type { ActionCompletion, ActionHandle, ActionMetadata } from "../actions/types";

  type Props = {
    connection: ConnectionInfo | undefined;
    onError?: (error: unknown) => void;
  };

  type PhaseState = "idle" | "running" | "success" | "failed";

  const PHASE_CURRENT_SYMBOL = "PARAM_PHASE_I_SEARCH";
  const MOTOR_DIR_SYMBOL = "PARAM_MOTOR_DIR";

  // Future semantic host contracts. The page enables these automatically when
  // firmware/Application API expose them; it does not emulate them locally.
  const ENCODER_INTERFACE_SYMBOL = "PARAM_ENCODER_INTERFACE";
  const ABZ_PPR_SYMBOL = "PARAM_ENCODER_ABZ_PPR";
  const ZERO_VALID_SYMBOL = "PARAM_POSITION_ZERO_VALID";
  const PHASE_SEARCH_ACTION = "ACTION_PHASE_SEARCH_START";
  const SET_ZERO_ACTION = "ACTION_POSITION_SET_ZERO";
  const HOMING_ACTION = "ACTION_HOME_START";

  const ALL_PARAMETER_SYMBOLS = [
    PHASE_CURRENT_SYMBOL,
    MOTOR_DIR_SYMBOL,
    ENCODER_INTERFACE_SYMBOL,
    ABZ_PPR_SYMBOL,
    ZERO_VALID_SYMBOL,
  ];

  let { connection, onError = () => undefined }: Props = $props();

  let metadata = $state<Record<string, ParameterMetadata>>({});
  let actions = $state<Record<string, ActionMetadata>>({});
  let values = $state<Record<string, ParameterValue | null>>({});
  let drafts = $state<Record<string, string>>({});
  let loading = $state(false);
  let writing = $state<Set<string>>(new Set());
  let phaseState = $state<PhaseState>("idle");
  let phaseMessage = $state("");
  let pendingPhaseHandle = $state<string | null>(null);
  let generation = 0;

  $effect(() => {
    const activeConnection = connection;
    const token = ++generation;
    phaseState = "idle";
    phaseMessage = "";
    pendingPhaseHandle = null;

    if (!activeConnection) {
      metadata = {};
      actions = {};
      values = {};
      drafts = {};
      loading = false;
      return;
    }

    untrack(() => void loadEncoder(activeConnection, token));
  });

  $effect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    onActionCompleted((completion) => handleActionCompleted(completion))
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

  function handleKey(handle: ActionHandle | ActionCompletion): string {
    return `${handle.txn}:${handle.actionId}`;
  }

  function valueText(value: ParameterValue | null | undefined): string {
    if (!value) return "";
    if (value.type === "position") return `${value.value.turns}, ${value.value.theta}`;
    if (value.type === "f32") return Number(value.value).toPrecision(7).replace(/(?:\.0+|(\.\d+?)0+)$/, "$1");
    return String(value.value);
  }

  function numericValue(symbol: string): number | null {
    const value = values[symbol];
    if (!value || value.type === "position") return null;
    const number = Number(value.value);
    return Number.isFinite(number) ? number : null;
  }

  function unitFor(symbol: string): string {
    return metadata[symbol]?.unit ?? "";
  }

  function isWritable(symbol: string): boolean {
    return metadata[symbol]?.access.toLowerCase().includes("w") ?? false;
  }

  function actionAvailable(symbol: string): boolean {
    return !!actions[symbol];
  }

  function zeroReferenceText(): string {
    const valid = numericValue(ZERO_VALID_SYMBOL);
    if (valid === null) return "—";
    return valid === 1 ? "Set" : "Not set";
  }

  function motorDirection(): "normal" | "reversed" {
    return numericValue(MOTOR_DIR_SYMBOL) === -1 ? "reversed" : "normal";
  }

  function parameterValue(meta: ParameterMetadata, text: string): ParameterValue {
    const parsed = Number(text.trim());
    if (!Number.isFinite(parsed)) throw new Error(`${meta.symbol}: value must be finite.`);

    if (meta.typeName === "f32") return { type: "f32", value: parsed };
    if (meta.typeName === "i8") {
      if (!Number.isInteger(parsed) || parsed < -128 || parsed > 127) throw new Error(`${meta.symbol}: expected i8.`);
      return { type: "i8", value: parsed };
    }
    if (meta.typeName === "u8") {
      if (!Number.isInteger(parsed) || parsed < 0 || parsed > 255) throw new Error(`${meta.symbol}: expected u8.`);
      return { type: "u8", value: parsed };
    }

    throw new Error(`${meta.symbol}: Encoder page does not edit ${meta.typeName}.`);
  }

  async function loadEncoder(activeConnection: ConnectionInfo, token: number) {
    loading = true;
    try {
      const [registry, actionRegistry] = await Promise.all([listParameters(), listActions()]);
      if (token !== generation || connection !== activeConnection) return;

      actions = Object.fromEntries(actionRegistry.map((item) => [item.symbol, item]));

      const wanted = new Set(ALL_PARAMETER_SYMBOLS);
      const entries = registry.filter((item) => wanted.has(item.symbol));
      metadata = Object.fromEntries(entries.map((item) => [item.symbol, item]));

      const readable = entries.filter((item) => item.access.toLowerCase().includes("r"));
      const results = await readParameters(readable.map((item) => item.id));
      if (token !== generation || connection !== activeConnection) return;

      const byId = new Map(results.map((item) => [item.id, item]));
      const nextValues: Record<string, ParameterValue | null> = {};
      const nextDrafts: Record<string, string> = {};

      for (const item of entries) {
        const result = byId.get(item.id);
        nextValues[item.symbol] = result?.value ?? null;
        if (result?.value) nextDrafts[item.symbol] = valueText(result.value);
      }

      values = nextValues;
      drafts = nextDrafts;
    } catch (error) {
      if (token === generation) onError(error);
    } finally {
      if (token === generation) loading = false;
    }
  }

  async function refreshValues() {
    const readable = Object.values(metadata).filter((item) => item.access.toLowerCase().includes("r"));
    if (readable.length === 0) return;

    const results = await readParameters(readable.map((item) => item.id));
    const byId = new Map(results.map((item) => [item.id, item]));
    const nextValues = { ...values };
    const nextDrafts = { ...drafts };

    for (const item of readable) {
      const result = byId.get(item.id);
      nextValues[item.symbol] = result?.value ?? null;
      if (result?.value) nextDrafts[item.symbol] = valueText(result.value);
    }

    values = nextValues;
    drafts = nextDrafts;
  }

  async function commit(symbol: string) {
    const meta = metadata[symbol];
    if (!meta || !isWritable(symbol) || writing.has(symbol)) return;

    writing = new Set(writing).add(symbol);
    try {
      const value = parameterValue(meta, drafts[symbol] ?? "");
      await writeParameter(meta.id, value);
      await refreshValues();
    } catch (error) {
      onError(error);
      drafts = { ...drafts, [symbol]: valueText(values[symbol]) };
    } finally {
      const next = new Set(writing);
      next.delete(symbol);
      writing = next;
    }
  }

  async function setMotorDirection(direction: "normal" | "reversed") {
    const meta = metadata[MOTOR_DIR_SYMBOL];
    if (!meta || !isWritable(MOTOR_DIR_SYMBOL) || writing.has(MOTOR_DIR_SYMBOL)) return;

    writing = new Set(writing).add(MOTOR_DIR_SYMBOL);
    try {
      await writeParameter(meta.id, { type: "i8", value: direction === "normal" ? 1 : -1 });
      await refreshValues();
    } catch (error) {
      onError(error);
    } finally {
      const next = new Set(writing);
      next.delete(MOTOR_DIR_SYMBOL);
      writing = next;
    }
  }

  function handleKeydown(event: KeyboardEvent, symbol: string) {
    if (event.key === "Enter") {
      event.preventDefault();
      (event.currentTarget as HTMLInputElement).blur();
    } else if (event.key === "Escape") {
      drafts = { ...drafts, [symbol]: valueText(values[symbol]) };
      (event.currentTarget as HTMLInputElement).blur();
    }
  }

  async function startPhaseSearch() {
    if (!actionAvailable(PHASE_SEARCH_ACTION) || phaseState === "running") return;
    phaseState = "running";
    phaseMessage = "";
    try {
      const handle = await startAction(PHASE_SEARCH_ACTION);
      pendingPhaseHandle = handleKey(handle);
    } catch (error) {
      phaseState = "failed";
      phaseMessage = error instanceof Error ? error.message : String(error);
      onError(error);
    }
  }

  function handleActionCompleted(completion: ActionCompletion) {
    if (completion.symbol !== PHASE_SEARCH_ACTION && handleKey(completion) !== pendingPhaseHandle) return;
    pendingPhaseHandle = null;
    if (completion.ok) {
      phaseState = "success";
      phaseMessage = "";
    } else {
      phaseState = "failed";
      phaseMessage = completion.status;
    }
  }
</script>

<div class="encoder-root">
  <section class="page-toolbar">
    <div class="page-title">ENCODER</div>
    <div class="toolbar-actions">
      <button class="tool-button" disabled={!connection || loading || phaseState === "running"} onclick={() => connection && void loadEncoder(connection, ++generation)} title="Refresh encoder configuration">
        <i class={`codicon ${loading ? "codicon-loading codicon-modifier-spin" : "codicon-refresh"}`}></i>
        Refresh
      </button>
    </div>
  </section>

  <section class="encoder-content">
    {#if !connection}
      <div class="empty-state"><i class="codicon codicon-plug"></i><div>Connect a device to configure encoder feedback.</div></div>
    {:else}
      <div class="encoder-sheet">
        <div class="encoder-columns">
          <div class="primary-column">
            <section class="encoder-section">
              <div class="section-title">Feedback Interface</div>
              <div class="setup-grid future-block" aria-label="Feedback interface">
                <div class="field-label">Interface</div>
                <select class="compact-select" disabled title="Firmware protocol abstraction is not exposed yet">
                  <option>—</option>
                </select>

                <div class="field-label">PPR</div>
                <span class="inline-editor">
                  <input class="compact-input mono" disabled value="" />
                </span>

                <div class="field-label">Counts/rev</div>
                <span class="readonly-value muted">—</span>
              </div>
            </section>

            <section class="encoder-section">
              <div class="section-title">Phase Alignment</div>
              <div class="setup-grid">
                <div class="field-label">Search current</div>
                <div>
                  {#if metadata[PHASE_CURRENT_SYMBOL]}
                    <span class="inline-editor">
                      <input
                        class="compact-input mono"
                        value={drafts[PHASE_CURRENT_SYMBOL] ?? ""}
                        disabled={!isWritable(PHASE_CURRENT_SYMBOL) || writing.has(PHASE_CURRENT_SYMBOL) || phaseState === "running"}
                        oninput={(event) => drafts = { ...drafts, [PHASE_CURRENT_SYMBOL]: (event.currentTarget as HTMLInputElement).value }}
                        onblur={() => void commit(PHASE_CURRENT_SYMBOL)}
                        onkeydown={(event) => handleKeydown(event, PHASE_CURRENT_SYMBOL)}
                      />
                      <span class="unit">{unitFor(PHASE_CURRENT_SYMBOL)}</span>
                    </span>
                  {:else}
                    <span class="muted">—</span>
                  {/if}
                </div>

                <div class="field-label">Phase search</div>
                <div class="action-line">
                  <vscode-button
                    secondary
                    disabled={!actionAvailable(PHASE_SEARCH_ACTION) || phaseState === "running"}
                    title={actionAvailable(PHASE_SEARCH_ACTION) ? "Start phase search" : "Semantic phase-search Application Action is not exposed yet"}
                    onclick={() => void startPhaseSearch()}
                  >Start</vscode-button>

                  {#if phaseState === "running"}
                    <span class="action-status state-running"><i class="codicon codicon-loading codicon-modifier-spin"></i> Running</span>
                  {:else if phaseState === "success"}
                    <span class="action-status state-success"><i class="codicon codicon-check"></i> Success</span>
                  {:else if phaseState === "failed"}
                    <span class="action-status state-failed" title={phaseMessage}><i class="codicon codicon-error"></i> Failed</span>
                  {:else if !actionAvailable(PHASE_SEARCH_ACTION)}
                    <span class="action-status muted">Unavailable</span>
                  {/if}
                </div>

                <div class="field-label">Motor direction</div>
                <div>
                  {#if metadata[MOTOR_DIR_SYMBOL]}
                    <select
                      class="compact-select"
                      disabled={!isWritable(MOTOR_DIR_SYMBOL) || writing.has(MOTOR_DIR_SYMBOL) || phaseState === "running"}
                      value={motorDirection()}
                      onchange={(event) => void setMotorDirection((event.currentTarget as HTMLSelectElement).value as "normal" | "reversed")}
                    >
                      <option value="normal">Normal</option>
                      <option value="reversed">Reversed</option>
                    </select>
                  {:else}
                    <span class="muted">—</span>
                  {/if}
                </div>
              </div>
            </section>
          </div>

          <aside class="reference-column">
            <section class="encoder-section">
              <div class="section-title">Mechanical Reference</div>
              <div class="reference-row">
                <span class="field-label">Zero reference</span>
                <span class="readonly-value">{zeroReferenceText()}</span>
              </div>

              <div class="reference-actions">
                <vscode-button secondary disabled={!actionAvailable(SET_ZERO_ACTION)} title={actionAvailable(SET_ZERO_ACTION) ? "Set current position as mechanical zero" : "Firmware/Application zero action is not exposed yet"}>
                  Set Current as Zero
                </vscode-button>
                <vscode-button secondary disabled={!actionAvailable(HOMING_ACTION)} title={actionAvailable(HOMING_ACTION) ? "Start software homing" : "Firmware/Application homing action is not exposed yet"}>
                  Software Homing
                </vscode-button>
              </div>

              {#if !metadata[ZERO_VALID_SYMBOL] && !actionAvailable(SET_ZERO_ACTION) && !actionAvailable(HOMING_ACTION)}
                <div class="future-note">Firmware unavailable</div>
              {/if}
            </section>
          </aside>
        </div>
      </div>
    {/if}
  </section>
</div>

<style>
  .encoder-root {
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-rows: auto 1fr;
  }

  .encoder-content {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: 22px 28px 36px;
  }

  .encoder-sheet {
    max-width: 980px;
  }

  .encoder-columns {
    display: grid;
    grid-template-columns: minmax(0, 3fr) minmax(260px, 2fr);
    gap: 44px;
    align-items: start;
  }

  .encoder-section + .encoder-section {
    margin-top: 30px;
  }

  .section-title {
    margin-bottom: 12px;
    font-size: 13px;
    font-weight: 600;
    color: var(--vscode-foreground);
  }

  .setup-grid {
    display: grid;
    grid-template-columns: minmax(150px, 0.8fr) minmax(220px, 1.2fr);
    align-items: center;
    column-gap: 18px;
  }

  .setup-grid > * {
    min-height: 42px;
    display: flex;
    align-items: center;
    border-bottom: 1px solid color-mix(in srgb, var(--vscode-panel-border) 55%, transparent);
  }

  .field-label {
    font-size: 12px;
    font-weight: 600;
  }

  .inline-editor {
    display: grid;
    grid-template-columns: minmax(84px, 132px) minmax(0, auto);
    align-items: center;
    gap: 7px;
  }

  .inline-editor .compact-input {
    min-width: 0;
    width: 100%;
  }

  .compact-select {
    min-width: 132px;
    height: 26px;
    border: 1px solid var(--vscode-dropdown-border, var(--vscode-input-border));
    background: var(--vscode-dropdown-background, var(--vscode-input-background));
    color: var(--vscode-dropdown-foreground, var(--vscode-input-foreground));
    padding: 0 7px;
    font: inherit;
    font-size: 12px;
    outline: none;
  }

  .compact-select:focus {
    border-color: var(--vscode-focusBorder);
  }

  .compact-select:disabled,
  .future-block {
    opacity: 0.58;
  }

  .unit {
    color: var(--vscode-descriptionForeground);
    white-space: nowrap;
  }

  .readonly-value {
    font-size: 12px;
  }

  .action-line {
    gap: 10px;
  }

  .action-status {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    white-space: nowrap;
  }

  .state-success {
    color: var(--vscode-testing-iconPassed, var(--vscode-foreground));
  }

  .state-failed {
    color: var(--vscode-errorForeground);
  }

  .state-running {
    color: var(--vscode-descriptionForeground);
  }

  .reference-column {
    min-width: 0;
    border-left: 1px solid var(--vscode-panel-border);
    padding-left: 28px;
  }

  .reference-row {
    min-height: 42px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    border-bottom: 1px solid color-mix(in srgb, var(--vscode-panel-border) 55%, transparent);
  }

  .reference-actions {
    margin-top: 16px;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .future-note {
    margin-top: 10px;
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
  }

  @media (max-width: 760px) {
    .encoder-columns {
      grid-template-columns: 1fr;
      gap: 30px;
    }

    .reference-column {
      border-left: 0;
      padding-left: 0;
    }
  }
</style>
