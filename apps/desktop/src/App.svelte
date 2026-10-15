<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { uiLanguage, restoreUiLanguage, cycleUiLanguage, languageBadge, languageSwitchTitle, localize, localizeFeedback, translate, type MessageKey } from "./i18n";
  import ConnectionPage from "./connection/ConnectionPage.svelte";
  import AutomationHost from "./automation/AutomationHost.svelte";
  import { startAutomationTracking, clearAutomationProjection } from "./automation/state";
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
  import EventsPage from "./events/EventsPage.svelte";
  import { clearProblemsProjection, problemSnapshot, refreshProblems, startProblemsTracking } from "./problems/state";
  import { canSaveParameters, disableMotor, enableMotor, saveParameters, stopMotor } from "./actions/api";
  import { connectParameters, disconnectParameters, parameterState, pollParameters, refreshParameters, saveParameterBaseline, selectParameters } from "./parameters/state";
  import { motorEnableStatus, motorStateFromParameter } from "./motor/state";
  import { clearRuntimeTelemetry, refreshRuntimeTelemetry, runtimeTelemetry, startRuntimeTelemetryTracking } from "./runtime/state";

  type Page = "connection" | "motor" | "encoder" | "limits" | "control" | "tuning" | "motion" | "analysis" | "parameters" | "events" | "automation";
  type ControlLoopPage = "current" | "speed" | "position";
  const GLOBAL_SYMBOLS = ["PARAM_MOTOR_STATE"] as const;
  // Motor state is not a Plot signal; runtime Current/Speed/Position/Vbus come
  // from the connection-owned 1 kHz baseline acquisition instead.
  const POLLED_SYMBOLS = ["PARAM_MOTOR_STATE", "PARAM_ENCODER_PROTOCOL", "PARAM_ENCODER_READY", "PARAM_ENCODER_VALID", "PARAM_ENCODER_FAULT"] as const;
  const DEFAULT_SCHEMA_PATH = "../../../AxDr_L_Motor/build/host/axdr-host-schema.toml";
  const SCHEMA_PATH_STORAGE_KEY = "nmixx.connection.hostSchemaPath";
  const globalParameters = selectParameters(GLOBAL_SYMBOLS);
  $: motorState = motorStateFromParameter($globalParameters.metadata.PARAM_MOTOR_STATE, $globalParameters.values.PARAM_MOTOR_STATE);
  $: currentIq = $runtimeTelemetry.currentIq ?? null;
  $: speedWm = $runtimeTelemetry.speedWm ?? null;
  $: positionText = runtimePosition($runtimeTelemetry.positionTurns ?? null);
  $: vbus = $runtimeTelemetry.vbus ?? null;
  $: activeProblems = $problemSnapshot.active;
  $: topProblem = activeProblems[0];
  let activePage: Page = "connection";
  let activeControlLoop: ControlLoopPage = "current";
  let controlArchitectureExpanded = true;
  let connectionPort = "";
  let connectionSchemaPath = DEFAULT_SCHEMA_PATH;
  let connection: ConnectionInfo | undefined;
  let connectionReady = false;
  $: enableStatus = motorEnableStatus(motorState, connectionReady);
  let connectionInitializing = false;
  $: readyConnection = connectionReady ? connection : undefined;
  let errorText = "";
  let scopeSummary: ScopeSummary = { state: "STOPPED", selectedChannels: 0, lostFrames: 0 };
  let globalActionBusy = false;
  let globalStopBusy = false;
  let motorActionNotice = "";
  let parameterSaveAvailable = false;
  let saveFeedback: "idle" | "saved" = "idle";
  let saveFeedbackTimer: ReturnType<typeof setTimeout> | undefined;
  let readingParameters = false;
  let connectionGeneration = 0;
  let problemsOpen = false;

  const workflowPages: Array<{ id: Page; titleKey: MessageKey; icon: string }> = [
    { id: "connection", titleKey: "ui.connection", icon: "codicon-plug" },
    { id: "motor", titleKey: "ui.motor", icon: "codicon-circuit-board" },
    { id: "encoder", titleKey: "ui.encoder", icon: "codicon-record" },
    { id: "limits", titleKey: "ui.limits.safety", icon: "codicon-shield" },
    { id: "control", titleKey: "ui.control.architecture", icon: "codicon-settings-gear" },
    { id: "motion", titleKey: "ui.motion", icon: "codicon-play-circle" },
    { id: "tuning", titleKey: "ui.control.tuning", icon: "codicon-pulse" },
    { id: "analysis", titleKey: "ui.analysis", icon: "codicon-graph-line" },
  ];
  const toolPages: Array<{ id: Page; titleKey: MessageKey; icon: string }> = [
    { id: "parameters", titleKey: "ui.parameters", icon: "codicon-list-flat" },
    { id: "events", titleKey: "ui.events", icon: "codicon-warning" },
    { id: "automation", titleKey: "ui.automation", icon: "codicon-terminal" },
  ];
  function setError(error: unknown) { errorText = error instanceof Error ? error.message : String(error); }
  function persistConnectionSchemaPath(path: string) {
    try { localStorage.setItem(SCHEMA_PATH_STORAGE_KEY, path); } catch { /* The in-memory path remains usable. */ }
  }
  async function setConnection(next: ConnectionInfo | undefined) {
    const token = ++connectionGeneration;
    clearAutomationProjection();
    clearProblemsProjection();
    clearRuntimeTelemetry();
    problemsOpen = false;
    connection = next;
    motorActionNotice = "";
    connectionReady = false;
    connectionInitializing = !!next;
    errorText = ""; parameterSaveAvailable = false; saveFeedback = "idle";
    if (saveFeedbackTimer) clearTimeout(saveFeedbackTimer);
    saveFeedbackTimer = undefined;
    if (!next) {
      disconnectParameters();
      scopeSummary = { state: "STOPPED", selectedChannels: 0, lostFrames: 0 };
      return;
    }
    try {
      // Publish the device to business pages only after its canonical Parameter
      // projection has initialized. Keep the backend connection for safe recovery.
      await connectParameters();
      if (token !== connectionGeneration) return;
      connectionReady = true;
      const available = await canSaveParameters();
      if (token !== connectionGeneration) return;
      parameterSaveAvailable = available;
      await refreshRuntimeTelemetry();
      if (token === connectionGeneration) await refreshProblems();
    } catch (error) {
      if (token === connectionGeneration) {
        // A secondary status refresh failure does not undo a valid Parameter
        // projection. Only a failed initialization keeps the pages unavailable.
        if (!connectionReady) disconnectParameters();
        setError(error);
      }
    } finally {
      if (token === connectionGeneration) connectionInitializing = false;
    }
  }
  function pageTitle(page: Page): MessageKey { return [...workflowPages, ...toolPages].find((item) => item.id === page)?.titleKey ?? "ui.connection"; }
  function runtimePosition(totalTurns: number | null): string {
    if (totalTurns === null || !Number.isFinite(totalTurns)) return "—";
    let turn = Math.floor(totalTurns);
    let theta = (totalTurns - turn) * Math.PI * 2;
    if (theta >= Math.PI * 2 - 1e-6) { turn += 1; theta = 0; }
    if (Object.is(turn, -0)) turn = 0;
    return `${turn} turn + ${theta.toFixed(3)} rad`;
  }
  async function refreshAllParameters() {
    if (!connectionReady || readingParameters) return;
    readingParameters = true;
    try { await refreshParameters(); } catch (error) { setError(error); } finally { readingParameters = false; }
  }
  function showSavedFeedback() {
    saveFeedback = "saved";
    if (saveFeedbackTimer) clearTimeout(saveFeedbackTimer);
    saveFeedbackTimer = setTimeout(() => { saveFeedback = "idle"; saveFeedbackTimer = undefined; }, 1400);
  }
  async function toggleMotorEnable() {
    if (!connectionReady || motorState === null || globalActionBusy) return;
    globalActionBusy = true;
    const enabling = motorState === "DISABLED";
    motorActionNotice = enabling ? "Sending Enable…" : "Sending Disable…";
    try {
      if (enabling) await enableMotor(); else await disableMotor();
      motorActionNotice = enabling
        ? "Enable accepted; check device Motor State."
        : "Disable accepted; check device Motor State.";
    } catch (error) {
      motorActionNotice = `Motor command feedback: ${String(error)}`;
      setError(error);
    } finally { globalActionBusy = false; }
  }
  async function stopCurrentMotorOperation() {
    if (!connection || globalStopBusy) return;
    globalStopBusy = true;
    motorActionNotice = "Sending Stop…";
    try {
      await stopMotor();
      // Stop ACK does not certify that controlled deceleration has finished.
      motorActionNotice = "Stop accepted; monitor firmware Motor State.";
    } catch (error) {
      motorActionNotice = `Stop command feedback: ${String(error)}`;
      setError(error);
    } finally { globalStopBusy = false; }
  }
  async function savePersistentParameters() {
    if (!connectionReady || !parameterSaveAvailable || motorState !== "DISABLED" || globalActionBusy) return;
    globalActionBusy = true; saveFeedback = "idle";
    try { await saveParameterBaseline(saveParameters); showSavedFeedback(); }
    catch (error) { setError(error); } finally { globalActionBusy = false; }
  }
  onMount(() => {
    restoreUiLanguage();
    try {
      const storedPath = localStorage.getItem(SCHEMA_PATH_STORAGE_KEY);
      if (storedPath?.trim()) connectionSchemaPath = storedPath;
    } catch { /* Use the development default when storage is unavailable. */ }
    const stopStatus = pollParameters(POLLED_SYMBOLS, 500, setError);
    const stopRuntime = startRuntimeTelemetryTracking(() => !!readyConnection);
    const stopAutomation = startAutomationTracking(() => !!readyConnection);
    const stopProblems = startProblemsTracking(() => !!readyConnection);
    return () => { stopStatus(); stopRuntime(); stopAutomation(); stopProblems(); };
  });
  onDestroy(() => {
    ++connectionGeneration;
    if (saveFeedbackTimer) clearTimeout(saveFeedbackTimer);
    disconnectParameters();
    disconnectDevice().catch(() => undefined);
  });
