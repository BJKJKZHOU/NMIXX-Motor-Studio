<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import ConnectionPage from "./connection/ConnectionPage.svelte";
  import { disconnectDevice } from "./connection/api";
  import type { ConnectionInfo } from "./connection/types";
  import ScopePage from "./analysis/scope/ScopeEchartsPage.svelte";
  import type { ScopeSummary } from "./analysis/scope/types";
  import ParameterTablePage from "./parameters/ParameterTablePage.svelte";
  import MotorPage from "./motor/MotorPage.svelte";
  import EncoderPage from "./encoder/EncoderPage.svelte";
  import LimitsPage from "./limits/LimitsPage.svelte";
  import ControlPage from "./control/ControlPage.svelte";
  import ControlTuningPage from "./control/ControlTuningPage.svelte";
  import MotionPage from "./motion/MotionPage.svelte";
  import { canSaveParameters, disableMotor, enableMotor, saveParameters, stopMotor } from "./actions/api";
  import { connectParameters, disconnectParameters, parameterState, pollParameters, refreshParameters, saveParameterBaseline, selectParameters } from "./parameters/state";
  import type { ParameterValue } from "./parameters/types";

  type Page = "connection" | "motor" | "encoder" | "limits" | "control" | "tuning" | "motion" | "analysis" | "parameters" | "events" | "automation";
  type ControlLoopPage = "current" | "speed" | "position";
  const GLOBAL_SYMBOLS = ["PARAM_MOTOR_STATE", "PARAM_RUN_IQ", "PARAM_RUN_WM", "PARAM_RUN_POSITION"] as const;
  const MOTOR_DISABLED = 0;
  const DEFAULT_SCHEMA_PATH = "../../../AxDr_L_Motor/build/host/axdr-host-schema.toml";
  const SCHEMA_PATH_STORAGE_KEY = "nmixx.connection.hostSchemaPath";
  const globalParameters = selectParameters(GLOBAL_SYMBOLS);
  $: motorState = numeric($globalParameters.values.PARAM_MOTOR_STATE);
  $: currentIq = numeric($globalParameters.values.PARAM_RUN_IQ);
  $: speedWm = numeric($globalParameters.values.PARAM_RUN_WM);
  $: positionText = position($globalParameters.values.PARAM_RUN_POSITION);
  let activePage: Page = "connection";
  let activeControlLoop: ControlLoopPage = "current";
  let controlArchitectureExpanded = true;
  let connectionPort = "";
  let connectionSchemaPath = DEFAULT_SCHEMA_PATH;
  let connection: ConnectionInfo | undefined;
  let errorText = "";
  let scopeSummary: ScopeSummary = { state: "STOPPED", selectedChannels: 0, lostFrames: 0 };
  let globalActionBusy = false;
  let globalStopBusy = false;
  let parameterSaveAvailable = false;
  let saveFeedback: "idle" | "saved" = "idle";
  let saveFeedbackTimer: ReturnType<typeof setTimeout> | undefined;
  let readingParameters = false;
  let connectionGeneration = 0;

  const workflowPages: Array<{ id: Page; title: string; icon: string }> = [
    { id: "connection", title: "Connection", icon: "codicon-plug" },
    { id: "motor", title: "Motor", icon: "codicon-circuit-board" },
    { id: "encoder", title: "Encoder", icon: "codicon-record" },
    { id: "limits", title: "Limits / Safety", icon: "codicon-shield" },
    { id: "control", title: "Control Architecture", icon: "codicon-settings-gear" },
    { id: "motion", title: "Motion", icon: "codicon-play-circle" },
    { id: "tuning", title: "Control Tuning", icon: "codicon-pulse" },
    { id: "analysis", title: "Analysis", icon: "codicon-graph-line" },
  ];
  const toolPages: Array<{ id: Page; title: string; icon: string }> = [
    { id: "parameters", title: "Parameters", icon: "codicon-list-flat" },
    { id: "events", title: "Events", icon: "codicon-warning" },
    { id: "automation", title: "Automation", icon: "codicon-terminal" },
  ];
  function setError(error: unknown) { errorText = error instanceof Error ? error.message : String(error); }
  function persistConnectionSchemaPath(path: string) {
    try { localStorage.setItem(SCHEMA_PATH_STORAGE_KEY, path); } catch { /* The in-memory path remains usable. */ }
  }
  async function setConnection(next: ConnectionInfo | undefined) {
    const token = ++connectionGeneration;
    connection = next;
    errorText = ""; parameterSaveAvailable = false; saveFeedback = "idle";
    if (saveFeedbackTimer) clearTimeout(saveFeedbackTimer);
    saveFeedbackTimer = undefined;
    if (!next) {
      disconnectParameters();
      scopeSummary = { state: "STOPPED", selectedChannels: 0, lostFrames: 0 };
      return;
    }
    try {
      await connectParameters();
      const available = await canSaveParameters();
      if (token === connectionGeneration) parameterSaveAvailable = available;
    } catch (error) { if (token === connectionGeneration) setError(error); }
  }
  function pageTitle(page: Page): string { return [...workflowPages, ...toolPages].find((item) => item.id === page)?.title ?? page; }
  function numeric(value: ParameterValue | null | undefined): number | null { return !value || value.type === "position" ? null : Number(value.value); }
  function position(value: ParameterValue | null | undefined): string { return value?.type === "position" ? `${value.value.turns} turn + ${Number(value.value.theta).toFixed(3)} rad` : "—"; }
  async function refreshAllParameters() {
    if (!connection || readingParameters) return;
    readingParameters = true;
    try { await refreshParameters(); } catch (error) { setError(error); } finally { readingParameters = false; }
  }
  function showSavedFeedback() {
    saveFeedback = "saved";
    if (saveFeedbackTimer) clearTimeout(saveFeedbackTimer);
    saveFeedbackTimer = setTimeout(() => { saveFeedback = "idle"; saveFeedbackTimer = undefined; }, 1400);
  }
  async function toggleMotorEnable() {
    if (!connection || motorState === null || globalActionBusy) return;
    globalActionBusy = true;
    try { if (motorState === MOTOR_DISABLED) await enableMotor(); else await disableMotor(); }
    catch (error) { setError(error); } finally { globalActionBusy = false; }
  }
  async function stopCurrentMotorOperation() {
    if (!connection || globalStopBusy) return;
    globalStopBusy = true;
    try { await stopMotor(); } catch (error) { setError(error); } finally { globalStopBusy = false; }
  }
  async function savePersistentParameters() {
    if (!connection || !parameterSaveAvailable || motorState !== MOTOR_DISABLED || globalActionBusy) return;
    globalActionBusy = true; saveFeedback = "idle";
    try { await saveParameterBaseline(saveParameters); showSavedFeedback(); }
    catch (error) { setError(error); } finally { globalActionBusy = false; }
  }
  onMount(() => {
    try {
      const storedPath = localStorage.getItem(SCHEMA_PATH_STORAGE_KEY);
      if (storedPath?.trim()) connectionSchemaPath = storedPath;
    } catch { /* Use the development default when storage is unavailable. */ }
    return pollParameters(GLOBAL_SYMBOLS, 500, setError);
  });
  onDestroy(() => {
    ++connectionGeneration;
    if (saveFeedbackTimer) clearTimeout(saveFeedbackTimer);
    disconnectParameters();
    disconnectDevice().catch(() => undefined);
  });
