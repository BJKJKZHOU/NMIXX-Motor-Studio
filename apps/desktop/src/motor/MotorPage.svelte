<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { ConnectionInfo } from "../connection/types";
  import { selectParameters } from "../parameters/state";
  import { createParameterEditor } from "../parameters/editor";
  import { parameterText } from "../parameters/codec";
  import type { ParameterValue } from "../parameters/types";
  import { modifiedParameterIds } from "../parameters/persistence";
  import { applyIdentification as applyIdentificationAction, listActions, onActionCompleted, startIdentification as startIdentificationAction } from "../actions/api";
  import type { ActionCompletion, ActionHandle, ActionMetadata } from "../actions/types";

  type Props = { connection: ConnectionInfo | undefined; onError?: (error: unknown) => void };
  type IdentKey = "rsLs" | "flux" | "jb";
  type IdentPhase = "idle" | "running" | "ready" | "applying" | "applied" | "failed";
  type IdentState = { phase: IdentPhase; message: string };
  type IdentConfig = { startAction: string; validSymbol: string; resultSymbols: string[]; activeSymbols: string[] };
  type RowSpec = { activeSymbol: string; identifiedSymbol?: string; identifiedValidSymbol?: string; identKey?: IdentKey };

  const IDENT_CONFIGS: Record<IdentKey, IdentConfig> = {
    rsLs: {
      startAction: "ACTION_IDENT_RS_LS_START", validSymbol: "PARAM_IDENT_RS_LS_VALID",
      resultSymbols: ["PARAM_IDENT_RS_RESULT", "PARAM_IDENT_LS_RESULT"],
      activeSymbols: ["PARAM_MOTOR_RS", "PARAM_MOTOR_LD", "PARAM_MOTOR_LQ"],
    },
    flux: {
      startAction: "ACTION_IDENT_FLUX_START", validSymbol: "PARAM_IDENT_FLUX_VALID",
      resultSymbols: ["PARAM_IDENT_FLUX_RESULT"], activeSymbols: ["PARAM_MOTOR_FLUX"],
    },
    jb: {
      startAction: "ACTION_IDENT_JB_START", validSymbol: "PARAM_IDENT_JB_VALID",
      resultSymbols: ["PARAM_IDENT_J_RESULT", "PARAM_IDENT_B_RESULT"], activeSymbols: ["PARAM_MOTOR_J", "PARAM_MOTOR_B"],
    },
  };
  const ROWS: RowSpec[] = [
    { activeSymbol: "PARAM_MOTOR_PP" },
    { activeSymbol: "PARAM_MOTOR_RS", identifiedSymbol: "PARAM_IDENT_RS_RESULT", identifiedValidSymbol: "PARAM_IDENT_RS_LS_VALID", identKey: "rsLs" },
    { activeSymbol: "PARAM_MOTOR_LD", identifiedSymbol: "PARAM_IDENT_LS_RESULT", identifiedValidSymbol: "PARAM_IDENT_RS_LS_VALID" },
    { activeSymbol: "PARAM_MOTOR_LQ", identifiedSymbol: "PARAM_IDENT_LS_RESULT", identifiedValidSymbol: "PARAM_IDENT_RS_LS_VALID" },
    { activeSymbol: "PARAM_MOTOR_FLUX", identifiedSymbol: "PARAM_IDENT_FLUX_RESULT", identifiedValidSymbol: "PARAM_IDENT_FLUX_VALID", identKey: "flux" },
    { activeSymbol: "PARAM_MOTOR_J", identifiedSymbol: "PARAM_IDENT_J_RESULT", identifiedValidSymbol: "PARAM_IDENT_JB_VALID", identKey: "jb" },
    { activeSymbol: "PARAM_MOTOR_B", identifiedSymbol: "PARAM_IDENT_B_RESULT", identifiedValidSymbol: "PARAM_IDENT_JB_VALID" },
  ];
  const IDENTIFICATION_SETTING_SYMBOLS = ["PARAM_IDENT_IF_CURRENT", "PARAM_IDENT_JB_EXCITE_RATIO", "PARAM_IDENT_JB_EXCITE_HZ"] as const;
  const failReasonSymbol = "PARAM_IDENT_FAIL_REASON";
  const applyActionSymbol = "ACTION_IDENT_APPLY";
  const parameters = selectParameters([
    ...ROWS.flatMap((row) => [row.activeSymbol, row.identifiedSymbol, row.identifiedValidSymbol].filter((symbol): symbol is string => !!symbol)),
    ...IDENTIFICATION_SETTING_SYMBOLS, failReasonSymbol,
  ]);
  const edits = createParameterEditor(parameters);
  let { connection, onError = () => undefined }: Props = $props();
  let metadata = $derived($parameters.metadata);
  let values = $derived($parameters.values);
  let drafts = $derived($edits.drafts);
  let writing = $derived($edits.writing);
  let actions = $state<Record<string, ActionMetadata>>({});
  let identStates = $state<Record<IdentKey, IdentState>>(initialIdentStates());
  let pendingHandles = $state<Record<string, { identKey: IdentKey; kind: "identify" }>>({});
  let generation = 0;
  let startingIdent = $state<IdentKey | null>(null);
  let earlyCompletion: ActionCompletion | undefined;

  onMount(() => {
    let disposed = false;
    let actionUnlisten: (() => void) | undefined;
    onActionCompleted((completion) => handleActionCompleted(completion))
      .then((stop) => { if (disposed) stop(); else actionUnlisten = stop; }).catch(onError);
    return () => { disposed = true; ++generation; actionUnlisten?.(); };
  });

  $effect(() => {
    const activeConnection = connection;
    const token = ++generation;
    identStates = initialIdentStates();
    pendingHandles = {};
    startingIdent = null;
    earlyCompletion = undefined;
    actions = {};
    if (activeConnection) {
      untrack(() => void listActions().then((registry) => {
        if (token === generation && connection === activeConnection) {
          actions = Object.fromEntries(registry.map((action) => [action.symbol, action]));
        }
      }).catch(onError));
    }
  });
  $effect(() => {
    const committed = values;
    untrack(() => restoreIdentificationStates(committed));
  });

  function initialIdentStates(): Record<IdentKey, IdentState> {
    return { rsLs: { phase: "idle", message: "" }, flux: { phase: "idle", message: "" }, jb: { phase: "idle", message: "" } };
  }
  function handleKey(handle: ActionHandle | ActionCompletion) { return `${handle.txn}:${handle.actionId}`; }
  function numericValue(value: ParameterValue | null | undefined): number | null {
    return !value || value.type === "position" ? null : Number(value.value);
  }
  function identifiedText(row: RowSpec): string {
    if (!row.identifiedSymbol || (row.identifiedValidSymbol && numericValue(values[row.identifiedValidSymbol]) !== 1)) return "—";
    return parameterText(values[row.identifiedSymbol]) || "—";
  }
  function unitFor(symbol: string) { return metadata[symbol]?.unit ?? ""; }
  function parameterLabel(symbol: string) { return metadata[symbol]?.label ?? "Unavailable"; }
  function actionLabel(symbol: string) { return actions[symbol]?.label ?? "Unavailable"; }
  function isWritable(symbol: string) { return !$parameters.loading && !$parameters.saving && !!metadata[symbol]?.access.includes("w"); }
  function actionAvailable(symbol: string) { return !!actions[symbol]; }
  function setIdentState(identKey: IdentKey, phase: IdentPhase, message = "") {
    identStates = { ...identStates, [identKey]: { phase, message } };
  }
  function restoreIdentificationStates(committed: Record<string, ParameterValue | null>) {
    const next = { ...identStates };
    for (const key of Object.keys(IDENT_CONFIGS) as IdentKey[]) {
      if (["running", "applying", "applied", "failed"].includes(next[key].phase)) continue;
      next[key] = { phase: numericValue(committed[IDENT_CONFIGS[key].validSymbol]) === 1 ? "ready" : "idle", message: "" };
    }
    identStates = next;
  }
  function identifyBusy() { return startingIdent !== null || Object.values(identStates).some((state) => state.phase === "running" || state.phase === "applying"); }
  async function commit(symbol: string) {
    if (!isWritable(symbol) || writing.has(symbol)) return;
    try {
      await edits.commit(symbol);
      for (const key of Object.keys(IDENT_CONFIGS) as IdentKey[]) {
        if (IDENT_CONFIGS[key].activeSymbols.includes(symbol) && identStates[key].phase === "applied") setIdentState(key, "idle");
      }
    } catch (error) { onError(error); }
  }
  function handleKeydown(event: KeyboardEvent, symbol: string) {
    if (event.key === "Enter") {
      event.preventDefault();
      void commit(symbol);
      (event.currentTarget as HTMLInputElement).blur();
    } else if (event.key === "Escape") {
      event.preventDefault();
      edits.discard(symbol);
      (event.currentTarget as HTMLInputElement).blur();
    }
  }
  async function startIdentification(identKey: IdentKey) {
    const config = IDENT_CONFIGS[identKey];
    if (!actionAvailable(config.startAction) || identifyBusy()) return;
    const token = generation;
    startingIdent = identKey;
    earlyCompletion = undefined;
    try {
      let result = await startIdentificationAction(identKey, false);
      if (token !== generation) return;
      if (result.status === "blocked") {
        setIdentState(identKey, "failed", result.issues.map((issue) => issue.reason).join("\n")); return;
      }
      if (result.status === "requires_enable") {
        if (!window.confirm(`${actionLabel(config.startAction)} identification requires enabling the motor.\n\nEnable motor and continue?`)) return;
        result = await startIdentificationAction(identKey, true);
        if (token !== generation) return;
        if (result.status === "blocked") {
          setIdentState(identKey, "failed", result.issues.map((issue) => issue.reason).join("\n")); return;
        }
      }
      if (result.status !== "started") return;
      setIdentState(identKey, "running");
      pendingHandles = { ...pendingHandles, [handleKey(result.handle)]: { identKey, kind: "identify" } };
      if (earlyCompletion && handleKey(earlyCompletion) === handleKey(result.handle)) handleActionCompleted(earlyCompletion);
    } catch (error) {
      if (token === generation) { setIdentState(identKey, "failed", String(error)); onError(error); }
    } finally {
      if (token === generation) { startingIdent = null; earlyCompletion = undefined; }
    }
  }
  async function applyIdentification(identKey: IdentKey) {
    if (!actionAvailable(applyActionSymbol) || identifyBusy() || identStates[identKey].phase !== "ready") return;
    setIdentState(identKey, "applying");
    try { await applyIdentificationAction(); setIdentState(identKey, "applied"); }
    catch (error) { setIdentState(identKey, "failed", String(error)); onError(error); }
  }
  function handleActionCompleted(completion: ActionCompletion) {
    let pending = pendingHandles[handleKey(completion)];
    if (!pending) {
      const key = (Object.keys(IDENT_CONFIGS) as IdentKey[]).find((key) => identStates[key].phase === "running" && IDENT_CONFIGS[key].startAction === completion.symbol);
      if (key) pending = { identKey: key, kind: "identify" };
    }
    if (!pending) {
      if (startingIdent && IDENT_CONFIGS[startingIdent].startAction === completion.symbol) earlyCompletion = completion;
      return;
    }
    const nextPending = { ...pendingHandles };
    delete nextPending[handleKey(completion)];
    pendingHandles = nextPending;
    if (!completion.ok) { setIdentState(pending.identKey, "failed", completion.status); return; }
    const config = IDENT_CONFIGS[pending.identKey];
    if (numericValue(values[config.validSymbol]) === 1) setIdentState(pending.identKey, "ready");
    else {
      const reason = numericValue(values[failReasonSymbol]);
      setIdentState(pending.identKey, "failed", reason ? `Reason ${reason}` : "No valid result");
    }
  }