</script>

<div class="workbench"><div class="body">
  <nav class="activity-bar" aria-label={translate("ui.control.architecture", $uiLanguage)}>
    {#each workflowPages as page}<button class:active={activePage === page.id} class="activity" title={translate(page.titleKey, $uiLanguage)} onclick={() => activePage = page.id}><i class={`codicon ${page.icon}`}></i></button>{/each}
    <div class="workflow-separator"></div>
    {#each toolPages as page}<button class:active={activePage === page.id} class="activity" title={translate(page.titleKey, $uiLanguage)} onclick={() => activePage = page.id}><i class={`codicon ${page.icon}`}></i></button>{/each}
    <div class="activity-spacer"></div>
  </nav>
  <div class="global-toolbar"><div class="global-toolbar-left">
    <div class="brand">NMIXX Motor Studio</div><div class="global-toolbar-divider" aria-hidden="true"></div>
    <div class="global-actions">
      <button class:motor-disabled={enableStatus === "DISABLED"} class:motor-enabled={enableStatus === "ENABLED"} class="tool-button global-action"
        disabled={!connectionReady || motorState === null || globalActionBusy} onclick={() => void toggleMotorEnable()}
        title={localize(enableStatus === "DISABLED" ? "Motor DISABLED — click to Enable" : enableStatus === "ENABLED" ? "Motor ENABLED — click to Disable" : "Motor enable state unknown", $uiLanguage)}>
        <i class="codicon codicon-power"></i> {enableStatus}
      </button>
      <button class="tool-button global-action stop-action" disabled={!connection || globalStopBusy} onclick={() => void stopCurrentMotorOperation()} title={translate("ui.stop.current.motor.operation", $uiLanguage)}>
        <i class={`codicon ${globalStopBusy ? "codicon-loading codicon-modifier-spin" : "codicon-debug-stop"}`}></i> {translate("ui.stop", $uiLanguage)}
      </button>
      <span class="global-group-gap"></span>
      <button class="tool-button global-action" disabled={!connectionReady || readingParameters} onclick={() => void refreshAllParameters()} title={translate("ui.read.current.ram.parameters.from.device", $uiLanguage)}><i class={`codicon ${readingParameters ? "codicon-loading codicon-modifier-spin" : "codicon-refresh"}`}></i> {translate("ui.read", $uiLanguage)}</button>
      <button class="tool-button global-action" disabled={!connectionReady || !parameterSaveAvailable || motorState !== "DISABLED" || globalActionBusy} onclick={() => void savePersistentParameters()}
        title={localize(!parameterSaveAvailable ? "Parameter persistence is not exposed by this firmware" : motorState !== "DISABLED" ? "Disable the motor before saving persistent parameters" : "Save persistent RAM parameters to device storage", $uiLanguage)}>
        <i class={`codicon ${saveFeedback === "saved" ? "codicon-check" : "codicon-save"}`}></i>{localize(saveFeedback === "saved" ? "Saved" : "Save", $uiLanguage)}
      </button>
    </div>
    {#if motorActionNotice}<span class="global-command-feedback" role="status" title={localizeFeedback(motorActionNotice, $uiLanguage)}>{localizeFeedback(motorActionNotice, $uiLanguage)} · {translate("ui.motor", $uiLanguage)} {motorState ?? "unknown"}</span>{/if}
  </div><div class="problems-host">
    <button class:problem-error={topProblem?.severity === "FAULT" || topProblem?.severity === "ERROR"}
      class:problem-warning={topProblem?.severity === "WARNING"} class:problem-info={topProblem?.severity === "INFO"}
      class="problems-indicator" disabled={!connectionReady}
      title={localize(connectionReady ? "Active problems" : "Connect and initialize a device to inspect problems", $uiLanguage)}
      onclick={() => problemsOpen = !problemsOpen}>
      <i class={`codicon ${topProblem?.severity === "FAULT" || topProblem?.severity === "ERROR" ? "codicon-error" : topProblem?.severity === "WARNING" ? "codicon-warning" : "codicon-info"}`}></i><span>{activeProblems.length}</span>
    </button>
    {#if problemsOpen && connectionReady}
      <div class="problems-popover">
        {#if topProblem}
          <button class="problem-summary" onclick={() => { activePage = "events"; problemsOpen = false; }}>
            <strong>{localize(topProblem.summary, $uiLanguage)}</strong>
            <span>{localizeFeedback(topProblem.description, $uiLanguage)}</span>
          </button>
        {:else}
          <div class="problem-summary empty">{translate("ui.no.active.problems", $uiLanguage)}</div>
        {/if}
      </div>
    {/if}
  </div></div>
  <main class="main-area">
    {#if activePage === "connection"}
      <ConnectionPage {connection} ready={connectionReady} initializing={connectionInitializing} onRetry={() => void setConnection(connection)} bind:port={connectionPort} bind:schemaPath={connectionSchemaPath} onSchemaPathChanged={persistConnectionSchemaPath} onConnected={(next) => void setConnection(next)} onDisconnected={() => void setConnection(undefined)} onError={setError} />
    {:else if activePage === "motor"}<div class="domain-page-container"><MotorPage connection={readyConnection} {motorState} onError={setError} /></div>
    {:else if activePage === "encoder"}<div class="domain-page-container"><EncoderPage connection={readyConnection} {motorState} onError={setError} /></div>
    {:else if activePage === "limits"}<div class="domain-page-container"><LimitsPage connection={readyConnection} {motorState} onError={setError} /></div>
    {:else if activePage === "control"}
      <div class="control-domain-shell"><aside class="control-navigation" aria-label={translate("ui.control.architecture", $uiLanguage)}>
        <button class="control-navigation-parent" onclick={() => controlArchitectureExpanded = !controlArchitectureExpanded} aria-expanded={controlArchitectureExpanded}><i class="codicon codicon-settings-gear"></i><span>{translate("ui.control.architecture", $uiLanguage)}</span><i class={`codicon ${controlArchitectureExpanded ? "codicon-chevron-down" : "codicon-chevron-right"} control-navigation-chevron`}></i></button>
        {#if controlArchitectureExpanded}<div class="control-navigation-children">
          <button class:active={activeControlLoop === "current"} onclick={() => activeControlLoop = "current"}>{translate("ui.current.loop", $uiLanguage)}</button>
          <button class:active={activeControlLoop === "speed"} onclick={() => activeControlLoop = "speed"}>{translate("ui.speed.loop", $uiLanguage)}</button>
          <button class:active={activeControlLoop === "position"} onclick={() => activeControlLoop = "position"}>{translate("ui.position.loop", $uiLanguage)}</button>
        </div>{/if}
      </aside><div class="control-domain-page"><ControlPage connection={readyConnection} {motorState} loop={activeControlLoop} onError={setError} /></div></div>
    {:else if activePage === "tuning"}<div class="domain-page-container"><ControlTuningPage connection={readyConnection} {motorState} onError={setError} /></div>
    {:else if activePage === "motion"}<div class="domain-page-container"><MotionPage capabilities={readyConnection?.motion} {motorState} onError={setError} /></div>
    {:else if activePage === "parameters"}<div class="domain-page-container"><ParameterTablePage connection={readyConnection} onError={setError} /></div>
    {:else if activePage === "events"}<div class="domain-page-container"><EventsPage connection={readyConnection} onNavigate={(page) => activePage = page} onError={setError} /></div>
    {:else if activePage === "automation"}<div class="domain-page-container"><AutomationHost connected={!!readyConnection} /></div>
    {:else if activePage !== "analysis"}
      <section class="page-toolbar"><div class="page-title">{translate(pageTitle(activePage), $uiLanguage).toUpperCase()}</div></section>
      <section class="placeholder-page"><div class="placeholder-title">{translate(pageTitle(activePage), $uiLanguage)}</div><div class="placeholder-copy">{translate("ui.this.workflow.page.is.reserved.for.the.corresponding.application.domain", $uiLanguage)}</div></section>
    {/if}
    <div class:inactive={activePage !== "analysis"} class="scope-page-container"><ScopePage connection={readyConnection} active={activePage === "analysis"} onSummary={(summary) => scopeSummary = summary} onError={setError} /></div>
    {#if errorText || $parameterState.error}<div class="error-text app-error">{localizeFeedback(errorText || $parameterState.error, $uiLanguage)}</div>{/if}
  </main>
</div>
<footer class="statusbar"><button class="language-toggle" aria-label={languageSwitchTitle($uiLanguage)}
  title={languageSwitchTitle($uiLanguage)}
  onclick={() => cycleUiLanguage($uiLanguage)}>{languageBadge($uiLanguage)}</button><div class="connection-status"><span class:connected={!!connection} class="status-dot"></span>{connection ? connectionReady ? "AxDr_L" : localize(connectionInitializing ? "Initializing…" : "Initialization incomplete", $uiLanguage) : translate("ui.disconnected", $uiLanguage)}</div><div class="status-spacer"></div>
  <div class="live-status"><span>{translate("ui.current", $uiLanguage)} {currentIq === null ? "—" : `${currentIq.toFixed(3)} A`}</span><span>{translate("ui.speed", $uiLanguage)} {speedWm === null ? "—" : `${speedWm.toFixed(3)} rad/s`}</span><span>{translate("ui.position", $uiLanguage)} {positionText}</span><span>Vbus {vbus === null ? "—" : `${vbus.toFixed(2)} V`}</span></div>
</footer></div>

<style>
  .language-toggle { flex: 0 0 auto; min-width: 26px; height: 19px; padding: 0 5px; border: 1px solid var(--vscode-panel-border); border-radius: 3px; background: transparent; color: var(--vscode-foreground); font: inherit; font-size: 10px; cursor: pointer; }
  .language-toggle:hover { background: var(--vscode-list-hoverBackground); }
  .language-toggle:focus-visible { outline: 1px solid var(--vscode-focusBorder); outline-offset: 1px; }
  .global-toolbar { grid-column: 1 / -1; grid-row: 1; min-width: 0; display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 0 10px 0 14px; background: #181818; border-bottom: 1px solid var(--vscode-panel-border, #2b2b2b); }
  .global-toolbar-left { min-width: 0; display: flex; align-items: center; gap: 12px; }
  .global-toolbar-divider { width: 1px; height: 22px; flex: 0 0 auto; background: var(--vscode-panel-border, #303030); }
  .global-actions { display: flex; align-items: center; gap: 5px; min-width: 0; }
  .global-group-gap { width: 7px; }
  .global-command-feedback { max-width: 300px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--vscode-descriptionForeground); font-size: 11px; }
  .global-action { min-width: 72px; }
  /* This button reports device state, including while an Action is pending. */
  .motor-disabled { color: #f85149; }
  .motor-enabled { color: #3fb950; }
  .stop-action:not(:disabled) { font-weight: 600; }
  .problems-host { position: relative; flex: 0 0 auto; }
  .problems-indicator { display: inline-flex; align-items: center; gap: 5px; border: 0; background: transparent; color: inherit; padding: 4px 7px; }
  .problems-indicator:not(:disabled) { cursor: pointer; }
  .problems-indicator.problem-error { color: var(--vscode-errorForeground); }
  .problems-indicator.problem-warning { color: var(--vscode-inputValidation-warningBorder, #cca700); }
  .problems-indicator.problem-info { color: var(--vscode-descriptionForeground); }
  .problems-popover { position: absolute; top: calc(100% + 6px); right: 0; z-index: 30; width: 320px; padding: 6px; border: 1px solid var(--vscode-panel-border); border-radius: 3px; background: var(--vscode-menu-background, #252526); box-shadow: 0 8px 24px rgba(0,0,0,.35); }
  .problem-summary { width: 100%; display: grid; gap: 5px; padding: 9px 10px; border: 0; text-align: left; color: var(--vscode-menu-foreground, var(--vscode-foreground)); background: transparent; }
  button.problem-summary { cursor: pointer; }
  button.problem-summary:hover { background: var(--vscode-list-hoverBackground); }
  .problem-summary strong { font-size: 12px; }
  .problem-summary span, .problem-summary.empty { color: var(--vscode-descriptionForeground); font-size: 11px; line-height: 1.4; }
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
