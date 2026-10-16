<script lang="ts">
  import { uiLanguage, localize, localizeFeedback, translate } from "../i18n";
  import { onMount } from "svelte";
  import { cancelTask, exportLog } from "./api";
  import {
    activeScript, activeScriptId, closeScript, editScript, isDirty, newScript,
    importScript, openExample, openLocalScript, saveCurrentScript, scriptTabs, selectScript,
  } from "./documents";
  import { isActive, logs, refreshTask, runSelected, scriptSettings, taskError, taskState } from "./state";
  import ScriptEditor from "./ScriptEditor.svelte";

  export let connected = false;
  let busy = false;
  let error = "";
  let feedback = "";
  let explorerVisible = true;
  let settingsVisible = false;
  let saveAsVisible = false;
  let logExportVisible = false;
  let openPath = "";
  let savePath = "";
  let filePicker: HTMLInputElement;
  $: activeTask = isActive($taskState?.state);
  $: output = $logs.map((line) => `[${(line.elapsedMs / 1000).toFixed(3)}s ${line.source}] ${line.text}`).join("\n");

  async function perform(operation: () => Promise<unknown>) {
    if (busy) return;
    busy = true; error = ""; feedback = "";
    try { await operation(); } catch (failure) { error = String(failure); }
    finally { busy = false; }
  }
  async function chooseFile(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    input.value = "";
    if (file.size > 262144) { error = "Script must not exceed 262144 bytes"; return; }
    try { importScript(file.name, await file.text()); saveAsVisible = false; }
    catch (failure) { error = String(failure); }
  }
  async function openFile() {
    const path = openPath.trim();
    if (!path) throw new Error("Enter the script path");
    await openLocalScript(path);
    openPath = "";
    saveAsVisible = false;
  }
  function createFile() { newScript(); saveAsVisible = false; }
  function activateTab(id: string) { selectScript(id); saveAsVisible = false; }
  async function selectExample(kind: "diagnosis" | "encoder" | "motion") {
    await openExample(kind);
    saveAsVisible = false;
  }
  function closeTab(id: string) {
    const tab = $scriptTabs.find((item) => item.id === id);
    if (!tab) return;
    if (isDirty(tab) && !window.confirm(translate("automation.workspace.discard.prompt", $uiLanguage))) return;
    closeScript(id);
  }
  function promptSaveAs() {
    if (!$activeScript) return;
    savePath = $activeScript.path ?? "";
    saveAsVisible = true;
  }
  function saveCurrent() {
    if (!$activeScript || busy) return;
    if (!$activeScript.path || $activeScript.example) { promptSaveAs(); return; }
    void perform(() => saveCurrentScript());
  }
  async function saveAs() {
    if (!savePath.trim()) throw Error("Enter a script file path");
    await saveCurrentScript(savePath.trim());
    saveAsVisible = false;
  }
  function runCurrent() {
    if (!connected || activeTask || busy || !$activeScript) return;
    // Capture the exact editor source now. Editing after Run affects the next run,
    // never the active Application task.
    const source = $activeScript.source;
    const document = {
      name: $activeScript.name, source,
      workingDirectory: $activeScript.workingDirectory,
    };
    void perform(() => runSelected(document));
  }
  function cancelCurrent() {
    if (!$taskState || !activeTask) return;
    void perform(async () => { await cancelTask($taskState!.taskId); await refreshTask(); });
  }
  async function saveLog() {
    if (!$scriptSettings.logPath.trim()) throw new Error("Enter a new output file path");
    await exportLog($scriptSettings.logPath);
    feedback = "Log saved. Existing files are never overwritten.";
  }

  onMount(() => {
    void refreshTask();
    // The workbench tracks the task even while this editor page is unmounted.
  });
</script>

