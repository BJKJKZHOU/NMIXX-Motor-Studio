<script lang="ts">
  import { onMount } from "svelte";
  import { cancelTask, exportLog } from "./api";
  import { ensureDocument, isActive, loadDocument, logs, refreshTask, runSelected, scriptSettings, selectBuiltin, taskError, taskState } from "./state";
  export let connected = false;
  let busy = false;
  let error = "";
  let feedback = "";
  $: activeTask = isActive($taskState?.state);
  $: output = $logs.map((line) => `[${(line.elapsedMs / 1000).toFixed(3)}s ${line.source}] ${line.text}`).join("\n");
  async function perform(operation: () => Promise<unknown>) {
    if (busy) return;
    busy = true; error = ""; feedback = "";
    try { await operation(); } catch (failure) { error = String(failure); } finally { busy = false; }
  }
  async function saveLog() {
    if (!$scriptSettings.logPath.trim()) throw new Error("Enter a new output file path");
    await exportLog($scriptSettings.logPath);
    feedback = "Log saved. Existing files are never overwritten.";
  }
  onMount(() => {
    void ensureDocument().catch((failure) => error = String(failure));
    void refreshTask();
    // The workbench observes task progress even when this page is not mounted.
  });
</script>

<div class="automation-page">
  <section class="page-toolbar">
    <div class="page-title">AUTOMATION / APPLICATION WORKFLOWS</div>
    <div class="toolbar-actions">
      <button class="tool-button" disabled={!connected || busy || activeTask || !$scriptSettings.document} onclick={() => void perform(runSelected)}>Run script</button>
      <button class="tool-button" disabled={!activeTask || busy} onclick={() => void perform(async () => { if ($taskState) await cancelTask($taskState.taskId); await refreshTask(); })}>Cancel script</button>
    </div>
  </section>
  <div class="automation-controls">
    <label>Script path <input class="compact-input" bind:value={$scriptSettings.path} placeholder="/home/user/tests/diagnosis.py" disabled={busy || activeTask} /></label>
    <button class="tool-button" disabled={busy || activeTask || !$scriptSettings.path.trim()} onclick={() => void perform(loadDocument)}>Open file</button>
    <button class="tool-button" disabled={busy || activeTask} onclick={() => void perform(() => selectBuiltin("diagnosis"))}>Runtime diagnosis</button>
    <button class="tool-button" disabled={busy || activeTask} onclick={() => void perform(() => selectBuiltin("motion"))}>Motion example</button>
    <label>Interpreter <input class="compact-input" bind:value={$scriptSettings.program} disabled={busy || activeTask} /></label>
    <label>Timeout (s) <input class="compact-input timeout" type="number" min="1" max="3600" step="1" bind:value={$scriptSettings.timeoutSeconds} disabled={busy || activeTask} /></label>
  </div>
  <div class="automation-notice">
    Uses the same Application API and connection as GUI actions, not CLI. Scripts can write parameters and operate the motor and Scope.
    The Motion example uses your current committed target and requires manual Enable first.
    Scripts run with your OS user permissions, not in a sandbox. Run only trusted files.
  </div>
  {#if error || $taskError}<div class="error-text" role="alert">{error || $taskError}</div>{/if}
  <div class="automation-content">
    <section class="source-panel">
      <div class="section-heading">{$scriptSettings.document?.name ?? "No script selected"} · source preview</div>
      <pre aria-label="Read-only script source">{$scriptSettings.document?.source ?? ""}</pre>
    </section>
    <section class="log-panel">
      <div class="section-heading">
        {$taskState?.state ?? "IDLE"}
        {#if $taskState?.taskId} · {$taskState.name} · task {$taskState.taskId}{/if}
        {#if $taskState?.exitCode !== null && $taskState?.exitCode !== undefined} · exit {$taskState.exitCode}{/if}
      </div>
      {#if $taskState?.message}<div class="automation-message">{$taskState.message}</div>{/if}
      {#if ($taskState?.firstSequence ?? 1) > 1}<div class="automation-message">Old output was truncated by the bounded log buffer.</div>{/if}
      <pre class="output" aria-label="Script output">{output || "No output yet."}</pre>
    </section>
  </div>
  <div class="log-export">
    <input class="compact-input" bind:value={$scriptSettings.logPath} placeholder="/home/user/logs/diagnosis-001.log" aria-label="New log output file" />
    <button class="tool-button" disabled={busy || activeTask || !$taskState?.taskId} onclick={() => void perform(saveLog)}>Save log</button>
    {#if feedback}<span>{feedback}</span>{/if}
  </div>
</div>
<style>
  .automation-page { min-width: 0; min-height: 0; display: grid; grid-template-rows: auto auto auto auto minmax(0, 1fr) auto; }
  .page-toolbar { grid-row: 1; }
  .automation-controls { grid-row: 2; }
  .automation-notice { grid-row: 3; }
  .error-text { grid-row: 4; }
  .automation-content { grid-row: 5; }
  .log-export { grid-row: 6; }
  .automation-controls, .log-export { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 10px 16px; }
  .automation-controls label { display: flex; align-items: center; gap: 6px; font-size: 11px; }
  .automation-controls label:first-child { flex: 1 1 320px; }
  .automation-controls label:first-child input, .log-export input { flex: 1; min-width: 180px; }
  .timeout { width: 70px; }
  .automation-notice, .automation-message { font-size: 11px; line-height: 1.5; padding: 8px 16px; color: var(--vscode-descriptionForeground); }
  .automation-content { min-height: 240px; min-width: 0; display: grid; grid-template-columns: minmax(260px, 0.9fr) minmax(320px, 1.1fr); overflow: hidden; }
  .source-panel, .log-panel { min-width: 0; min-height: 0; display: flex; flex-direction: column; border: 1px solid var(--vscode-panel-border); }
  .section-heading { padding: 8px 12px; font-size: 11px; border-bottom: 1px solid var(--vscode-panel-border); }
  pre { margin: 0; padding: 12px; min-height: 0; overflow: auto; flex: 1; font-size: 11px; line-height: 1.5; }
  .output { white-space: pre-wrap; overflow-wrap: anywhere; }
  .error-text { padding: 0 16px; }
  .log-export span { font-size: 11px; }
</style>
