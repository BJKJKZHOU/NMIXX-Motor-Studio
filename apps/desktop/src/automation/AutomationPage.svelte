<script lang="ts">
  import { uiLanguage, localize, localizeFeedback, translate } from "../i18n";
  import { onMount } from "svelte";
  import { cancelTask, exportLog } from "./api";
  import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
  import {
    activeScript, activeScriptId, closeScript, editScript, isDirty, newScript,
    openExample, openLocalScript, saveCurrentScript, scriptTabs, selectScript,
  } from "./documents";
  import { isActive, logs, refreshTask, runSelected, scriptSettings, taskError, taskState } from "./state";
  import ScriptEditor from "./ScriptEditor.svelte";

  export let connected = false;
  let busy = false;
  let error = "";
  let feedback = "";
  let explorerVisible = true;
  let settingsVisible = false;
  $: activeTask = isActive($taskState?.state);
  $: output = $logs.map((line) => `[${(line.elapsedMs / 1000).toFixed(3)}s ${line.source}] ${line.text}`).join("\n");

  async function perform(operation: () => Promise<unknown>) {
    if (busy) return;
    busy = true; error = ""; feedback = "";
    try { await operation(); } catch (failure) { error = String(failure); }
    finally { busy = false; }
  }
  async function openFile() {
    const path = await openFileDialog({
      multiple: false, directory: false,
      filters: [{ name: "Python", extensions: ["py"] }],
    });
    if (typeof path === "string") await openLocalScript(path);
  }
  function createFile() { newScript(); }
  function activateTab(id: string) { selectScript(id); }
  async function selectExample(kind: "diagnosis" | "encoder" | "motion") {
    await openExample(kind);
  }
  function closeTab(id: string) {
    const tab = $scriptTabs.find((item) => item.id === id);
    if (!tab) return;
    if (isDirty(tab) && !window.confirm(translate("automation.workspace.discard.prompt", $uiLanguage))) return;
    closeScript(id);
  }
  async function saveAs() {
    const current = $activeScript;
    if (!current) return;
    const path = await saveFileDialog({
      defaultPath: current.path ?? current.name,
      filters: [{ name: "Python", extensions: ["py"] }],
    });
    if (path) {
      // The native Save dialog confirms replacement of an existing target.
      await saveCurrentScript(path, true);
    }
  }
  function saveCurrent() {
    const current = $activeScript;
    if (!current || busy) return;
    void perform(current.path && !current.example
      ? () => saveCurrentScript()
      : saveAs);
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
    const path = await saveFileDialog({
      defaultPath: "script.log",
      filters: [{ name: "Log", extensions: ["log", "txt"] }],
    });
    if (!path) return;
    await exportLog(path);
    feedback = "Log saved. Existing files are never overwritten.";
  }

  onMount(() => {
    void refreshTask();
    // The workbench tracks the task even while this editor page is unmounted.
  });
</script>

