<script lang="ts">
  import { untrack } from "svelte";
  import type { ConnectionInfo } from "../connection/types";
  import { listParameters, readParameters, writeParameter } from "../parameters/api";
  import type { ParameterMetadata, ParameterValue } from "../parameters/types";

  type Props = {
    connection: ConnectionInfo | undefined;
    onError?: (error: unknown) => void;
  };

  const CURRENT_USER_SYMBOL = "PARAM_LIMIT_I_MAX";
  const SPEED_USER_SYMBOL = "PARAM_LIMIT_WM_MAX";
  const CURRENT_HARDWARE_SYMBOL = "PARAM_LIMIT_I_HARDWARE";
  const SPEED_HARDWARE_SYMBOL = "PARAM_LIMIT_WM_HARDWARE";

  const OPERATING_LIMITS = [
    { label: "Current limit", userSymbol: CURRENT_USER_SYMBOL, hardwareSymbol: CURRENT_HARDWARE_SYMBOL },
    { label: "Maximum speed", userSymbol: SPEED_USER_SYMBOL, hardwareSymbol: SPEED_HARDWARE_SYMBOL },
  ];

  const ALL_SYMBOLS = [
    CURRENT_USER_SYMBOL,
    SPEED_USER_SYMBOL,
    CURRENT_HARDWARE_SYMBOL,
    SPEED_HARDWARE_SYMBOL,
  ];

  let { connection, onError = () => undefined }: Props = $props();

  let metadata = $state<Record<string, ParameterMetadata>>({});
  let values = $state<Record<string, ParameterValue | null>>({});
  let drafts = $state<Record<string, string>>({});
  let loading = $state(false);
  let writing = $state<Set<string>>(new Set());
  let generation = 0;

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

    untrack(() => void loadLimits(activeConnection, token));
  });

  function valueText(value: ParameterValue | null | undefined): string {
    if (!value) return "";
    if (value.type === "position") return `${value.value.turns}, ${value.value.theta}`;
    if (value.type === "f32") return Number(value.value).toPrecision(7).replace(/(?:\.0+|(\.\d+?)0+)$/, "$1");
    return String(value.value);
  }

  function displayText(symbol: string): string {
    const value = values[symbol];
    return value ? valueText(value) : "—";
  }

  function unitFor(symbol: string): string {
    return metadata[symbol]?.unit ?? "";
  }

  function isWritable(symbol: string): boolean {
    return metadata[symbol]?.access.toLowerCase().includes("w") ?? false;
  }

  function isConfigured(symbol: string): boolean {
    const value = values[symbol];
    if (!value || value.type === "position") return false;
    return Number.isFinite(Number(value.value));
  }

  function numericValue(symbol: string): number | null {
    const value = values[symbol];
    if (!value || value.type === "position") return null;
    const number = Number(value.value);
    return Number.isFinite(number) ? number : null;
  }

  function activeSource(userSymbol: string, hardwareSymbol: string): "user" | "hardware" | null {
    const user = numericValue(userSymbol);
    const hardware = numericValue(hardwareSymbol);
    if (user === null || hardware === null) return null;
    return user <= hardware ? "user" : "hardware";
  }

  function parameterValue(meta: ParameterMetadata, text: string): ParameterValue {
    const parsed = Number(text.trim());
    if (!Number.isFinite(parsed)) throw new Error(`${meta.symbol}: value must be finite.`);

    if (meta.typeName === "f32") return { type: "f32", value: parsed };
    if (meta.typeName === "u8") {
      if (!Number.isInteger(parsed) || parsed < 0 || parsed > 255) throw new Error(`${meta.symbol}: expected u8.`);
      return { type: "u8", value: parsed };
    }
    if (meta.typeName === "i8") {
      if (!Number.isInteger(parsed) || parsed < -128 || parsed > 127) throw new Error(`${meta.symbol}: expected i8.`);
      return { type: "i8", value: parsed };
    }
    if (meta.typeName === "i32") {
      if (!Number.isInteger(parsed)) throw new Error(`${meta.symbol}: expected i32.`);
      return { type: "i32", value: parsed };
    }
    if (meta.typeName === "u32") {
      if (!Number.isInteger(parsed) || parsed < 0) throw new Error(`${meta.symbol}: expected u32.`);
      return { type: "u32", value: parsed };
    }

    throw new Error(`${meta.symbol}: Limits page does not edit ${meta.typeName}.`);
  }

  async function loadLimits(activeConnection: ConnectionInfo, token: number) {
    loading = true;
    try {
      const registry = await listParameters();
      if (token !== generation || connection !== activeConnection) return;

      const wanted = new Set(ALL_SYMBOLS);
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

  function handleKeydown(event: KeyboardEvent, symbol: string) {
    if (event.key === "Enter") {
      event.preventDefault();
      (event.currentTarget as HTMLInputElement).blur();
    } else if (event.key === "Escape") {
      drafts = { ...drafts, [symbol]: valueText(values[symbol]) };
      (event.currentTarget as HTMLInputElement).blur();
    }
  }
</script>

<div class="limits-root">
  <section class="page-toolbar">
    <div class="page-title">LIMITS / SAFETY</div>
    <div class="toolbar-actions">
      <button class="tool-button" disabled={!connection || loading} onclick={() => connection && void loadLimits(connection, ++generation)} title="Refresh limits">
        <i class={`codicon ${loading ? "codicon-loading codicon-modifier-spin" : "codicon-refresh"}`}></i>
        Refresh
      </button>
    </div>
  </section>

  <section class="limits-content">
    {#if !connection}
      <div class="empty-state"><i class="codicon codicon-plug"></i><div>Connect a device to configure limits.</div></div>
    {:else}
      <div class="limits-sheet">
        <section class="limits-section">
          <div class="section-title">Operating Limits</div>
          <div class="operating-grid" role="table" aria-label="Operating limits">
            <div class="operating-header" role="row">
              <div role="columnheader">Parameter</div>
              <div role="columnheader">User Limit</div>
              <div role="columnheader">Hardware Limit</div>
            </div>

            {#each OPERATING_LIMITS as row}
              {@const source = activeSource(row.userSymbol, row.hardwareSymbol)}
              <div class="operating-row" role="row">
                <div class="parameter-name" role="cell">{row.label}</div>
                <div class:configured-cell={isConfigured(row.userSymbol)} class:active-limit-cell={source === "user"} class="limit-cell" role="cell">
                  {#if metadata[row.userSymbol]}
                    <span class="inline-editor">
                      <input
                        class="compact-input mono"
                        value={drafts[row.userSymbol] ?? ""}
                        disabled={!isWritable(row.userSymbol) || writing.has(row.userSymbol)}
                        oninput={(event) => drafts = { ...drafts, [row.userSymbol]: (event.currentTarget as HTMLInputElement).value }}
                        onblur={() => void commit(row.userSymbol)}
                        onkeydown={(event) => handleKeydown(event, row.userSymbol)}
                      />
                      <span class="unit">{unitFor(row.userSymbol)}</span>
                    </span>
                  {:else}
                    <span class="muted">—</span>
                  {/if}
                </div>
                <div class:active-limit-cell={source === "hardware"} class="limit-cell hardware-value mono" role="cell">
                  {#if metadata[row.hardwareSymbol]}
                    <span>{displayText(row.hardwareSymbol)}</span>
                    <span class="unit">{unitFor(row.hardwareSymbol)}</span>
                  {:else}
                    <span class="muted">—</span>
                  {/if}
                </div>
              </div>
            {/each}
          </div>
        </section>

        <section class="limits-section position-section unavailable-section" aria-disabled="true">
          <div class="section-heading">
            <div class="section-title">Position Limits</div>
            <div class="section-note">Firmware unavailable</div>
          </div>

          <div class="position-grid" role="table" aria-label="Position limits">
            <div class="position-row" role="row">
              <div class="parameter-name" role="cell">Zero reference</div>
              <div class="muted mono" role="cell">—</div>
            </div>
            <div class="position-row" role="row">
              <div class="parameter-name" role="cell">Enable</div>
              <div role="cell"><button class="disabled-control" disabled>Off</button></div>
            </div>
            <div class="position-row" role="row">
              <div class="parameter-name" role="cell">Minimum position</div>
              <div class="position-editor" role="cell">
                <input class="compact-input mono" value="" placeholder="Turn" disabled />
                <input class="compact-input mono" value="" placeholder="Theta" disabled />
                <span class="unit">turn + rad</span>
              </div>
            </div>
            <div class="position-row" role="row">
              <div class="parameter-name" role="cell">Maximum position</div>
              <div class="position-editor" role="cell">
                <input class="compact-input mono" value="" placeholder="Turn" disabled />
                <input class="compact-input mono" value="" placeholder="Theta" disabled />
                <span class="unit">turn + rad</span>
              </div>
            </div>
          </div>
        </section>
      </div>
    {/if}
  </section>
</div>

<style>
  .limits-root {
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-rows: auto 1fr;
  }

  .limits-content {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: 22px 28px 36px;
  }

  .limits-sheet {
    max-width: 980px;
  }

  .limits-section + .limits-section {
    margin-top: 30px;
  }

  .section-heading {
    display: flex;
    align-items: baseline;
    gap: 10px;
    margin-bottom: 12px;
  }

  .section-title {
    margin-bottom: 12px;
    font-size: 13px;
    font-weight: 600;
    color: var(--vscode-foreground);
  }

  .section-heading .section-title {
    margin-bottom: 0;
  }

  .section-note {
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
  }

  .operating-grid,
  .position-grid {
    min-width: 650px;
  }

  .operating-header,
  .operating-row {
    display: grid;
    grid-template-columns: minmax(180px, 0.9fr) minmax(260px, 1.25fr) minmax(220px, 1fr);
    column-gap: 18px;
    align-items: stretch;
  }

  .operating-header {
    min-height: 34px;
    align-items: center;
    border-bottom: 1px solid var(--vscode-panel-border);
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
    font-weight: 600;
  }

  .operating-row,
  .position-row {
    min-height: 46px;
    border-bottom: 1px solid color-mix(in srgb, var(--vscode-panel-border) 55%, transparent);
    font-size: 12px;
  }

  .position-row {
    display: grid;
    grid-template-columns: minmax(180px, 0.9fr) minmax(500px, 2.25fr);
    column-gap: 18px;
    align-items: center;
  }

  .parameter-name {
    align-self: center;
    font-weight: 600;
  }

  .limit-cell {
    min-width: 0;
    display: flex;
    align-items: center;
    padding: 5px 8px;
    margin: 3px -8px;
    border-radius: 3px;
  }

  .configured-cell {
    background: color-mix(in srgb, var(--vscode-foreground) 6%, transparent);
  }

  .active-limit-cell {
    background: color-mix(in srgb, var(--vscode-focusBorder) 18%, transparent);
  }

  .configured-cell.active-limit-cell {
    background: color-mix(in srgb, var(--vscode-focusBorder) 18%, var(--vscode-foreground) 5%);
  }

  .hardware-value {
    gap: 7px;
    white-space: nowrap;
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

  .position-section {
    max-width: 760px;
  }

  .unavailable-section {
    opacity: 0.5;
  }

  .position-editor {
    display: grid;
    grid-template-columns: 105px 105px auto;
    align-items: center;
    gap: 7px;
  }

  .disabled-control {
    min-width: 54px;
    height: 26px;
    border: 1px solid var(--vscode-panel-border);
    border-radius: 3px;
    background: var(--vscode-input-background);
    color: var(--vscode-disabledForeground, var(--vscode-descriptionForeground));
    font: inherit;
  }

  .unit {
    color: var(--vscode-descriptionForeground);
    white-space: nowrap;
  }
</style>