</script>

<div class="motor-root">
  <section class="page-toolbar">
    <div class="page-title">MOTOR</div>
  </section>

  <section class="motor-content">
    {#if !connection}
      <div class="empty-state"><i class="codicon codicon-plug"></i><div>Connect a device to configure motor parameters.</div></div>
    {:else}
      <div class="motor-sheet">
        <section class="motor-section">
          <div class="section-title">Motor Parameters</div>

          <div class="parameter-grid" role="table" aria-label="Motor parameters">
            <div class="grid-header" role="row">
              <div role="columnheader">Parameter</div>
              <div role="columnheader">Default</div>
              <div role="columnheader">Active</div>
              <div role="columnheader">Identified</div>
              <div role="columnheader">Action</div>
            </div>

            {#each ROWS as row}
              <div class="grid-row" role="row">
                <div class="parameter-name" role="cell">{parameterLabel(row.activeSymbol)}</div>
                <div class="default-value muted" role="cell" title="Firmware compiled defaults are not exposed by the current HostSchema">—</div>
                <div role="cell">
                  {#if metadata[row.activeSymbol]}
                    <span class="inline-editor">
                      <input
                        class:ramModified={$modifiedParameterIds.has(metadata[row.activeSymbol].id)}
                        class="compact-input mono"
                        value={drafts[row.activeSymbol] ?? ""}
                        disabled={!isWritable(row.activeSymbol) || writing.has(row.activeSymbol) || identifyBusy()}
                        oninput={(event) => edits.edit(row.activeSymbol, event.currentTarget.value)}
                        onkeydown={(event) => handleKeydown(event, row.activeSymbol)}
                        onblur={() => edits.discard(row.activeSymbol)}
                      />
                      <span class="unit">{unitFor(row.activeSymbol)}</span>
                    </span>
                  {:else}
                    <span class="muted">—</span>
                  {/if}
                </div>
                <div class="identified-value mono" role="cell">{identifiedText(row)}</div>
                <div class="action-cell" role="cell">
                  {#if row.identKey}
                    {@const state = identStates[row.identKey]}
                    {@const startSymbol = IDENT_CONFIGS[row.identKey].startAction}
                    {@const startLabel = actionLabel(startSymbol)}
                    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
                    <vscode-button
                      secondary
                      disabled={!actionAvailable(startSymbol) || identifyBusy()}
                      title={actionAvailable(startSymbol) ? `Start ${startLabel}` : `${startLabel} is not exposed by this firmware`}
                      onclick={() => void startIdentification(row.identKey!)}
                    >{startLabel}</vscode-button>

                    {#if state.phase === "running"}
                      <span class="action-status state-running"><i class="codicon codicon-loading codicon-modifier-spin"></i> Running</span>
                    {:else if state.phase === "ready"}
                      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
                      <vscode-button class="apply-button" disabled={!actionAvailable(applyActionSymbol)} onclick={() => void applyIdentification(row.identKey!)} title="Apply the latest valid identification result to Active parameters">{actionLabel(applyActionSymbol)}</vscode-button>
                    {:else if state.phase === "applying"}
                      <span class="action-status state-running"><i class="codicon codicon-loading codicon-modifier-spin"></i> Applying</span>
                    {:else if state.phase === "applied"}
                      <span class="action-status state-success"><i class="codicon codicon-check"></i> Applied</span>
                    {:else if state.phase === "failed"}
                      <span class="action-status state-failed" title={state.message}><i class="codicon codicon-error"></i> Failed</span>
                    {:else}
                      <span class="action-status muted">—</span>
                    {/if}
                  {:else}
                    <span class="muted">—</span>
                  {/if}
                </div>
              </div>
            {/each}
          </div>
        </section>

        <section class="motor-section identification-settings">
          <div class="section-title">Identification Settings</div>
          <div class="settings-grid" role="table" aria-label="Identification settings">
            <div class="settings-header" role="row">
              <div role="columnheader">Parameter</div>
              <div role="columnheader">Value</div>
            </div>
            {#each IDENTIFICATION_SETTING_SYMBOLS as settingSymbol}
              {#if metadata[settingSymbol]}
                <div class="settings-row" role="row">
                  <div class="parameter-name" role="cell">{parameterLabel(settingSymbol)}</div>
                  <div role="cell">
                    <span class="inline-editor">
                      <input
                        class:ramModified={$modifiedParameterIds.has(metadata[settingSymbol].id)}
                        class="compact-input mono"
                        value={drafts[settingSymbol] ?? ""}
                        disabled={!isWritable(settingSymbol) || writing.has(settingSymbol) || identifyBusy()}
                        oninput={(event) => edits.edit(settingSymbol, event.currentTarget.value)}
                        onkeydown={(event) => handleKeydown(event, settingSymbol)}
                        onblur={() => edits.discard(settingSymbol)}
                      />
                      <span class="unit">{unitFor(settingSymbol)}</span>
                    </span>
                  </div>
                </div>
              {/if}
            {/each}
          </div>
        </section>
      </div>
    {/if}
  </section>
</div>

<style>
  .motor-root {
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-rows: auto 1fr;
  }

  .motor-content {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: 22px 28px 36px;
  }

  .motor-sheet {
    max-width: 980px;
  }

  .motor-section + .motor-section {
    margin-top: 30px;
  }

  .section-title {
    margin-bottom: 12px;
    font-size: 13px;
    font-weight: 600;
    color: var(--vscode-foreground);
  }

  .parameter-grid,
  .settings-grid {
    min-width: 760px;
  }

  .grid-header,
  .grid-row {
    display: grid;
    grid-template-columns: minmax(120px, 0.85fr) minmax(120px, 0.8fr) minmax(205px, 1.35fr) minmax(150px, 1fr) minmax(250px, 1.5fr);
    column-gap: 18px;
    align-items: center;
  }

  .settings-header,
  .settings-row {
    display: grid;
    grid-template-columns: minmax(180px, 1fr) minmax(240px, 1.35fr);
    column-gap: 18px;
    align-items: center;
    max-width: 520px;
  }

  .grid-header,
  .settings-header {
    min-height: 34px;
    border-bottom: 1px solid var(--vscode-panel-border);
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
    font-weight: 600;
  }

  .grid-row,
  .settings-row {
    min-height: 44px;
    border-bottom: 1px solid color-mix(in srgb, var(--vscode-panel-border) 55%, transparent);
    font-size: 12px;
  }

  .parameter-name {
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

  .unit {
    color: var(--vscode-descriptionForeground);
    white-space: nowrap;
  }

  .identified-value,
  .default-value {
    white-space: nowrap;
  }

  .action-cell {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .action-status {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .state-running {
    color: var(--nmixx-status-runningForeground);
  }

  .state-success {
    color: var(--nmixx-status-successForeground);
  }

  .state-failed {
    color: var(--nmixx-status-errorForeground);
  }

  .apply-button {
    --vscode-button-background: var(--nmixx-action-successBackground);
    --vscode-button-hoverBackground: var(--nmixx-action-successHoverBackground);
  }

  .muted {
    color: var(--vscode-descriptionForeground);
  }
</style>
