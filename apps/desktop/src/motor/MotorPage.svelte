<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { ConnectionInfo } from "../connection/types";
  import { listParameters, onParametersRefreshed, readCachedParameters, readCurrentParameters, readParameters, writeParameter } from "../parameters/api";
  import type { ParameterMetadata, ParameterValue } from "../parameters/types";
  import { modifiedParameterIds } from "../parameters/persistence";
  import { parameterDraftSnapshot, parameterMetadataSnapshot, parameterValueSnapshot } from "../parameters/sessionState";
  import { applyIdentification as applyIdentificationAction, listActions, onActionCompleted, startIdentification as startIdentificationAction } from "../actions/api";
  import type { ActionCompletion, ActionHandle, ActionMetadata } from "../actions/types";


  type Props = {
    connection: ConnectionInfo | undefined;
    onError?: (error: unknown) => void;
  };

  type IdentKey = "rsLs" | "flux" | "jb";
  type IdentPhase = "idle" | "running" | "ready" | "applying" | "applied" | "failed";

  type IdentState = {
    phase: IdentPhase;
    message: string;
  };

  type IdentConfig = {
    startAction: string;
    validSymbol: string;
    resultSymbols: string[];
    activeSymbols: string[];
  };

  type RowSpec = {
    activeSymbol: string;
    identifiedSymbol?: string;
    identifiedValidSymbol?: string;
    identKey?: IdentKey;
  };

  const IDENT_CONFIGS: Record<IdentKey, IdentConfig> = {
    rsLs: {
      startAction: "ACTION_IDENT_RS_LS_START",
      validSymbol: "PARAM_IDENT_RS_LS_VALID",
      resultSymbols: ["PARAM_IDENT_RS_RESULT", "PARAM_IDENT_LS_RESULT"],
      activeSymbols: ["PARAM_MOTOR_RS", "PARAM_MOTOR_LD", "PARAM_MOTOR_LQ"],
    },
    flux: {
      startAction: "ACTION_IDENT_FLUX_START",
      validSymbol: "PARAM_IDENT_FLUX_VALID",
      resultSymbols: ["PARAM_IDENT_FLUX_RESULT"],
      activeSymbols: ["PARAM_MOTOR_FLUX"],
    },
    jb: {
      startAction: "ACTION_IDENT_JB_START",
      validSymbol: "PARAM_IDENT_JB_VALID",
      resultSymbols: ["PARAM_IDENT_J_RESULT", "PARAM_IDENT_B_RESULT"],
      activeSymbols: ["PARAM_MOTOR_J", "PARAM_MOTOR_B"],
    },
  };

  const ROWS: RowSpec[] = [
    { activeSymbol: "PARAM_MOTOR_PP" },
    {
      activeSymbol: "PARAM_MOTOR_RS",
      identifiedSymbol: "PARAM_IDENT_RS_RESULT",
      identifiedValidSymbol: "PARAM_IDENT_RS_LS_VALID",
      identKey: "rsLs",
    },
    {
      activeSymbol: "PARAM_MOTOR_LD",
      identifiedSymbol: "PARAM_IDENT_LS_RESULT",
      identifiedValidSymbol: "PARAM_IDENT_RS_LS_VALID",
    },
    {
      activeSymbol: "PARAM_MOTOR_LQ",
      identifiedSymbol: "PARAM_IDENT_LS_RESULT",
      identifiedValidSymbol: "PARAM_IDENT_RS_LS_VALID",
    },
    {
      activeSymbol: "PARAM_MOTOR_FLUX",
      identifiedSymbol: "PARAM_IDENT_FLUX_RESULT",
      identifiedValidSymbol: "PARAM_IDENT_FLUX_VALID",
      identKey: "flux",
    },
    {
      activeSymbol: "PARAM_MOTOR_J",
      identifiedSymbol: "PARAM_IDENT_J_RESULT",
      identifiedValidSymbol: "PARAM_IDENT_JB_VALID",
      identKey: "jb",
    },
    {
      activeSymbol: "PARAM_MOTOR_B",
      identifiedSymbol: "PARAM_IDENT_B_RESULT",
      identifiedValidSymbol: "PARAM_IDENT_JB_VALID",
    },
  ];

  const IDENTIFICATION_SETTING_SYMBOLS = [
    "PARAM_IDENT_IF_CURRENT",
    "PARAM_IDENT_JB_EXCITE_RATIO",
    "PARAM_IDENT_JB_EXCITE_HZ",
  ] as const;
  const failReasonSymbol = "PARAM_IDENT_FAIL_REASON";
  const applyActionSymbol = "ACTION_IDENT_APPLY";

  const ALL_PARAMETER_SYMBOLS = [
    ...ROWS.flatMap((row) =>
      [row.activeSymbol, row.identifiedSymbol, row.identifiedValidSymbol].filter(
        (symbol): symbol is string => !!symbol,
      ),
    ),
    ...IDENTIFICATION_SETTING_SYMBOLS,
    failReasonSymbol,
  ];

  let { connection, onError = () => undefined }: Props = $props();

  let metadata = $state<Record<string, ParameterMetadata>>(parameterMetadataSnapshot(ALL_PARAMETER_SYMBOLS));
  let actions = $state<Record<string, ActionMetadata>>({});
  let values = $state<Record<string, ParameterValue | null>>(parameterValueSnapshot(ALL_PARAMETER_SYMBOLS));
  let drafts = $state<Record<string, string>>(parameterDraftSnapshot(ALL_PARAMETER_SYMBOLS));
  let loading = $state(false);
  let writing = $state<Set<string>>(new Set());
  let identStates = $state<Record<IdentKey, IdentState>>(initialIdentStates());
  let pendingHandles = $state<Record<string, { identKey: IdentKey; kind: "identify" }>>({});
  let generation = 0;

  onMount(() => {
    let disposed = false;
    let actionUnlisten: (() => void) | undefined;
    let refreshUnlisten: (() => void) | undefined;

    onActionCompleted((completion) => void handleActionCompleted(completion))
      .then((stop) => {
        if (disposed) stop();
        else actionUnlisten = stop;
      })
      .catch(onError);

    onParametersRefreshed(() => void refreshFromCache())
      .then((stop) => {
        if (disposed) stop();
        else refreshUnlisten = stop;
      })
      .catch(onError);

    return () => {
      disposed = true;
      actionUnlisten?.();
      refreshUnlisten?.();
    };
  });

  $effect(() => {
    const activeConnection = connection;
    const token = ++generation;
    identStates = initialIdentStates();
    pendingHandles = {};

    if (activeConnection) {
      metadata = parameterMetadataSnapshot(ALL_PARAMETER_SYMBOLS);
      values = parameterValueSnapshot(ALL_PARAMETER_SYMBOLS);
      drafts = parameterDraftSnapshot(ALL_PARAMETER_SYMBOLS);
    }

    if (!activeConnection) {
      metadata = {};
      actions = {};
      values = {};
      drafts = {};
      loading = false;
      return;
    }
    untrack(() => void loadMotorParameters(activeConnection, token));
  });

  function initialIdentStates(): Record<IdentKey, IdentState> {
    return {
      rsLs: { phase: "idle", message: "" },
      flux: { phase: "idle", message: "" },
      jb: { phase: "idle", message: "" },
    };
  }

  function handleKey(handle: ActionHandle | ActionCompletion): string {
    return `${handle.txn}:${handle.actionId}`;
  }

  function valueText(value: ParameterValue | null | undefined): string {
    if (!value) return "—";
    if (value.type === "position") return `${value.value.turns}, ${value.value.theta}`;
    if (value.type === "f32") return Number(value.value).toPrecision(7).replace(/(?:\.0+|(\.\d+?)0+)$/, "$1");
    return String(value.value);
  }

  function numericValue(value: ParameterValue | null | undefined): number | null {
    if (!value || value.type === "position") return null;
    return Number(value.value);
  }

  function displayValue(symbol?: string): string {
    if (!symbol) return "—";
    return valueText(values[symbol]);
  }

  function identifiedText(row: RowSpec): string {
    if (!row.identifiedSymbol) return "—";
    if (row.identifiedValidSymbol && numericValue(values[row.identifiedValidSymbol]) !== 1) return "—";
    return displayValue(row.identifiedSymbol);
  }

  function unitFor(symbol: string): string {
    return metadata[symbol]?.unit ?? "";
  }

  function parameterLabel(symbol: string): string {
    return metadata[symbol]?.label ?? "Unavailable";
  }

  function actionLabel(symbol: string): string {
    return actions[symbol]?.label ?? "Unavailable";
  }

  function isWritable(symbol: string): boolean {
    return metadata[symbol]?.access.toLowerCase().includes("w") ?? false;
  }

  function parameterValue(meta: ParameterMetadata, text: string): ParameterValue {
    const parsed = Number(text.trim());
    if (!Number.isFinite(parsed)) throw new Error(`${meta.label}: value must be finite.`);
    if (meta.typeName === "u8") {
      if (!Number.isInteger(parsed) || parsed < 0 || parsed > 255) throw new Error(`${meta.label}: expected u8.`);
      return { type: "u8", value: parsed };
    }
    if (meta.typeName === "f32") return { type: "f32", value: parsed };
    throw new Error(`${meta.label}: Motor page does not edit ${meta.typeName}.`);
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

  async function loadMotorParameters(activeConnection: ConnectionInfo, token: number) {
    loading = true;
    try {
      const [registry, actionRegistry] = await Promise.all([listParameters(), listActions()]);
      if (token !== generation || connection !== activeConnection) return;

      actions = Object.fromEntries(actionRegistry.map((item) => [item.symbol, item]));

      const wanted = new Set([
        ...ROWS.flatMap((row) => [row.activeSymbol, row.identifiedSymbol, row.identifiedValidSymbol].filter(Boolean) as string[]),
        ...IDENTIFICATION_SETTING_SYMBOLS,
        failReasonSymbol,
      ]);
      const entries = registry.filter((item) => wanted.has(item.symbol));
      metadata = Object.fromEntries(entries.map((item) => [item.symbol, item]));

      const readable = entries.filter((item) => item.access.toLowerCase().includes("r"));
      const results = await readCurrentParameters(readable.map((item) => item.id));
      if (token !== generation || connection !== activeConnection) return;
      applyValues(readable, results);
      restoreIdentificationStates();
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
      restoreIdentificationStates();
    } catch (error) {
      onError(error);
    }
  }

  async function refreshSymbols(symbols: string[]): Promise<Record<string, ParameterValue | null>> {
    const entries = symbols
      .map((symbol) => metadata[symbol])
      .filter((item): item is ParameterMetadata => !!item && item.access.toLowerCase().includes("r"));
    if (entries.length === 0) return {};

    const results = await readParameters(entries.map((item) => item.id));
    const byId = new Map(results.map((item) => [item.id, item]));
    const patch: Record<string, ParameterValue | null> = {};
    const draftPatch: Record<string, string> = {};

    for (const item of entries) {
      const result = byId.get(item.id);
      patch[item.symbol] = result?.value ?? null;
      if (result?.value) draftPatch[item.symbol] = valueText(result.value);
    }

    values = { ...values, ...patch };
    drafts = { ...drafts, ...draftPatch };
    return patch;
  }

  async function commit(symbol: string) {
    const meta = metadata[symbol];
    if (!meta || !isWritable(symbol) || writing.has(symbol)) return;
    writing = new Set(writing).add(symbol);
    try {
      const value = parameterValue(meta, drafts[symbol] ?? "");
      await writeParameter(meta.id, value);
      values = { ...values, [symbol]: value };
      drafts = { ...drafts, [symbol]: valueText(value) };

      for (const identKey of Object.keys(IDENT_CONFIGS) as IdentKey[]) {
        if (IDENT_CONFIGS[identKey].activeSymbols.includes(symbol) && identStates[identKey].phase === "applied") {
          setIdentState(identKey, "idle");
        }
      }
    } catch (error) {
      onError(error);
      drafts = { ...drafts, [symbol]: valueText(values[symbol]) };
    } finally {
      const next = new Set(writing);
      next.delete(symbol);
      writing = next;
    }
  }

  function handleKeydown(event: KeyboardEvent, symbol: string) {
    if (event.key === "Enter") {
      event.preventDefault();
      void commit(symbol);
      (event.currentTarget as HTMLInputElement).blur();
    } else if (event.key === "Escape") {
      drafts = { ...drafts, [symbol]: valueText(values[symbol]) };
      (event.currentTarget as HTMLInputElement).blur();
    }
  }

  function setIdentState(identKey: IdentKey, phase: IdentPhase, message = "") {
    identStates = { ...identStates, [identKey]: { phase, message } };
  }

  function closeEnough(a: number | null, b: number | null): boolean {
    if (a === null || b === null) return false;
    const scale = Math.max(1, Math.abs(a), Math.abs(b));
    return Math.abs(a - b) <= 1e-6 * scale;
  }

  function resultApplied(identKey: IdentKey): boolean {
    if (identKey === "rsLs") {
      const rs = numericValue(values.PARAM_IDENT_RS_RESULT);
      const ls = numericValue(values.PARAM_IDENT_LS_RESULT);
      return closeEnough(numericValue(values.PARAM_MOTOR_RS), rs)
        && closeEnough(numericValue(values.PARAM_MOTOR_LD), ls)
        && closeEnough(numericValue(values.PARAM_MOTOR_LQ), ls);
    }
    if (identKey === "flux") {
      return closeEnough(numericValue(values.PARAM_MOTOR_FLUX), numericValue(values.PARAM_IDENT_FLUX_RESULT));
    }
    return closeEnough(numericValue(values.PARAM_MOTOR_J), numericValue(values.PARAM_IDENT_J_RESULT))
      && closeEnough(numericValue(values.PARAM_MOTOR_B), numericValue(values.PARAM_IDENT_B_RESULT));
  }

  function restoreIdentificationStates() {
    const next = { ...identStates };
    for (const identKey of Object.keys(IDENT_CONFIGS) as IdentKey[]) {
      const current = next[identKey];
      if (current.phase === "running" || current.phase === "applying" || current.phase === "failed") continue;
      const valid = numericValue(values[IDENT_CONFIGS[identKey].validSymbol]) === 1;
      next[identKey] = valid
        ? { phase: resultApplied(identKey) ? "applied" : "ready", message: "" }
        : { phase: "idle", message: "" };
    }
    identStates = next;
  }

  function actionAvailable(symbol: string): boolean {
    return !!actions[symbol];
  }

  function identifyBusy(): boolean {
    return (Object.values(identStates) as IdentState[]).some((state) => state.phase === "running" || state.phase === "applying");
  }

  async function startIdentification(identKey: IdentKey) {
    const config = IDENT_CONFIGS[identKey];
    if (!actionAvailable(config.startAction) || identifyBusy()) return;

    try {
      for (const otherKey of Object.keys(IDENT_CONFIGS) as IdentKey[]) {
        if (otherKey !== identKey && identStates[otherKey].phase === "ready") setIdentState(otherKey, "idle");
      }

      let result = await startIdentificationAction(identKey, false);
      if (result.status === "blocked") {
        setIdentState(
          identKey,
          "failed",
          result.issues.map((issue) => issue.reason).join("\n"),
        );
        return;
      }

      if (result.status === "requires_enable") {
        const confirmed = window.confirm(
          `${actionLabel(config.startAction)} identification requires enabling the motor.\n\nEnable motor and continue?`,
        );
        if (!confirmed) return;
        result = await startIdentificationAction(identKey, true);
        if (result.status === "blocked") {
          setIdentState(
            identKey,
            "failed",
            result.issues.map((issue) => issue.reason).join("\n"),
          );
          return;
        }
      }

      if (result.status !== "started") return;
      setIdentState(identKey, "running");
      pendingHandles = {
        ...pendingHandles,
        [handleKey(result.handle)]: { identKey, kind: "identify" },
      };
    } catch (error) {
      setIdentState(identKey, "failed", error instanceof Error ? error.message : String(error));
      onError(error);
    }
  }

  async function applyIdentification(identKey: IdentKey) {
    if (!actionAvailable(applyActionSymbol) || identifyBusy() || identStates[identKey].phase !== "ready") return;

    setIdentState(identKey, "applying");
    try {
      await applyIdentificationAction();
      await refreshSymbols(IDENT_CONFIGS[identKey].activeSymbols);
      setIdentState(identKey, "applied");
    } catch (error) {
      setIdentState(identKey, "failed", error instanceof Error ? error.message : String(error));
      onError(error);
    }
  }

  async function handleActionCompleted(completion: ActionCompletion) {
    let pending = pendingHandles[handleKey(completion)];

    if (!pending) {
      const runningKey = (Object.keys(IDENT_CONFIGS) as IdentKey[]).find(
        (key) => identStates[key].phase === "running" && IDENT_CONFIGS[key].startAction === completion.symbol,
      );
      if (runningKey) pending = { identKey: runningKey, kind: "identify" };

    }

    if (!pending) return;

    const nextPending = { ...pendingHandles };
    delete nextPending[handleKey(completion)];
    pendingHandles = nextPending;

    if (!completion.ok) {
      setIdentState(pending.identKey, "failed", completion.status);
      return;
    }

    const config = IDENT_CONFIGS[pending.identKey];
    try {
      const patch = await refreshSymbols([...config.resultSymbols, config.validSymbol, failReasonSymbol]);
      if (numericValue(patch[config.validSymbol]) === 1) {
        setIdentState(pending.identKey, "ready");
      } else {
        const reason = numericValue(patch[failReasonSymbol]);
        setIdentState(pending.identKey, "failed", reason && reason !== 0 ? `Reason ${reason}` : "No valid result");
      }
    } catch (error) {
      setIdentState(pending.identKey, "failed", error instanceof Error ? error.message : String(error));
      onError(error);
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
                        oninput={(event) => drafts = { ...drafts, [row.activeSymbol]: (event.currentTarget as HTMLInputElement).value }}
                        onkeydown={(event) => handleKeydown(event, row.activeSymbol)}
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
                        oninput={(event) => drafts = { ...drafts, [settingSymbol]: (event.currentTarget as HTMLInputElement).value }}
                        onkeydown={(event) => handleKeydown(event, settingSymbol)}
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