<div class="automation-workspace">
  <input class="hidden-file-picker" type="file" accept=".py,.txt" bind:this={filePicker}
    aria-label={translate("automation.workspace.choose.file", $uiLanguage)} onchange={(event) => void chooseFile(event)} />
  <header class="page-toolbar">
    <div class="page-title">{translate("automation.workspace.title", $uiLanguage)}</div>
    <div class="toolbar-actions">
      <button class="tool-button" title={translate("automation.workspace.new", $uiLanguage)} onclick={createFile}>
        <i class="codicon codicon-new-file"></i> {translate("automation.workspace.new", $uiLanguage)}
      </button>
      <button class="tool-button" title={translate("automation.workspace.open", $uiLanguage)} onclick={() => filePicker?.click()}>
        <i class="codicon codicon-folder-opened"></i> {translate("automation.workspace.open", $uiLanguage)}
      </button>
      <button class="tool-button" disabled={!$activeScript || busy} onclick={saveCurrent}>
        <i class="codicon codicon-save"></i> {translate("automation.workspace.save", $uiLanguage)}
      </button>
      <button class="tool-button" disabled={!$activeScript || busy} onclick={promptSaveAs}>
        {translate("automation.workspace.save.as", $uiLanguage)}
      </button>
      <span class="toolbar-separator"></span>
      <button class="tool-button run-button" disabled={!connected || busy || activeTask || !$activeScript || !$activeScript.source.trim()} onclick={runCurrent}>
        <i class="codicon codicon-play"></i> Run
      </button>
      <button class="tool-button" disabled={!activeTask || busy} onclick={cancelCurrent}>
        <i class="codicon codicon-debug-stop"></i> {translate("automation.workspace.cancel", $uiLanguage)}
      </button>
      <button class="tool-button icon-only" title={translate("automation.workspace.settings", $uiLanguage)} aria-label={translate("automation.workspace.settings", $uiLanguage)}
        onclick={() => settingsVisible = !settingsVisible}><i class="codicon codicon-settings-gear"></i></button>
    </div>
  </header>

  {#if settingsVisible}
    <div class="settings-strip">
      <label>{translate("ui.interpreter", $uiLanguage)}
        <input class="compact-input" bind:value={$scriptSettings.program} disabled={activeTask}/>
      </label>
      <label>{translate("ui.timeout.s", $uiLanguage)}
        <input class="compact-input timeout" type="number" min="1" max="3600" step="1" bind:value={$scriptSettings.timeoutSeconds} disabled={activeTask}/>
      </label>
      <span>{translate("automation.workspace.snapshot.note", $uiLanguage)}</span>
      <span>{translate("automation.execution.notice", $uiLanguage)}</span>
    </div>
  {/if}
  {#if saveAsVisible && $activeScript}
    <div class="save-as-strip">
      <label>{translate("automation.workspace.save.as", $uiLanguage)}
        <input class="compact-input" bind:value={savePath} placeholder="/home/user/scripts/motor_test.py" onkeydown={(event) => { if (event.key === "Enter") void perform(saveAs); }} />
      </label>
      <button class="tool-button" disabled={busy || !savePath.trim()} onclick={() => void perform(saveAs)}>{translate("automation.workspace.save", $uiLanguage)}</button>
      <button class="tool-button" onclick={() => saveAsVisible = false}>{translate("automation.workspace.cancel", $uiLanguage)}</button>
    </div>
  {/if}
  {#if error || $taskError}
    <div class="editor-error" role="alert">{localizeFeedback(error || $taskError, $uiLanguage)}</div>
  {/if}

  <div class="workspace-body">
    {#if explorerVisible}
      <aside class="script-explorer">
        <div class="explorer-heading">
          <span>{translate("automation.workspace.explorer", $uiLanguage)}</span>
          <button class="icon-tool" aria-label={translate("automation.workspace.hide.explorer", $uiLanguage)} onclick={() => explorerVisible = false}><i class="codicon codicon-chevron-left"></i></button>
        </div>
        <div class="explorer-section">
          <div class="explorer-section-title">{translate("automation.workspace.local.files", $uiLanguage)}</div>
          <input class="compact-input file-path" aria-label={translate("automation.workspace.path", $uiLanguage)}
            placeholder="/home/user/scripts/test.py" bind:value={openPath}
            onkeydown={(event) => { if (event.key === "Enter") void perform(openFile); }}/>
          <button class="explorer-command" disabled={busy || !openPath.trim()} onclick={() => void perform(openFile)}>
            <i class="codicon codicon-folder-opened"></i> {translate("automation.workspace.open", $uiLanguage)}
          </button>
        </div>
        <div class="explorer-section">
          <div class="explorer-section-title">{translate("automation.workspace.opened", $uiLanguage)}</div>
          {#each $scriptTabs as tab (tab.id)}
            <button class:selected={$activeScriptId === tab.id} class="explorer-file" onclick={() => activateTab(tab.id)}>
              <i class="codicon codicon-file-code"></i>
              <span>{tab.name}</span>{#if isDirty(tab)}<span class="dirty-mark">●</span>{/if}
            </button>
          {/each}
        </div>
        <div class="explorer-section">
          <div class="explorer-section-title">{translate("automation.workspace.examples", $uiLanguage)}</div>
          <button class="explorer-file" onclick={() => void perform(() => selectExample("diagnosis"))}><i class="codicon codicon-file-code"></i> runtime_diagnosis.py</button>
          <button class="explorer-file" onclick={() => void perform(() => selectExample("encoder"))}><i class="codicon codicon-file-code"></i> encoder_turn_diagnosis.py</button>
          <button class="explorer-file" onclick={() => void perform(() => selectExample("motion"))}><i class="codicon codicon-file-code"></i> motion_workflow.py</button>
        </div>
      </aside>
    {/if}
    <div class="editor-column">
      <div class="script-tabs">
        {#if !explorerVisible}
          <button class="icon-tool reveal-explorer" aria-label={translate("automation.workspace.explorer", $uiLanguage)}
            onclick={() => explorerVisible = true}><i class="codicon codicon-files"></i></button>
        {/if}
        {#each $scriptTabs as tab (tab.id)}
          <div class:active={$activeScriptId === tab.id} class="script-tab">
            <button class="tab-name" title={tab.path ?? tab.name} onclick={() => activateTab(tab.id)}>
              <i class="codicon codicon-file-code"></i> {tab.name}{#if isDirty(tab)} <span class="dirty-mark">●</span>{/if}
            </button>
            <button class="tab-close" aria-label={translate("automation.workspace.close", $uiLanguage)}
              onclick={() => closeTab(tab.id)}><i class="codicon codicon-close"></i></button>
          </div>
        {/each}
      </div>
      <div class="editor-space">
        {#if $activeScript}
          <ScriptEditor tab={$activeScript} onEdit={editScript} onSave={saveCurrent} onRun={runCurrent} />
        {:else}
          <div class="editor-empty">
            <i class="codicon codicon-code"></i>
            <span>{translate("automation.workspace.empty", $uiLanguage)}</span>
          </div>
        {/if}
      </div>
      <div class="editor-status">
        <span>{$activeScript ? ($activeScript.path ?? translate("automation.workspace.unsaved", $uiLanguage)) : "—"}</span>
        <span>Python {#if $activeScript && isDirty($activeScript)} · {translate("automation.workspace.modified", $uiLanguage)}{/if}</span>
      </div>
    </div>
  </div>

  <section class="script-output">
    <div class="output-heading">
      <span><i class="codicon codicon-output"></i> {translate("automation.workspace.output", $uiLanguage)}</span>
      <span class="output-state">
        {#if $taskState}
          {$taskState.name} · {localize($taskState.state, $uiLanguage)}
          {#if $taskState.exitCode !== null} · {translate("automation.task.exit.code.label", $uiLanguage)} {$taskState.exitCode}{/if}
        {:else}
          {translate("automation.workspace.no.task", $uiLanguage)}
        {/if}
      </span>
      <button class="icon-tool" title={translate("ui.save.log", $uiLanguage)} disabled={!$taskState?.taskId}
        onclick={() => logExportVisible = !logExportVisible}><i class="codicon codicon-save"></i></button>
    </div>
    {#if logExportVisible}
      <div class="log-export">
        <input class="compact-input" bind:value={$scriptSettings.logPath} placeholder="/home/user/logs/script.log"/>
        <button class="tool-button" disabled={busy || !$taskState?.taskId} onclick={() => void perform(saveLog)}>{translate("ui.save.log", $uiLanguage)}</button>
      </div>
    {/if}
    {#if $taskState?.message}<div class="output-message">{localizeFeedback($taskState.message, $uiLanguage)}</div>{/if}
    {#if ($taskState?.firstSequence ?? 1) > 1}<div class="output-message">{translate("ui.old.output.was.truncated.by.the.bounded.log.buffer", $uiLanguage)}</div>{/if}
    <pre class="output-content" aria-label={translate("automation.output.aria", $uiLanguage)}>{output || translate("ui.no.output.yet", $uiLanguage)}</pre>
    {#if feedback}<div class="output-message">{localizeFeedback(feedback, $uiLanguage)}</div>{/if}
  </section>
</div>

<style>
  .hidden-file-picker { display: none; }
  .automation-workspace { display: flex; flex-direction: column; min-width: 0; min-height: 0; height: 100%; background: var(--vscode-editor-background, #1e1e1e); }
  .page-toolbar { flex: 0 0 36px; min-width: 0; }
  .toolbar-actions { display: flex; align-items: center; gap: 5px; }
  .toolbar-actions .tool-button { display: flex; align-items: center; gap: 6px; }
  .toolbar-separator { height: 18px; width: 1px; background: var(--vscode-panel-border); margin: 0 3px; }
  .run-button:not(:disabled) { color: var(--vscode-testing-iconPassed, #3fb950); }
  .settings-strip, .save-as-strip { display: flex; gap: 12px; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--vscode-panel-border); }
  .settings-strip label, .save-as-strip label { display: flex; align-items: center; gap: 6px; font-size: 11px; }
  .settings-strip span { color: var(--vscode-descriptionForeground); font-size: 11px; }
  .settings-strip input { width: 105px; } .settings-strip .timeout { width: 70px; }
  .save-as-strip label { flex: 1; } .save-as-strip input { flex: 1; min-width: 130px; }
  .editor-error { padding: 7px 12px; color: var(--vscode-errorForeground); background: var(--vscode-inputValidation-errorBackground, #402222); font-size: 11px; }
  .workspace-body { flex: 1 1 auto; display: flex; min-height: 190px; min-width: 0; border-bottom: 1px solid var(--vscode-panel-border); }
  .script-explorer { flex: 0 0 190px; width: 190px; min-height: 0; overflow: auto; border-right: 1px solid var(--vscode-panel-border); background: var(--vscode-sideBar-background, #252526); }
  .explorer-heading { height: 32px; padding: 0 10px; display: flex; align-items: center; justify-content: space-between; font-size: 11px; font-weight: 600; }
  .explorer-section { margin: 8px 0 16px; min-width: 0; }
  .explorer-section-title { padding: 4px 10px; color: var(--vscode-descriptionForeground); font-size: 10px; font-weight: 600; }
  .file-path { width: calc(100% - 16px); margin: 4px 8px; box-sizing: border-box; }
  .explorer-command, .explorer-file { display: flex; align-items: center; gap: 7px; width: 100%; min-width: 0; border: 0; background: transparent; color: inherit; text-align: left; font: inherit; font-size: 11px; cursor: pointer; padding: 6px 10px; }
  .explorer-file:hover, .explorer-command:hover, .explorer-file.selected { background: var(--vscode-list-hoverBackground, #343434); }
  .explorer-file span:not(.dirty-mark) { overflow: hidden; text-overflow: ellipsis; }
  .dirty-mark { color: var(--vscode-notificationsInfoIcon-foreground, #75beff); font-size: 10px; }
  .editor-column { flex: 1 1 auto; min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  .script-tabs { display: flex; flex: 0 0 34px; height: 34px; align-items: stretch; overflow-x: auto; overflow-y: hidden; background: var(--vscode-editorGroupHeader-tabsBackground, #252526); border-bottom: 1px solid var(--vscode-panel-border); }
  .script-tab { flex: 0 0 auto; display: flex; align-items: center; border-right: 1px solid var(--vscode-panel-border); color: var(--vscode-tab-inactiveForeground); }
  .script-tab.active { background: var(--vscode-editor-background); color: var(--vscode-tab-activeForeground); border-top: 1px solid var(--vscode-focusBorder); }
  .tab-name, .tab-close, .icon-tool { border: 0; background: transparent; color: inherit; cursor: pointer; font: inherit; }
  .tab-name { display: flex; align-items: center; gap: 5px; white-space: nowrap; padding: 8px 7px 8px 11px; font-size: 11px; }
  .tab-close { padding: 6px 8px; } .tab-close:hover, .icon-tool:hover { background: var(--vscode-list-hoverBackground); }
  .reveal-explorer { padding: 7px 10px; }
  .editor-space { flex: 1 1 auto; min-height: 0; min-width: 0; overflow: hidden; }
  .editor-empty { display: flex; height: 100%; flex-direction: column; gap: 14px; align-items: center; justify-content: center; color: var(--vscode-descriptionForeground); font-size: 12px; }
  .editor-empty .codicon { font-size: 30px; opacity: 0.6; }
  .editor-status { display: flex; justify-content: space-between; gap: 15px; min-height: 22px; padding: 3px 10px; box-sizing: border-box; font-size: 10px; color: var(--vscode-descriptionForeground); background: var(--vscode-statusBar-background, #202020); }
  .editor-status span:first-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .script-output { flex: 0 0 29%; min-height: 100px; max-height: 45%; display: flex; flex-direction: column; min-width: 0; }
  .output-heading { display: flex; gap: 10px; align-items: center; min-height: 29px; padding: 0 12px; border-bottom: 1px solid var(--vscode-panel-border); font-size: 11px; font-weight: 600; }
  .output-state { flex: 1; text-align: right; color: var(--vscode-descriptionForeground); font-weight: 400; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .log-export { display: flex; gap: 8px; padding: 6px 10px; border-bottom: 1px solid var(--vscode-panel-border); }
  .log-export input { flex: 1; min-width: 100px; }
  .output-content { flex: 1; min-height: 0; margin: 0; overflow: auto; padding: 9px 12px; font: 11px/1.6 "SFMono-Regular", Consolas, monospace; white-space: pre-wrap; overflow-wrap: anywhere; }
  .output-message { padding: 4px 12px; font-size: 11px; color: var(--vscode-descriptionForeground); }
  @media(max-width: 850px) {
    .script-explorer { flex-basis: 155px; width: 155px; }
    .toolbar-actions { gap: 2px; }
    .toolbar-actions .tool-button { padding: 2px 4px; }
  }
</style>