</script>

<div class="workbench"><div class="body">
  <nav class="activity-bar" aria-label="Commissioning workflow">
    {#each workflowPages as page}<button class:active={activePage === page.id} class="activity" title={page.title} onclick={() => activePage = page.id}><i class={`codicon ${page.icon}`}></i></button>{/each}
    <div class="workflow-separator"></div>
    {#each toolPages as page}<button class:active={activePage === page.id} class="activity" title={page.title} onclick={() => activePage = page.id}><i class={`codicon ${page.icon}`}></i></button>{/each}
    <div class="activity-spacer"></div>
  </nav>
  <div class="global-toolbar"><div class="global-toolbar-left">
    <div class="brand">NMIXX Motor Studio</div><div class="global-toolbar-divider" aria-hidden="true"></div>
    <div class="global-actions">
      <button class:enable-action={motorState === MOTOR_DISABLED} class:disable-action={motorState !== null && motorState !== MOTOR_DISABLED} class="tool-button global-action"
        disabled={!connection || motorState === null || globalActionBusy} onclick={() => void toggleMotorEnable()} title={motorState === MOTOR_DISABLED ? "Enable motor" : "Disable motor"}>
        <i class={`codicon ${motorState === MOTOR_DISABLED ? "codicon-play" : "codicon-debug-disconnect"}`}></i>{motorState === MOTOR_DISABLED ? "Enable" : "Disable"}
      </button>
      <button class="tool-button global-action stop-action" disabled={!connection || globalStopBusy} onclick={() => void stopCurrentMotorOperation()} title="Stop current motor operation">
        <i class={`codicon ${globalStopBusy ? "codicon-loading codicon-modifier-spin" : "codicon-debug-stop"}`}></i> Stop
      </button>
      <span class="global-group-gap"></span>
      <button class="tool-button global-action" disabled={!connection || readingParameters} onclick={() => void refreshAllParameters()} title="Read current RAM parameters from device"><i class={`codicon ${readingParameters ? "codicon-loading codicon-modifier-spin" : "codicon-refresh"}`}></i> Read</button>
      <button class="tool-button global-action" disabled={!connection || !parameterSaveAvailable || motorState !== MOTOR_DISABLED || globalActionBusy} onclick={() => void savePersistentParameters()}
        title={!parameterSaveAvailable ? "Parameter persistence is not exposed by this firmware" : motorState !== MOTOR_DISABLED ? "Disable the motor before saving persistent parameters" : "Save persistent RAM parameters to device storage"}>
        <i class={`codicon ${saveFeedback === "saved" ? "codicon-check" : "codicon-save"}`}></i>{saveFeedback === "saved" ? "Saved" : "Save"}
      </button>
    </div>
  </div><button class="problems-indicator" disabled title="Problems service is not implemented yet"><i class="codicon codicon-warning"></i><span>0</span></button></div>
  <main class="main-area">
    {#if activePage === "connection"}
      <ConnectionPage {connection} bind:port={connectionPort} bind:schemaPath={connectionSchemaPath} onSchemaPathChanged={persistConnectionSchemaPath} onConnected={(next) => void setConnection(next)} onDisconnected={() => void setConnection(undefined)} onError={setError} />
    {:else if activePage === "motor"}<div class="domain-page-container"><MotorPage {connection} onError={setError} /></div>
    {:else if activePage === "encoder"}<div class="domain-page-container"><EncoderPage {connection} onError={setError} /></div>
    {:else if activePage === "limits"}<div class="domain-page-container"><LimitsPage {connection} onError={setError} /></div>
    {:else if activePage === "control"}
      <div class="control-domain-shell"><aside class="control-navigation" aria-label="Control Architecture pages">
        <button class="control-navigation-parent" onclick={() => controlArchitectureExpanded = !controlArchitectureExpanded} aria-expanded={controlArchitectureExpanded}><i class="codicon codicon-settings-gear"></i><span>Control Architecture</span><i class={`codicon ${controlArchitectureExpanded ? "codicon-chevron-down" : "codicon-chevron-right"} control-navigation-chevron`}></i></button>
        {#if controlArchitectureExpanded}<div class="control-navigation-children">
          <button class:active={activeControlLoop === "current"} onclick={() => activeControlLoop = "current"}>Current Loop</button>
          <button class:active={activeControlLoop === "speed"} onclick={() => activeControlLoop = "speed"}>Speed Loop</button>
          <button class:active={activeControlLoop === "position"} onclick={() => activeControlLoop = "position"}>Position Loop</button>
        </div>{/if}
      </aside><div class="control-domain-page"><ControlPage {connection} {motorState} loop={activeControlLoop} onError={setError} /></div></div>
    {:else if activePage === "tuning"}<div class="domain-page-container"><ControlTuningPage {connection} {motorState} onError={setError} /></div>
    {:else if activePage === "motion"}<div class="domain-page-container"><MotionPage capabilities={connection?.motion} {motorState} onError={setError} /></div>
    {:else if activePage === "parameters"}<div class="domain-page-container"><ParameterTablePage {connection} onError={setError} /></div>
    {:else if activePage !== "analysis"}
      <section class="page-toolbar"><div class="page-title">{pageTitle(activePage).toUpperCase()}</div></section>
      <section class="placeholder-page"><div class="placeholder-title">{pageTitle(activePage)}</div><div class="placeholder-copy">This workflow page is reserved for the corresponding application domain. Device behavior will be added through the shared Application API rather than implemented in the shell.</div></section>
    {/if}
    <div class:inactive={activePage !== "analysis"} class="scope-page-container"><ScopePage {connection} active={activePage === "analysis"} onSummary={(summary) => scopeSummary = summary} onError={setError} /></div>
    {#if errorText || $parameterState.error}<div class="error-text app-error">{errorText || $parameterState.error}</div>{/if}
  </main>
</div>
<footer class="statusbar"><div class="connection-status"><span class:connected={!!connection} class="status-dot"></span>{connection ? "AxDr_L" : "Disconnected"}</div><div class="status-spacer"></div>
  <div class="live-status"><span>Current {currentIq === null ? "—" : `${currentIq.toFixed(3)} A`}</span><span>Speed {speedWm === null ? "—" : `${speedWm.toFixed(3)} rad/s`}</span><span>Position {positionText}</span></div>
</footer></div>

<style>
  .global-toolbar { grid-column: 1 / -1; grid-row: 1; min-width: 0; display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 0 10px 0 14px; background: #181818; border-bottom: 1px solid var(--vscode-panel-border, #2b2b2b); }
  .global-toolbar-left { min-width: 0; display: flex; align-items: center; gap: 12px; }
  .global-toolbar-divider { width: 1px; height: 22px; flex: 0 0 auto; background: var(--vscode-panel-border, #303030); }
  .global-actions { display: flex; align-items: center; gap: 5px; min-width: 0; }
  .global-group-gap { width: 7px; }
  .global-action { min-width: 72px; }
  .enable-action:not(:disabled) { color: #3fb950; }
  .disable-action:not(:disabled) { color: #f85149; }
  .stop-action:not(:disabled) { font-weight: 600; }
  .problems-indicator { flex: 0 0 auto; display: inline-flex; align-items: center; gap: 5px; border: 0; background: transparent; color: inherit; padding: 4px 7px; }
  .domain-page-container { grid-row: 1 / -1; min-width: 0; min-height: 0; display: grid; }
  .control-domain-shell { grid-row: 1 / -1; min-width: 0; min-height: 0; display: grid; grid-template-columns: 196px minmax(0, 1fr); }
  .control-navigation { min-width: 0; min-height: 0; background: var(--vscode-sideBar-background); border-right: 1px solid var(--vscode-panel-border); }
  .control-navigation-parent, .control-navigation-children button { width: 100%; border: 0; color: var(--vscode-sideBar-foreground); background: transparent; text-align: left; cursor: default; }
  .control-navigation-parent { height: 36px; display: grid; grid-template-columns: 20px minmax(0, 1fr) 18px; align-items: center; gap: 7px; padding: 0 8px 0 10px; font-size: 12px; font-weight: 600; border-bottom: 1px solid color-mix(in srgb, var(--vscode-panel-border) 75%, transparent); }
  .control-navigation-parent:hover, .control-navigation-children button:hover { background: var(--vscode-list-hoverBackground); }
  .control-navigation-chevron { justify-self: end; color: var(--vscode-descriptionForeground); }
  .control-navigation-children { display: grid; }
  .control-navigation-children button { height: 31px; padding: 0 12px 0 37px; font-size: 12px; color: var(--vscode-descriptionForeground); }
  .control-navigation-children button.active { color: var(--vscode-list-activeSelectionForeground); background: var(--vscode-list-activeSelectionBackground); font-weight: 600; }
  .control-domain-page { min-width: 0; min-height: 0; display: grid; }
  .connection-status { display: inline-flex; align-items: center; gap: 6px; }
  .live-status { display: flex; align-items: center; justify-content: flex-end; gap: 18px; white-space: nowrap; text-align: right; }
</style>
