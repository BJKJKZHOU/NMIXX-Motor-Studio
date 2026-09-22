<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { ConnectionInfo } from "../connection/types";
  import { selectParameters } from "../parameters/state";
  import { createParameterEditor } from "../parameters/editor";
  import { modifiedParameterIds } from "../parameters/persistence";
  import { listActions, onActionCompleted, onMotorStopIssued, startAction, startImmediateAction } from "../actions/api";
  import type { ActionCompletion, ActionHandle, ActionMetadata } from "../actions/types";
  import { startPhaseSearch as startPhaseSearchAction } from "./api";

  type Props = { connection: ConnectionInfo | undefined; onError?: (error: unknown) => void };
  type PhaseState = "idle" | "running" | "success" | "stopped" | "failed";
  type HomingState = PhaseState;
  type EnumOption = { value: number; symbol: string; label: string };

  const PHASE_CURRENT_SYMBOL = "PARAM_PHASE_I_SEARCH";
  const MOTOR_MODE_SYMBOL = "PARAM_MOTOR_MODE";
  const MOTOR_DIR_SYMBOL = "PARAM_MOTOR_DIR";
  const ENCODER_PROTOCOL_SYMBOL = "PARAM_ENCODER_PROTOCOL";
  const ENCODER_SPI_TYPE_SYMBOL = "PARAM_ENCODER_SPI_TYPE";
  const ABZ_PPR_SYMBOL = "PARAM_ENCODER_ABZ_PPR";
  const ZERO_VALID_SYMBOL = "PARAM_POSITION_ZERO_VALID";
  const PHASE_SEARCH_ACTION = "ACTION_PHASE_SEARCH_START";
  const SET_ZERO_ACTION = "ACTION_POSITION_SET_ZERO";
  const HOMING_ACTION = "ACTION_HOME_START";
  const ALL_PARAMETER_SYMBOLS = [PHASE_CURRENT_SYMBOL, MOTOR_MODE_SYMBOL, MOTOR_DIR_SYMBOL,
    ENCODER_PROTOCOL_SYMBOL, ENCODER_SPI_TYPE_SYMBOL, ABZ_PPR_SYMBOL, ZERO_VALID_SYMBOL];

  let { connection, onError = () => undefined }: Props = $props();
  const parameters = selectParameters(ALL_PARAMETER_SYMBOLS);
  const edits = createParameterEditor(parameters);
  let metadata = $derived($parameters.metadata);
  let values = $derived($parameters.values);
  let drafts = $derived($edits.drafts);
  let writing = $derived($edits.writing);
  let encoderProtocolValue = $derived(numericValue(ENCODER_PROTOCOL_SYMBOL));
  let encoderSpiTypeValue = $derived(numericValue(ENCODER_SPI_TYPE_SYMBOL));
  let motorDirectionValue = $derived(numericValue(MOTOR_DIR_SYMBOL) === -1 ? "reversed"
    : numericValue(MOTOR_DIR_SYMBOL) === 1 ? "normal" : "");
  let actions = $state<Record<string, ActionMetadata>>({});
  let phaseState = $state<PhaseState>("idle");
  let phaseMessage = $state("");
  let pendingPhaseHandle = $state<string | null>(null);
  let homingState = $state<HomingState>("idle");
  let homingMessage = $state("");
  let pendingHomingHandle = $state<string | null>(null);
  let zeroActionBusy = $state(false);

  $effect(() => {
    const activeConnection = connection;
    let disposed = false;
    actions = {};
    phaseState = "idle"; phaseMessage = ""; pendingPhaseHandle = null;
    homingState = "idle"; homingMessage = ""; pendingHomingHandle = null;
    zeroActionBusy = false;
    if (activeConnection) untrack(() => {
      void listActions().then((registry) => {
        if (!disposed) actions = Object.fromEntries(registry.map((item) => [item.symbol, item]));
      }).catch((error) => { if (!disposed) onError(error); });
    });
    return () => { disposed = true; };
  });

  onMount(() => {
    let disposed = false;
    const listeners: Array<() => void> = [];
    const register = (promise: Promise<() => void>) => {
      void promise.then((stop) => { if (disposed) stop(); else listeners.push(stop); }).catch(onError);
    };
    register(onActionCompleted(handleActionCompleted));
    register(onMotorStopIssued(() => {
      if (phaseState === "running") { pendingPhaseHandle = null; phaseState = "stopped"; phaseMessage = ""; }
      if (homingState === "running") { pendingHomingHandle = null; homingState = "stopped"; homingMessage = ""; }
    }));
    return () => { disposed = true; listeners.forEach((stop) => stop()); };
  });

  function handleKey(handle: ActionHandle | ActionCompletion): string { return `${handle.txn}:${handle.actionId}`; }
  function numericValue(symbol: string): number | null {
    const value = values[symbol];
    if (!value || value.type === "position") return null;
    const number = Number(value.value);
    return Number.isFinite(number) ? number : null;
  }
  function unitFor(symbol: string): string { return metadata[symbol]?.unit ?? ""; }
  function parameterLabel(symbol: string, fallback?: string): string { return metadata[symbol]?.label ?? fallback ?? "Unavailable"; }
  function actionLabel(symbol: string, fallback?: string): string { return actions[symbol]?.label ?? fallback ?? "Unavailable"; }
  function isWritable(symbol: string): boolean { return !$parameters.loading && !$parameters.saving && (metadata[symbol]?.access.includes("w") ?? false); }
  function actionAvailable(symbol: string): boolean { return !!actions[symbol]; }
  function phaseSearchAvailable(): boolean {
    return !!metadata[MOTOR_MODE_SYMBOL] && actionAvailable("ACTION_MOTOR_ENABLE") && actionAvailable("ACTION_MOTOR_RUN");
  }
  function enumLabel(symbol: string): string { return symbol.replace(/^ENC_PROTOCOL_/, "").replace(/^ENC_SPI_/, "").replaceAll("_", " "); }
  function enumOptions(symbol: string): EnumOption[] {
    const meta = metadata[symbol];
    return meta?.allowedSymbols.map((enumSymbol, index) => ({ symbol: enumSymbol, value: meta.allowed[index] ?? index, label: enumLabel(enumSymbol) })) ?? [];
  }
  function enumSymbolForValue(symbol: string, value: number | null): string | null {
    return value === null ? null : enumOptions(symbol).find((option) => option.value === value)?.symbol ?? null;
  }
  function isSpiProtocol(): boolean { return enumSymbolForValue(ENCODER_PROTOCOL_SYMBOL, encoderProtocolValue) === "ENC_PROTOCOL_SPI"; }
  function isAbzProtocol(): boolean { return enumSymbolForValue(ENCODER_PROTOCOL_SYMBOL, encoderProtocolValue) === "ENC_PROTOCOL_ABZ"; }
  function zeroReferenceText(): string {
    const valid = numericValue(ZERO_VALID_SYMBOL);
    return valid === null ? "—" : valid === 1 ? "Set" : "Not set";
  }
  async function setU8(symbol: string, value: number) {
    try { await edits.select(symbol, { type: "u8", value }); } catch (error) { onError(error); }
  }
  async function setMotorDirection(direction: "normal" | "reversed") {
    try { await edits.select(MOTOR_DIR_SYMBOL, { type: "i8", value: direction === "normal" ? 1 : -1 }); }
    catch (error) { onError(error); }
  }
  function handleKeydown(event: KeyboardEvent, symbol: string) { edits.keydown(event, symbol, onError); }

  async function startPhaseSearch() {
    if (!connection || !phaseSearchAvailable() || phaseState === "running") return;
    phaseState = "running"; phaseMessage = "";
    try { pendingPhaseHandle = handleKey(await startPhaseSearchAction()); }
    catch (error) { phaseState = "failed"; phaseMessage = String(error); onError(error); }
  }
  function handleActionCompleted(completion: ActionCompletion) {
    if (phaseState === "running" && (completion.symbol === PHASE_SEARCH_ACTION || handleKey(completion) === pendingPhaseHandle)) {
      pendingPhaseHandle = null;
      phaseState = completion.ok ? "success" : "failed";
      phaseMessage = completion.ok ? "" : completion.status;
      return;
    }
    if (homingState === "running" && (completion.symbol === HOMING_ACTION || handleKey(completion) === pendingHomingHandle)) {
      pendingHomingHandle = null;
      homingState = completion.ok ? "success" : "failed";
      homingMessage = completion.ok ? "" : completion.status;
    }
  }
  async function setCurrentAsZero() {
    if (!actionAvailable(SET_ZERO_ACTION) || zeroActionBusy) return;
    zeroActionBusy = true;
    try { await startImmediateAction(SET_ZERO_ACTION); }
    catch (error) { onError(error); } finally { zeroActionBusy = false; }
  }
  async function startHoming() {
    if (!actionAvailable(HOMING_ACTION) || homingState === "running") return;
    homingState = "running"; homingMessage = "";
    try { pendingHomingHandle = handleKey(await startAction(HOMING_ACTION)); }
    catch (error) { homingState = "failed"; homingMessage = String(error); onError(error); }
  }
