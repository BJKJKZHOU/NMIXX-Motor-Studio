<script lang="ts">
  import type { ConnectionInfo } from "../connection/types";
  import { selectParameters } from "../parameters/state";
  import { createParameterEditor } from "../parameters/editor";
  import { parameterText } from "../parameters/codec";
  import { modifiedParameterIds } from "../parameters/persistence";

  type Props = { connection: ConnectionInfo | undefined; onError?: (error: unknown) => void };
  const CURRENT_USER_SYMBOL = "PARAM_LIMIT_I_MAX";
  const SPEED_USER_SYMBOL = "PARAM_LIMIT_WM_MAX";
  const CURRENT_HARDWARE_SYMBOL = "PARAM_LIMIT_I_HARDWARE";
  const SPEED_HARDWARE_SYMBOL = "PARAM_LIMIT_WM_HARDWARE";
  const VBUS_MIN_SYMBOL = "PARAM_LIMIT_VBUS_MIN";
  const VBUS_ACTUAL_SYMBOL = "PARAM_ADC_VBUS";
  const VBUS_MAX_SYMBOL = "PARAM_LIMIT_VBUS_MAX";
  const OPERATING_LIMITS = [
    { userSymbol: CURRENT_USER_SYMBOL, hardwareSymbol: CURRENT_HARDWARE_SYMBOL },
    { userSymbol: SPEED_USER_SYMBOL, hardwareSymbol: SPEED_HARDWARE_SYMBOL },
  ];
  const parameters = selectParameters([
    CURRENT_USER_SYMBOL, SPEED_USER_SYMBOL, CURRENT_HARDWARE_SYMBOL, SPEED_HARDWARE_SYMBOL,
    VBUS_MIN_SYMBOL, VBUS_ACTUAL_SYMBOL, VBUS_MAX_SYMBOL,
  ]);
  const edits = createParameterEditor(parameters);
  let { connection, onError = () => undefined }: Props = $props();
  let metadata = $derived($parameters.metadata);
  let values = $derived($parameters.values);
  let drafts = $derived($edits.drafts);
  let writing = $derived($edits.writing);

  function displayText(symbol: string) { return parameterText(values[symbol]) || "—"; }
  function unitFor(symbol: string) { return metadata[symbol]?.unit ?? ""; }
  function parameterLabel(symbol: string, fallback = "Unavailable") { return metadata[symbol]?.label ?? fallback; }
  function isWritable(symbol: string) { return !$parameters.loading && !$parameters.saving && !!metadata[symbol]?.access.includes("w"); }
  function numericValue(symbol: string): number | null {
    const value = values[symbol];
    if (!value || value.type === "position") return null;
    const number = Number(value.value);
    return Number.isFinite(number) ? number : null;
  }
  function isConfigured(symbol: string) { return numericValue(symbol) !== null; }
  function activeSource(userSymbol: string, hardwareSymbol: string): "user" | "hardware" | null {
    const user = numericValue(userSymbol);
    const hardware = numericValue(hardwareSymbol);
    return user === null || hardware === null ? null : user <= hardware ? "user" : "hardware";
  }
  function busState(): "under" | "normal" | "over" | null {
    const min = numericValue(VBUS_MIN_SYMBOL);
    const actual = numericValue(VBUS_ACTUAL_SYMBOL);
    const max = numericValue(VBUS_MAX_SYMBOL);
    if (min === null || actual === null || max === null) return null;
    return actual < min ? "under" : actual > max ? "over" : "normal";
  }
  function handleKeydown(event: KeyboardEvent, symbol: string) { edits.keydown(event, symbol, onError); }
</script>

