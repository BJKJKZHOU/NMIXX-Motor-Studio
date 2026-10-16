<script lang="ts">
  import { onMount } from "svelte";
  import type { ScriptTab } from "./documents";

  export let tab: ScriptTab;
  export let onEdit: (id: string, source: string) => void;
  export let onSave: () => void;
  export let onRun: () => void;

  type Monaco = typeof import("monaco-editor");
  let host: HTMLDivElement;
  let monaco: Monaco | undefined;
  let editor: import("monaco-editor").editor.IStandaloneCodeEditor | undefined;
  const models = new Map<string, import("monaco-editor").editor.ITextModel>();
  let loaded = false;
  let fallbackReason = "";

  function show(tab: ScriptTab) {
    if (!monaco || !editor) return;
    let model = models.get(tab.id);
    if (!model) {
      const uri = monaco.Uri.parse(`inmemory://nmixx-scripts/${encodeURIComponent(tab.id)}.py`);
      model = monaco.editor.createModel(tab.source, "python", uri);
      model.onDidChangeContent(() => {
        const value = model!.getValue();
        onEdit(tab.id, value);
      });
      models.set(tab.id, model);
    } else if (model.getValue() !== tab.source) {
      model.setValue(tab.source);
    }
    if (editor.getModel() !== model) editor.setModel(model);
  }

  $: if (editor && tab) show(tab);

  onMount(() => {
    let disposed = false;
    let observer: ResizeObserver | undefined;
    void (async () => {
      try {
        // Monaco and its worker are bundled locally, and load only in Automation.
        const [module, worker] = await Promise.all([
          import("monaco-editor"),
          import("monaco-editor/esm/vs/editor/editor.worker?worker"),
        ]);
        if (disposed) return;
        (self as unknown as {
          MonacoEnvironment?: { getWorker: () => Worker }
        }).MonacoEnvironment = { getWorker: () => new worker.default() };
        monaco = module;
        editor = monaco.editor.create(host, {
          language: "python", theme: "vs-dark", fontSize: 12,
          automaticLayout: true, minimap: { enabled: false },
          scrollBeyondLastLine: false, tabSize: 4, insertSpaces: true,
          wordWrap: "off", lineNumbers: "on", glyphMargin: false,
          renderWhitespace: "selection", padding: { top: 10 },
        });
        editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, onSave);
        editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, onRun);
        show(tab);
        loaded = true;
        observer = new ResizeObserver(() => editor?.layout());
        observer.observe(host);
      } catch (error) {
        if (!disposed) fallbackReason = String(error);
      }
    })();
    return () => {
      disposed = true;
      observer?.disconnect();
      editor?.dispose();
      for (const model of models.values()) model.dispose();
      models.clear();
    };
  });

  function keyboard(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault(); onSave();
    } else if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
      event.preventDefault(); onRun();
    }
  }
</script>

<div class="script-editor">
  <div class:editor-hidden={!loaded} class="monaco-host" bind:this={host}></div>
  {#if !loaded}
    <textarea
      class="fallback-editor"
      aria-label="Python script editor"
      value={tab.source}
      oninput={(event) => onEdit(tab.id, event.currentTarget.value)}
      onkeydown={keyboard}
      spellcheck="false"
    ></textarea>
    {#if fallbackReason}<div class="editor-fallback-notice">Monaco unavailable; plain text editing remains available.</div>{/if}
  {/if}
</div>

<style>
  .script-editor { position: relative; height: 100%; min-width: 0; min-height: 0; overflow: hidden; background: #1e1e1e; }
  .monaco-host { position: absolute; inset: 0; }
  .monaco-host.editor-hidden { visibility: hidden; }
  .fallback-editor {
    position: absolute; inset: 0; width: 100%; height: 100%; resize: none;
    box-sizing: border-box; border: 0; outline: 0; padding: 14px 18px;
    background: #1e1e1e; color: #d4d4d4; font: 12px/1.6 "SFMono-Regular", Consolas, monospace;
    tab-size: 4; white-space: pre; overflow: auto;
  }
  .editor-fallback-notice {
    position: absolute; right: 8px; bottom: 8px;
    background: var(--vscode-editorWidget-background, #252526);
    color: var(--vscode-descriptionForeground); padding: 4px 8px; font-size: 10px;
  }
</style>