</script>

<div class="encoder-root">
  <section class="page-toolbar"><div class="page-title">ENCODER</div></section>
  <section class="encoder-content">
    {#if !connection}
      <div class="empty-state"><i class="codicon codicon-plug"></i><div>Connect a device to configure encoder feedback.</div></div>
    {:else}
      <div class="encoder-sheet">
        <div class="encoder-columns">
          <div class="primary-column">
            <section class="encoder-section">
              <div class="section-title">Feedback Protocol</div>
              <div class="setup-grid" aria-label="Encoder feedback protocol">
                <div class="field-label">{parameterLabel(ENCODER_PROTOCOL_SYMBOL, "Protocol")}</div>
                <div>
                  {#if metadata[ENCODER_PROTOCOL_SYMBOL]}
                    <select class:ramModified={$modifiedParameterIds.has(metadata[ENCODER_PROTOCOL_SYMBOL].id)} class="compact-select"
                      disabled={!isWritable(ENCODER_PROTOCOL_SYMBOL) || writing.has(ENCODER_PROTOCOL_SYMBOL) || phaseState === "running"}
                      value={encoderProtocolValue ?? ""} onchange={(event) => void setU8(ENCODER_PROTOCOL_SYMBOL, Number(event.currentTarget.value))}>
                      {#each enumOptions(ENCODER_PROTOCOL_SYMBOL) as option}<option value={option.value}>{option.label}</option>{/each}
                    </select>
                  {:else}<span class="muted">Firmware unavailable</span>{/if}
                </div>
                {#if isSpiProtocol()}
                  <div class="field-label">{parameterLabel(ENCODER_SPI_TYPE_SYMBOL, "SPI Encoder")}</div>
                  <div>
                    {#if metadata[ENCODER_SPI_TYPE_SYMBOL]}
                      <select class:ramModified={$modifiedParameterIds.has(metadata[ENCODER_SPI_TYPE_SYMBOL].id)} class="compact-select"
                        disabled={!isWritable(ENCODER_SPI_TYPE_SYMBOL) || writing.has(ENCODER_SPI_TYPE_SYMBOL) || phaseState === "running"}
                        value={encoderSpiTypeValue ?? ""} onchange={(event) => void setU8(ENCODER_SPI_TYPE_SYMBOL, Number(event.currentTarget.value))}>
                        {#each enumOptions(ENCODER_SPI_TYPE_SYMBOL) as option}<option value={option.value}>{option.label}</option>{/each}
                      </select>
                    {:else}<span class="muted">Firmware unavailable</span>{/if}
                  </div>
                {/if}
                {#if isAbzProtocol() && metadata[ABZ_PPR_SYMBOL]}
                  <div class="field-label">{parameterLabel(ABZ_PPR_SYMBOL, "PPR")}</div>
                  <div><span class="inline-editor">
                    <input class:ramModified={$modifiedParameterIds.has(metadata[ABZ_PPR_SYMBOL].id)} class="compact-input mono"
                      value={drafts[ABZ_PPR_SYMBOL] ?? ""} disabled={!isWritable(ABZ_PPR_SYMBOL) || writing.has(ABZ_PPR_SYMBOL) || phaseState === "running"}
                      oninput={(event) => edits.edit(ABZ_PPR_SYMBOL, event.currentTarget.value)} onkeydown={(event) => handleKeydown(event, ABZ_PPR_SYMBOL)} onblur={() => edits.discard(ABZ_PPR_SYMBOL)} />
                    <span class="unit">{unitFor(ABZ_PPR_SYMBOL)}</span>
                  </span></div>
                {/if}
              </div>
            </section>
            <section class="encoder-section">
              <div class="section-title">Phase Alignment</div>
              <div class="setup-grid">
                <div class="field-label">{parameterLabel(PHASE_CURRENT_SYMBOL, "Search current")}</div>
                <div>
                  {#if metadata[PHASE_CURRENT_SYMBOL]}
                    <span class="inline-editor">
                      <input class:ramModified={$modifiedParameterIds.has(metadata[PHASE_CURRENT_SYMBOL].id)} class="compact-input mono"
                        value={drafts[PHASE_CURRENT_SYMBOL] ?? ""} disabled={!isWritable(PHASE_CURRENT_SYMBOL) || writing.has(PHASE_CURRENT_SYMBOL) || phaseState === "running"}
                        oninput={(event) => edits.edit(PHASE_CURRENT_SYMBOL, event.currentTarget.value)} onkeydown={(event) => handleKeydown(event, PHASE_CURRENT_SYMBOL)} onblur={() => edits.discard(PHASE_CURRENT_SYMBOL)} />
                      <span class="unit">{unitFor(PHASE_CURRENT_SYMBOL)}</span>
                    </span>
                  {:else}<span class="muted">—</span>{/if}
                </div>
                <div class="field-label">Phase search</div>
                <div class="action-line">
                  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
                  <vscode-button secondary disabled={!phaseSearchAvailable() || phaseState === "running"}
                    title={phaseSearchAvailable() ? "Start phase search" : "Required phase-search capabilities are not exposed by this firmware"}
                    onclick={() => void startPhaseSearch()}>{actionLabel(PHASE_SEARCH_ACTION, "Start")}</vscode-button>
                  {#if phaseState === "running"}<span class="action-status state-running"><i class="codicon codicon-loading codicon-modifier-spin"></i> Running</span>
                  {:else if phaseState === "success"}<span class="action-status state-success"><i class="codicon codicon-check"></i> Success</span>
                  {:else if phaseState === "stopped"}<span class="action-status muted"><i class="codicon codicon-debug-stop"></i> Stopped</span>
                  {:else if phaseState === "failed"}<span class="action-status state-failed" title={phaseMessage}><i class="codicon codicon-error"></i> Failed</span>{/if}
                </div>
                <div class="field-label">{parameterLabel(MOTOR_DIR_SYMBOL, "Motor direction")}</div>
                <div>
                  {#if metadata[MOTOR_DIR_SYMBOL]}
                    <select class:ramModified={$modifiedParameterIds.has(metadata[MOTOR_DIR_SYMBOL].id)} class="compact-select"
                      disabled={!isWritable(MOTOR_DIR_SYMBOL) || writing.has(MOTOR_DIR_SYMBOL) || phaseState === "running"}
                      value={motorDirectionValue} onchange={(event) => void setMotorDirection(event.currentTarget.value as "normal" | "reversed")}>
                      <option value="normal">Normal</option><option value="reversed">Reversed</option>
                    </select>
                  {:else}<span class="muted">—</span>{/if}
                </div>
              </div>
            </section>
          </div>
          <aside class="reference-column">
            <section class="encoder-section">
              <div class="section-title">Mechanical Reference</div>
              <div class="reference-row"><span class="field-label">{parameterLabel(ZERO_VALID_SYMBOL, "Zero reference")}</span><span class="readonly-value">{zeroReferenceText()}</span></div>
              <div class="reference-actions">
                <vscode-button secondary disabled={!actionAvailable(SET_ZERO_ACTION) || zeroActionBusy}
                  title={actionAvailable(SET_ZERO_ACTION) ? "Set current position as mechanical zero" : "Firmware/Application zero action is not exposed yet"}
                  onclick={() => void setCurrentAsZero()}>{actionLabel(SET_ZERO_ACTION, "Set Current as Zero")}</vscode-button>
                <vscode-button secondary disabled={!actionAvailable(HOMING_ACTION) || homingState === "running"}
                  title={actionAvailable(HOMING_ACTION) ? "Start software homing" : "Firmware/Application homing action is not exposed yet"}
                  onclick={() => void startHoming()}>{actionLabel(HOMING_ACTION, "Software Homing")}</vscode-button>
              </div>
              {#if homingState === "running"}<div class="action-status state-running"><i class="codicon codicon-loading codicon-modifier-spin"></i> Homing</div>
              {:else if homingState === "success"}<div class="action-status state-success"><i class="codicon codicon-check"></i> Homed</div>
              {:else if homingState === "stopped"}<div class="action-status muted"><i class="codicon codicon-debug-stop"></i> Homing stopped</div>
              {:else if homingState === "failed"}<div class="action-status state-failed" title={homingMessage}><i class="codicon codicon-error"></i> Homing failed</div>{/if}
              {#if !metadata[ZERO_VALID_SYMBOL] && !actionAvailable(SET_ZERO_ACTION) && !actionAvailable(HOMING_ACTION)}<div class="future-note">Firmware unavailable</div>{/if}
            </section>
          </aside>
        </div>
      </div>
    {/if}
  </section>
</div>

<style>
  .encoder-root { min-width: 0; min-height: 0; display: grid; grid-template-rows: auto 1fr; }
  .encoder-content { min-width: 0; min-height: 0; overflow: auto; padding: 22px 28px 36px; }
  .encoder-sheet { max-width: 980px; }
  .encoder-columns { display: grid; grid-template-columns: minmax(0, 3fr) minmax(260px, 2fr); gap: 44px; align-items: start; }
  .encoder-section + .encoder-section { margin-top: 30px; }
  .section-title { margin-bottom: 12px; font-size: 13px; font-weight: 600; color: var(--vscode-foreground); }
  .setup-grid { display: grid; grid-template-columns: minmax(150px, 0.8fr) minmax(220px, 1.2fr); align-items: center; column-gap: 18px; }
  .setup-grid > * { min-height: 42px; display: flex; align-items: center; border-bottom: 1px solid color-mix(in srgb, var(--vscode-panel-border) 55%, transparent); }
  .field-label { font-size: 12px; font-weight: 600; }
  .inline-editor { display: grid; grid-template-columns: minmax(84px, 132px) minmax(0, auto); align-items: center; gap: 7px; }
  .inline-editor .compact-input { min-width: 0; width: 100%; }
  .compact-select { min-width: 132px; height: 26px; border: 1px solid var(--vscode-dropdown-border, var(--vscode-input-border)); background: var(--vscode-dropdown-background, var(--vscode-input-background)); color: var(--vscode-dropdown-foreground, var(--vscode-input-foreground)); padding: 0 7px; font: inherit; font-size: 12px; outline: none; }
  .compact-select:focus { border-color: var(--vscode-focusBorder); }
  .compact-select:disabled { opacity: 0.58; }
  .unit { color: var(--vscode-descriptionForeground); white-space: nowrap; }
  .readonly-value { font-size: 12px; }
  .action-line { gap: 10px; }
  .action-status { display: inline-flex; align-items: center; gap: 5px; font-size: 12px; white-space: nowrap; }
  .state-success { color: var(--vscode-testing-iconPassed, var(--vscode-foreground)); }
  .state-failed { color: var(--vscode-errorForeground); }
  .state-running { color: var(--vscode-descriptionForeground); }
  .reference-column { min-width: 0; border-left: 1px solid var(--vscode-panel-border); padding-left: 28px; }
  .reference-row { min-height: 42px; display: flex; align-items: center; justify-content: space-between; gap: 20px; border-bottom: 1px solid color-mix(in srgb, var(--vscode-panel-border) 55%, transparent); }
  .reference-actions { margin-top: 16px; display: flex; flex-wrap: wrap; gap: 8px; }
  .future-note { margin-top: 10px; color: var(--vscode-descriptionForeground); font-size: 11px; }
  @media (max-width: 760px) { .encoder-columns { grid-template-columns: 1fr; gap: 30px; } .reference-column { border-left: 0; padding-left: 0; } }
</style>
