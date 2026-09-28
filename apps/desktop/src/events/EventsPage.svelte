<script lang="ts">
  import type { ConnectionInfo } from "../connection/types";
  import { clearProtection } from "../actions/api";
  import { clearProblemHistory, problemError, problemSnapshot, recheckProblems } from "../problems/state";
  import type { ProblemRecord, ProblemSeverity } from "../problems/types";

  export let connection: ConnectionInfo | undefined;
  export let onError: (error: unknown) => void = () => undefined;
  let busy = false;

  $: active = $problemSnapshot.active;
  $: history = $problemSnapshot.history;
  $: hasClearableFault = active.some((problem) => problem.severity === "ERROR" || problem.severity === "FAULT");

  function severityLabel(severity: ProblemSeverity): string {
    return severity === "FAULT" ? "Fault" : severity === "ERROR" ? "Error" : severity === "WARNING" ? "Warning" : "Info";
  }
  function time(ms: number): string { return ms ? new Date(ms).toLocaleTimeString() : "—"; }
  function mask(problem: ProblemRecord): string { return "0x" + problem.code.toString(16).toUpperCase().padStart(8, "0"); }

  async function perform(operation: () => Promise<void>) {
    if (busy || !connection) return;
    busy = true;
    try { await operation(); } catch (error) { onError(error); }
    finally { busy = false; }
  }
  async function clearFault() {
    await clearProtection();
    await recheckProblems();
  }
</script>

<div class="events-root">
  <section class="page-toolbar">
    <div class="page-title">EVENTS / PROBLEMS</div>
    <div class="toolbar-actions">
      <button class="tool-button" disabled={!connection || busy} onclick={() => void perform(recheckProblems)}><i class="codicon codicon-refresh"></i> Recheck</button>
      <button class="tool-button" disabled={!connection || busy || !connection?.commissioning.protectionClear || !hasClearableFault}
        title={connection?.commissioning.protectionClear ? "Clear device protection faults; firmware requires DISABLED" : "Protection clear is not exposed by this firmware"}
        onclick={() => void perform(clearFault)}>Clear Fault</button>
      <button class="tool-button" disabled={!connection || busy || history.length === active.length} onclick={() => void perform(clearProblemHistory)}>Clear History</button>
    </div>
  </section>

  <section class="events-content">
    {#if !connection}
      <div class="empty-state"><i class="codicon codicon-plug"></i><div>Connect a device to inspect problems and event history.</div></div>
    {:else}
      {#if $problemError}<div class="error-text" role="alert">{$problemError}</div>{/if}
      <section class="events-section">
        <div class="section-title">Active Problems <span>{active.length}</span></div>
        {#if active.length === 0}
          <div class="events-empty">No active problems.</div>
        {:else}
          <div class="problem-list">
            {#each active as problem (problem.id)}
              <article class="problem-row"
                class:severity-warning={problem.severity === "WARNING"}
                class:severity-error={problem.severity === "ERROR"}
                class:severity-fault={problem.severity === "FAULT"}>
                <div class="problem-level"><strong>{severityLabel(problem.severity)}</strong><span>Protection</span></div>
                <div class="problem-detail"><strong>{problem.summary}</strong><p>{problem.description}</p><div class="problem-meta">Mask {mask(problem)} · Last {time(problem.lastSeenMs)} · Count {problem.occurrenceCount}</div></div>
                <div class="problem-action muted">Device protection</div>
              </article>
            {/each}
          </div>
        {/if}
      </section>

      <section class="events-section">
        <div class="section-title">History <span>{history.length}</span></div>
        {#if history.length === 0}
          <div class="events-empty">No retained problem history.</div>
        {:else}
          <div class="history-list">
            {#each history as problem (problem.id)}
              <div class="history-row">
                <span class:active={problem.active} class:resolved={!problem.active} class="history-state">{problem.active ? "Active" : "Resolved"}</span>
                <strong>{severityLabel(problem.severity)} · {problem.summary}</strong>
                <span class="mono">{mask(problem)}</span>
                <span>First {time(problem.firstSeenMs)}</span>
                <span>Last {time(problem.lastSeenMs)}</span>
                <span>×{problem.occurrenceCount}</span>
              </div>
            {/each}
          </div>
        {/if}
      </section>
    {/if}
  </section>
</div>

<style>
  .events-root { min-height: 0; display: grid; grid-template-rows: auto minmax(0, 1fr); }
  .events-content { min-height: 0; overflow: auto; padding: 18px 24px 32px; }
  .events-section { max-width: 1100px; margin-bottom: 28px; }
  .section-title { display: flex; align-items: center; gap: 8px; margin-bottom: 10px; font-size: 13px; font-weight: 600; }
  .section-title span { color: var(--vscode-descriptionForeground); font-size: 11px; font-weight: 400; }
  .events-empty { padding: 18px 12px; border-top: 1px solid var(--vscode-panel-border); color: var(--vscode-descriptionForeground); font-size: 12px; }
  .problem-list { border-top: 1px solid var(--vscode-panel-border); }
  .problem-row { min-height: 94px; display: grid; grid-template-columns: 150px minmax(0, 1fr) 150px; gap: 18px; padding: 13px 10px; border-bottom: 1px solid var(--vscode-panel-border); border-left: 3px solid var(--vscode-panel-border); }
  .severity-warning { border-left-color: var(--vscode-inputValidation-warningBorder, #cca700); }
  .severity-error, .severity-fault { border-left-color: var(--vscode-inputValidation-errorBorder, #f14c4c); }
  .problem-level { display: grid; align-content: start; gap: 4px; }
  .problem-level strong { font-size: 12px; }
  .problem-level span, .problem-meta, .problem-action { color: var(--vscode-descriptionForeground); font-size: 11px; }
  .problem-detail strong { font-size: 12px; }
  .problem-detail p { margin: 5px 0 7px; color: var(--vscode-descriptionForeground); font-size: 11px; line-height: 1.45; }
  .problem-action { align-self: start; text-align: right; }
  .history-list { border-top: 1px solid var(--vscode-panel-border); }
  .history-row { min-height: 38px; display: grid; grid-template-columns: 74px minmax(260px, 1fr) 105px 120px 120px 50px; gap: 10px; align-items: center; padding: 5px 10px; border-bottom: 1px solid color-mix(in srgb, var(--vscode-panel-border) 70%, transparent); font-size: 11px; }
  .history-state { color: var(--vscode-descriptionForeground); }
  .history-state.active { color: var(--vscode-errorForeground); }
  .mono { font-family: "SFMono-Regular", Consolas, "Liberation Mono", monospace; }
  .muted { color: var(--vscode-descriptionForeground); }
</style>