<div class="automation-workspace">
  <header class="page-toolbar">
    <div class="page-title">{translate("automation.workspace.title", $uiLanguage)}</div>
    <div class="toolbar-actions">
      <button class="tool-button" title={translate("automation.workspace.new", $uiLanguage)} onclick={createFile}>
        <i class="codicon codicon-new-file"></i> {translate("automation.workspace.new", $uiLanguage)}
      </button>
      <button class="tool-button" title={translate("automation.workspace.open", $uiLanguage)} onclick={() => void perform(openFile)}>
        <i class="codicon codicon-folder-opened"></i> {translate("automation.workspace.open", $uiLanguage)}
      </button>
      <button class="tool-button" disabled={!$activeScript || busy} onclick={saveCurrent}>
        <i class="codicon codicon-save"></i> {translate("automation.workspace.save", $uiLanguage)}
      </button>
      <button class="tool-button" disabled={!$activeScript || busy} onclick={() => void perform(saveAs)}>
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
  {#if error || $taskError}
    <div class="editor-error" role="alert">{localizeFeedback(error || $taskError, $uiLanguage)}</div>
  {/if}

  <!-- Built-in VSCode Elements splitter, rather than a custom drag/resize system. -->
  <vscode-split-layout class="workspace-split" split="horizontal"
    initial-handle-position="71%" min-start="190px" min-end="100px">
    <div slot="start" class="workspace-body">
    {#if explorerVisible}
      <aside class="script-explorer">
        <div class="explorer-heading">
          <span>{translate("automation.workspace.explorer", $uiLanguage)}</span>
          <button class="icon-tool" aria-label={translate("automation.workspace.hide.explorer", $uiLanguage)} onclick={() => explorerVisible = false}><i class="codicon codicon-chevron-left"></i></button>
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

    <section slot="end" class="script-output">
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
        onclick={() => void perform(saveLog)}><i class="codicon codicon-save"></i></button>
    </div>
    {#if $taskState?.message}<div class="output-message">{localizeFeedback($taskState.message, $uiLanguage)}</div>{/if}
    {#if ($taskState?.firstSequence ?? 1) > 1}<div class="output-message">{translate("ui.old.output.was.truncated.by.the.bounded.log.buffer", $uiLanguage)}</div>{/if}
    <pre class="output-content" aria-label={translate("automation.output.aria", $uiLanguage)}>{output || translate("ui.no.output.yet", $uiLanguage)}</pre>
    {#if feedback}<div class="output-message">{localizeFeedback(feedback, $uiLanguage)}</div>{/if}
    </section>
  </vscode-split-layout>
</div>

<style>
  .automation-workspace { display: flex; flex-direction: column; min-width: 0; min-height: 0; height: 100%; background: var(--vscode-editor-background, #1e1e1e); }
  .page-toolbar { flex: 0 0 36px; min-width: 0; }
  .toolbar-actions { display: flex; align-items: center; gap: 5px; }
  .toolbar-actions .tool-button { display: flex; align-items: center; gap: 6px; }
  .toolbar-separator { height: 18px; width: 1px; background: var(--vscode-panel-border); margin: 0 3px; }
  .run-button:not(:disabled) { color: var(--vscode-testing-iconPassed, #3fb950); }
  .settings-strip { display: flex; gap: 12px; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--vscode-panel-border); }
  .settings-strip label { display: flex; align-items: center; gap: 6px; font-size: 11px; }
  .settings-strip span { color: var(--vscode-descriptionForeground); font-size: 11px; }
  .settings-strip input { width: 105px; } .settings-strip .timeout { width: 70px; }
  .editor-error { padding: 7px 12px; color: var(--vscode-errorForeground); background: var(--vscode-inputValidation-errorBackground, #402222); font-size: 11px; }
  .workspace-split { display: block; flex: 1 1 auto; min-width: 0; min-height: 0; }
  .workspace-body { display: flex; width: 100%; height: 100%; min-width: 0; min-height: 0; }
  .script-explorer { flex: 0 0 190px; width: 190px; min-height: 0; overflow: auto; border-right: 1px solid var(--vscode-panel-border); background: var(--vscode-sideBar-background, #252526); }
  .explorer-heading { height: 32px; padding: 0 10px; display: flex; align-items: center; justify-content: space-between; font-size: 11px; font-weight: 600; }
  .explorer-section { margin: 8px 0 16px; min-width: 0; }
  .explorer-section-title { padding: 4px 10px; color: var(--vscode-descriptionForeground); font-size: 10px; font-weight: 600; }
  .explorer-file { display: flex; align-items: center; gap: 7px; width: 100%; min-width: 0; border: 0; background: transparent; color: inherit; text-align: left; font: inherit; font-size: 11px; cursor: pointer; padding: 6px 10px; }
  .explorer-file:hover, .explorer-file.selected { background: var(--vscode-list-hoverBackground, #343434); }
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
  .script-output { display: flex; flex-direction: column; width: 100%; height: 100%; min-width: 0; min-height: 0; }
  .output-heading { display: flex; gap: 10px; align-items: center; min-height: 29px; padding: 0 12px; border-bottom: 1px solid var(--vscode-panel-border); font-size: 11px; font-weight: 600; }
  .output-state { flex: 1; text-align: right; color: var(--vscode-descriptionForeground); font-weight: 400; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .output-content { flex: 1; min-height: 0; margin: 0; overflow: auto; padding: 9px 12px; font: 11px/1.6 "SFMono-Regular", Consolas, monospace; white-space: pre-wrap; overflow-wrap: anywhere; }
  .output-message { padding: 4px 12px; font-size: 11px; color: var(--vscode-descriptionForeground); }
  @media(max-width: 850px) {
    .script-explorer { flex-basis: 155px; width: 155px; }
    .toolbar-actions { gap: 2px; }
    .toolbar-actions .tool-button { padding: 2px 4px; }
  }
</style>