<div class="limits-root">
  <section class="page-toolbar">
    <div class="page-title">LIMITS / SAFETY</div>
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
                <div class="parameter-name" role="cell">{parameterLabel(row.userSymbol)}</div>
                <div class:configured-cell={isConfigured(row.userSymbol)} class:active-limit-cell={source === "user"} class="limit-cell" role="cell">
                  {#if metadata[row.userSymbol]}
                    <span class="inline-editor">
                      <input
                        class:ramModified={$modifiedParameterIds.has(metadata[row.userSymbol].id)}
                        class="compact-input mono"
                        value={drafts[row.userSymbol] ?? ""}
                        disabled={!isWritable(row.userSymbol) || writing.has(row.userSymbol)}
                        oninput={(event) => edits.edit(row.userSymbol, event.currentTarget.value)}
                        onkeydown={(event) => handleKeydown(event, row.userSymbol)}
                        onblur={() => edits.discard(row.userSymbol)}
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

        <section class="limits-section bus-section">
          <div class="section-title">Bus Voltage</div>
          <div class="bus-grid" role="table" aria-label="Bus voltage limits">
            <div class="bus-header" role="row">
              <div role="columnheader">Minimum</div>
              <div role="columnheader">Actual</div>
              <div role="columnheader">Maximum</div>
            </div>
            <div class="bus-row" role="row">
              <div class="bus-cell" role="cell">
                {#if metadata[VBUS_MIN_SYMBOL]}
                  <span class="inline-editor">
                    <input
                      class:ramModified={$modifiedParameterIds.has(metadata[VBUS_MIN_SYMBOL].id)}
                      class="compact-input mono"
                      value={drafts[VBUS_MIN_SYMBOL] ?? ""}
                      disabled={!isWritable(VBUS_MIN_SYMBOL) || writing.has(VBUS_MIN_SYMBOL)}
                      oninput={(event) => edits.edit(VBUS_MIN_SYMBOL, event.currentTarget.value)}
                      onkeydown={(event) => handleKeydown(event, VBUS_MIN_SYMBOL)}
                      onblur={() => edits.discard(VBUS_MIN_SYMBOL)}
                    />
                    <span class="unit">{unitFor(VBUS_MIN_SYMBOL)}</span>
                  </span>
                {:else}
                  <span class="muted">—</span>
                {/if}
                <span class="bus-caption">Undervoltage limit</span>
              </div>

              <div
                class:bus-under={busState() === "under"}
                class:bus-over={busState() === "over"}
                class="bus-cell bus-actual"
                role="cell"
              >
                {#if metadata[VBUS_ACTUAL_SYMBOL]}
                  <div>
                    <span class="mono">{displayText(VBUS_ACTUAL_SYMBOL)}</span>
                    <span class="unit">{unitFor(VBUS_ACTUAL_SYMBOL)}</span>
                  </div>
                {:else}
                  <span class="muted">—</span>
                {/if}
                <span class="bus-caption">DC bus voltage</span>
              </div>

              <div class="bus-cell" role="cell">
                {#if metadata[VBUS_MAX_SYMBOL]}
                  <span class="inline-editor">
                    <input
                      class:ramModified={$modifiedParameterIds.has(metadata[VBUS_MAX_SYMBOL].id)}
                      class="compact-input mono"
                      value={drafts[VBUS_MAX_SYMBOL] ?? ""}
                      disabled={!isWritable(VBUS_MAX_SYMBOL) || writing.has(VBUS_MAX_SYMBOL)}
                      oninput={(event) => edits.edit(VBUS_MAX_SYMBOL, event.currentTarget.value)}
                      onkeydown={(event) => handleKeydown(event, VBUS_MAX_SYMBOL)}
                      onblur={() => edits.discard(VBUS_MAX_SYMBOL)}
                    />
                    <span class="unit">{unitFor(VBUS_MAX_SYMBOL)}</span>
                  </span>
                {:else}
                  <span class="muted">—</span>
                {/if}
                <span class="bus-caption">Overvoltage limit</span>
              </div>
            </div>
          </div>
        </section>

        <section class="limits-section position-section unavailable-section">
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

  .bus-section {
    max-width: 760px;
  }

  .bus-grid {
    min-width: 650px;
  }

  .bus-header,
  .bus-row {
    display: grid;
    grid-template-columns: repeat(3, minmax(180px, 1fr));
    column-gap: 18px;
  }

  .bus-header {
    min-height: 34px;
    align-items: center;
    border-bottom: 1px solid var(--vscode-panel-border);
    color: var(--vscode-descriptionForeground);
    font-size: 11px;
    font-weight: 600;
  }

  .bus-row {
    min-height: 72px;
    border-bottom: 1px solid color-mix(in srgb, var(--vscode-panel-border) 55%, transparent);
  }

  .bus-cell {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 5px;
    padding: 7px 8px;
    margin: 3px -8px;
    border-radius: 3px;
  }

  .bus-actual {
    font-size: 14px;
  }

  .bus-under,
  .bus-over {
    background: color-mix(in srgb, var(--vscode-inputValidation-errorBorder, var(--vscode-errorForeground)) 16%, transparent);
  }

  .bus-caption {
    color: var(--vscode-descriptionForeground);
    font-size: 10px;
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
