<script lang="ts">
  import { untrack } from "svelte";
  import type { ConnectionInfo } from "../connection/types";
  import { listParameters, readParameters, writeParameter } from "../parameters/api";
  import type { ParameterMetadata, ParameterValue } from "../parameters/types";

  type Props = {
    connection: ConnectionInfo | undefined;
    onError?: (error: unknown) => void;
  };

  type RowSpec = {
    label: string;
    activeSymbol: string;
    identifiedSymbol?: string;
    actionLabel?: string;
  };

  const ROWS: RowSpec[] = [
    { label: "Pole pairs", activeSymbol: "PARAM_MOTOR_PP" },
    { label: "Rs", activeSymbol: "PARAM_MOTOR_RS", identifiedSymbol: "PARAM_IDENT_RS_RESULT", actionLabel: "Rs/Ls" },
    { label: "Ld", activeSymbol: "PARAM_MOTOR_LD", identifiedSymbol: "PARAM_IDENT_LS_RESULT" },
    { label: "Lq", activeSymbol: "PARAM_MOTOR_LQ", identifiedSymbol: "PARAM_IDENT_LS_RESULT" },
    { label: "Flux", activeSymbol: "PARAM_MOTOR_FLUX", identifiedSymbol: "PARAM_IDENT_FLUX_RESULT", actionLabel: "Flux" },
    { label: "J", activeSymbol: "PARAM_MOTOR_J", actionLabel: "J/B" },
    { label: "B", activeSymbol: "PARAM_MOTOR_B" },
  ];

  let { connection, onError = () => undefined }: Props = $props();

  let metadata = $state<Record<string, ParameterMetadata>>({});
  let values = $state<Record<string, ParameterValue | null>>({});
  let drafts = $state<Record<string, string>>({});
  let loading = $state(false);
  let writing = $state<Set<string>>(new Set());
  let generation = 0;

  const identificationCurrentSymbol = "PARAM_IDENT_IF_CURRENT";

  $effect(() => {
    const activeConnection = connection;
    const token = ++generation;
    if (!activeConnection) {
      metadata = {};
      values = {};
      drafts = {};
      loading = false;
      return;
    }
    untrack(() => void loadMotorParameters(activeConnection, token));
  });

  function valueText(value: ParameterValue | null | undefined): string {
    if (!value) return "—";
    if (value.type === "position") return `${value.value.turns}, ${value.value.theta}`;
    if (value.type === "f32") return Number(value.value).toPrecision(7).replace(/(?:\.0+|(\.\d+?)0+)$/, "$1");
    return String(value.value);
  }

  function displayValue(symbol?: string): string {
    if (!symbol) return "—";
    return valueText(values[symbol]);
  }

  function unitFor(symbol: string): string {
    return metadata[symbol]?.unit ?? "";
  }

  function isWritable(symbol: string): boolean {
    return metadata[symbol]?.access.toLowerCase().includes("w") ?? false;
  }

  function parameterValue(meta: ParameterMetadata, text: string): ParameterValue {
    const parsed = Number(text.trim());
    if (!Number.isFinite(parsed)) throw new Error(`${meta.symbol}: value must be finite.`);
    if (meta.typeName === "u8") {
      if (!Number.isInteger(parsed) || parsed < 0 || parsed > 255) throw new Error(`${meta.symbol}: expected u8.`);
      return { type: "u8", value: parsed };
    }
    if (meta.typeName === "f32") return { type: "f32", value: parsed };
    throw new Error(`${meta.symbol}: Motor page does not edit ${meta.typeName}.`);
  }

  async function loadMotorParameters(activeConnection: ConnectionInfo, token: number) {
    loading = true;
    try {
      const registry = await listParameters();
      if (token !== generation || connection !== activeConnection) return;

      const wanted = new Set([
        ...ROWS.flatMap((row) => [row.activeSymbol, row.identifiedSymbol].filter(Boolean) as string[]),
        identificationCurrentSymbol,
      ]);
      const entries = registry.filter((item) => wanted.has(item.symbol));
      metadata = Object.fromEntries(entries.map((item) => [item.symbol, item]));

      const results = await readParameters(entries.filter((item) => item.access.toLowerCase().includes("r")).map((item) => item.id));
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

  async function commit(symbol: string) {
    const meta = metadata[symbol];
    if (!meta || !isWritable(symbol) || writing.has(symbol)) return;
    writing = new Set(writing).add(symbol);
    try {
      const value = parameterValue(meta, drafts[symbol] ?? "");
      await writeParameter(meta.id, value);
      values = { ...values, [symbol]: value };
      drafts = { ...drafts, [symbol]: valueText(value) };
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
      (event.currentTarget as HTMLInputElement).blur();
      void commit(symbol);
    }
    if (event.key === "Escape") {
      drafts = { ...drafts, [symbol]: valueText(values[symbol]) };
      (event.currentTarget as HTMLInputElement).blur();
    }
  }
</script>

<div class="motor-root">
  <section class="page-toolbar">
    <div class="page-title">MOTOR</div>
    <div class="toolbar-actions">
      <button class="tool-button" disabled={!connection || loading} onclick={() => connection && void loadMotorParameters(connection, ++generation)} title="Refresh motor parameters">
        <i class={`codicon ${loading ? "codicon-loading codicon-modifier-spin" : "codicon-refresh"}`}></i>
        Refresh
      </button>
    </div>
  </section>

  <section class="motor-content">
    {#if !connection}
      <div class="empty-state"><i class="codicon codicon-plug"></i><div>Connect a device to configure motor parameters.</div></div>
    {:else}
      <div class="motor-sheet">
        <div class="sheet-heading">
          <div class="section-title">Motor Parameters</div>
          <label class="ident-current">
            <span>Identification current</span>
            {#if metadata[identificationCurrentSymbol]}
              <span class="inline-editor">
                <input
                  class="compact-input mono"
                  value={drafts[identificationCurrentSymbol] ?? ""}
                  disabled={!isWritable(identificationCurrentSymbol) || writing.has(identificationCurrentSymbol)}
                  oninput={(event) => drafts = { ...drafts, [identificationCurrentSymbol]: (event.currentTarget as HTMLInputElement).value }}
                  onblur={() => void commit(identificationCurrentSymbol)}
                  onkeydown={(event) => handleKeydown(event, identificationCurrentSymbol)}
                />
                <span class="unit">{unitFor(identificationCurrentSymbol)}</span>
              </span>
            {:else}
              <span class="muted">—</span>
            {/if}
          </label>
        </div>

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
              <div class="parameter-name" role="cell">{row.label}</div>
              <div class="default-value muted" role="cell" title="Firmware compiled defaults are not exposed by the current HostSchema">—</div>
              <div role="cell">
                {#if metadata[row.activeSymbol]}
                  <span class="inline-editor">
                    <input
                      class="compact-input mono"
                      value={drafts[row.activeSymbol] ?? ""}
                      disabled={!isWritable(row.activeSymbol) || writing.has(row.activeSymbol)}
                      oninput={(event) => drafts = { ...drafts, [row.activeSymbol]: (event.currentTarget as HTMLInputElement).value }}
                      onblur={() => void commit(row.activeSymbol)}
                      onkeydown={(event) => handleKeydown(event, row.activeSymbol)}
                    />
                    <span class="unit">{unitFor(row.activeSymbol)}</span>
                  </span>
                {:else}
                  <span class="muted">—</span>
                {/if}
              </div>
              <div class="identified-value mono" role="cell">{displayValue(row.identifiedSymbol)}</div>
              <div class="action-cell" role="cell">
                {#if row.actionLabel}
                  <vscode-button secondary disabled title="Identification actions will use the shared Application Action API once surfaced to the desktop client">{row.actionLabel}</vscode-button>
                  <span class="action-status muted">—</span>
                {:else}
                  <span class="muted">—</span>
                {/if}
              </div>
            </div>
          {/each}
        </div>

        <div class="sheet-note">
          Active values use the shared Parameter service. Default values and Identification Actions remain read-only placeholders until those Application API contracts are exposed to the desktop client.
        </div>
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

  .sheet-heading {
    display: grid;
    grid-template-columns: minmax(220px, 1fr) minmax(320px, auto);
    align-items: center;
    gap: 24px;
    margin-bottom: 18px;
  }

  .section-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--vscode-foreground);
  }

  .ident-current {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    color: var(--vscode-descriptionForeground);
    font-size: 12px;
  }

  .parameter-grid {
    min-width: 760px;
  }

  .grid-header,
  .grid-row {
    display: grid;
    grid-template-columns: minmax(120px, 0.85fr) minmax(120px, 0.8fr) minmax(205px, 1.35fr) minmax(150px, 1fr) minmax(210px, 1.25fr);
    column-gap: 18px;
    align-items: center;
  }

  .grid-header {
    min-height: 34px;
    border-bottom: 1px solid var(--vscode-panel-border);
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
    font-weight: 600;
  }

  .grid-row {
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
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .muted {
    color: var(--vscode-descriptionForeground);
  }

  .sheet-note {
    margin-top: 14px;
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
    line-height: 1.45;
  }

  @media (max-width: 860px) {
    .sheet-heading {
      grid-template-columns: 1fr;
    }

    .ident-current {
      justify-content: flex-start;
    }
  }
</style>
