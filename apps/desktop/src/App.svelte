<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import ConnectionPage from "./connection/ConnectionPage.svelte";
  import { disconnectDevice } from "./connection/api";
  import type { ConnectionInfo } from "./connection/types";
  import ScopePage from "./analysis/scope/ScopePage.svelte";
  import type { ScopeSummary } from "./analysis/scope/types";
  import ParameterTablePage from "./parameters/ParameterTablePage.svelte";
  import MotorPage from "./motor/MotorPage.svelte";
  import EncoderPage from "./encoder/EncoderPage.svelte";
  import LimitsPage from "./limits/LimitsPage.svelte";
  import { disableMotor, enableMotor, onActionCompleted, stopMotor } from "./actions/api";
  import { listParameters, readParameters } from "./parameters/api";
  import type { ParameterMetadata, ParameterValue } from "./parameters/types";

  type Page = "connection" | "motor" | "encoder" | "limits" | "control" | "analysis" | "parameters" | "events" | "automation";

  const GLOBAL_SYMBOLS = ["PARAM_MOTOR_STATE", "PARAM_RUN_IQ", "PARAM_RUN_WM", "PARAM_RUN_POSITION"] as const;
  const MOTOR_DISABLED = 0;
  const MOTOR_ENABLED = 1;
  const MOTOR_RUN = 2;

  let activePage: Page = "connection";
  let connection: ConnectionInfo | undefined;
  let errorText = "";
  let scopeSummary: ScopeSummary = { state: "STOPPED", selectedChannels: 0, lostFrames: 0 };
  let parameterRegistry: ParameterMetadata[] = [];
  let globalIds: Record<string, number> = {};
  let motorState: number | null = null;
  let currentIq: number | null = null;
  let speedWm: number | null = null;
  let positionText = "—";
  let motorActionBusy = false;
  let readingParameters = false;
  let globalRefreshTimer: ReturnType<typeof setInterval> | undefined;
  let stopActionListener: (() => void) | undefined;

  const workflowPages: Array<{ id: Page; title: string; icon: string }> = [
    { id: "connection", title: "Connection", icon: "codicon-plug" },
    { id: "motor", title: "Motor", icon: "codicon-circuit-board" },
    { id: "encoder", title: "Encoder", icon: "codicon-record" },
    { id: "limits", title: "Limits / Safety", icon: "codicon-shield" },
    { id: "control", title: "Control", icon: "codicon-settings-gear" },
    { id: "analysis", title: "Analysis", icon: "codicon-graph-line" },
  ];

  const toolPages: Array<{ id: Page; title: string; icon: string }> = [
    { id: "parameters", title: "Parameters", icon: "codicon-list-flat" },
    { id: "events", title: "Events", icon: "codicon-warning" },
    { id: "automation", title: "Automation", icon: "codicon-terminal" },
  ];

  function setError(error: unknown) {
    errorText = error instanceof Error ? error.message : String(error);
  }

  async function setConnection(next: ConnectionInfo | undefined) {
    connection = next;
    errorText = "";
    clearGlobalStatus();
    if (!next) {
      scopeSummary = { state: "STOPPED", selectedChannels: 0, lostFrames: 0 };
      return;
    }

    try {
      parameterRegistry = await listParameters();
      globalIds = Object.fromEntries(
        parameterRegistry
          .filter((item) => GLOBAL_SYMBOLS.includes(item.symbol as typeof GLOBAL_SYMBOLS[number]))
          .map((item) => [item.symbol, item.id]),
      );
      await refreshAllParameters();
      globalRefreshTimer = setInterval(() => void refreshGlobalStatus(), 500);
    } catch (error) {
      setError(error);
    }
  }

  function clearGlobalStatus() {
    if (globalRefreshTimer) clearInterval(globalRefreshTimer);
    globalRefreshTimer = undefined;
    parameterRegistry = [];
    globalIds = {};
    motorState = null;
    currentIq = null;
    speedWm = null;
    positionText = "—";
    motorActionBusy = false;
    readingParameters = false;
  }

  function pageTitle(page: Page): string {
    return [...workflowPages, ...toolPages].find((item) => item.id === page)?.title ?? page;
  }

  function numeric(value: ParameterValue | null): number | null {
    if (!value || value.type === "position") return null;
    return Number(value.value);
  }

  function position(value: ParameterValue | null): string {
    if (!value || value.type !== "position") return "—";
    return `${value.value.turns} turn + ${Number(value.value.theta).toFixed(3)} rad`;
  }

  async function refreshGlobalStatus() {
    if (!connection) return;
    const symbols = GLOBAL_SYMBOLS.filter((symbol) => globalIds[symbol] !== undefined);
    if (symbols.length === 0) return;
    try {
      const results = await readParameters(symbols.map((symbol) => globalIds[symbol]));
      const values = new Map(results.map((result) => [result.id, result.value]));
      motorState = numeric(values.get(globalIds.PARAM_MOTOR_STATE) ?? null);
      currentIq = numeric(values.get(globalIds.PARAM_RUN_IQ) ?? null);
      speedWm = numeric(values.get(globalIds.PARAM_RUN_WM) ?? null);
      positionText = position(values.get(globalIds.PARAM_RUN_POSITION) ?? null);
    } catch (error) {
      setError(error);
    }
  }

  async function refreshAllParameters() {
    if (!connection || readingParameters) return;
    readingParameters = true;
    try {
      const readable = parameterRegistry.filter((item) => item.access.toLowerCase().includes("r"));
      if (readable.length > 0) await readParameters(readable.map((item) => item.id));
      await refreshGlobalStatus();
      window.dispatchEvent(new CustomEvent("nmixx-parameters-refreshed"));
    } catch (error) {
      setError(error);
    } finally {
      readingParameters = false;
    }
  }

  async function toggleMotorEnable() {
    if (!connection || motorActionBusy || motorState === null) return;
    motorActionBusy = true;
    try {
      if (motorState === MOTOR_DISABLED) await enableMotor();
      else await disableMotor();
    } catch (error) {
      motorActionBusy = false;
      setError(error);
    }
  }

  async function stopCurrentMotorOperation() {
    if (!connection || motorActionBusy || motorState !== MOTOR_RUN) return;
    motorActionBusy = true;
    try {
      await stopMotor();
    } catch (error) {
      motorActionBusy = false;
      setError(error);
    }
  }

  onMount(() => {
    let disposed = false;
    onActionCompleted(() => {
      motorActionBusy = false;
      void refreshGlobalStatus();
    }).then((unlisten) => {
      if (disposed) unlisten();
      else stopActionListener = unlisten;
    }).catch(setError);

    return () => {
      disposed = true;
      stopActionListener?.();
      stopActionListener = undefined;
    };
  });

  onDestroy(() => {
    clearGlobalStatus();
    disconnectDevice().catch(() => undefined);
  });
</script>

<div class="workbench">
  <header class="titlebar">
    <div class="brand">NMIXX Motor Studio</div>
  </header>

  <div class="body">
    <nav class="activity-bar" aria-label="Commissioning workflow">
      {#each workflowPages as page}
        <button class:active={activePage === page.id} class="activity" title={page.title} onclick={() => activePage = page.id}>
          <i class={`codicon ${page.icon}`}></i>
        </button>
      {/each}
      <div class="workflow-separator"></div>
      {#each toolPages as page}
        <button class:active={activePage === page.id} class="activity" title={page.title} onclick={() => activePage = page.id}>
          <i class={`codicon ${page.icon}`}></i>
        </button>
      {/each}
      <div class="activity-spacer"></div>
    </nav>

    <div class="workspace">
      <div class="global-toolbar">
        <div class="global-actions">
          <button
            class:enable-action={motorState === MOTOR_DISABLED}
            class:disable-action={motorState !== null && motorState !== MOTOR_DISABLED}
            class="tool-button global-action"
            disabled={!connection || motorState === null || motorActionBusy}
            onclick={() => void toggleMotorEnable()}
            title={motorState === MOTOR_DISABLED ? "Enable motor" : "Disable motor"}
          >
            <i class={`codicon ${motorState === MOTOR_DISABLED ? "codicon-play" : "codicon-debug-disconnect"}`}></i>
            {motorState === MOTOR_DISABLED ? "Enable" : "Disable"}
          </button>
          <button
            class="tool-button global-action stop-action"
            disabled={!connection || motorState !== MOTOR_RUN || motorActionBusy}
            onclick={() => void stopCurrentMotorOperation()}
            title="Stop current motor operation"
          >
            <i class="codicon codicon-debug-stop"></i> Stop
          </button>
          <span class="global-group-gap"></span>
          <button class="tool-button global-action" disabled={!connection || readingParameters} onclick={() => void refreshAllParameters()} title="Read current RAM parameters from device">
            <i class={`codicon ${readingParameters ? "codicon-loading codicon-modifier-spin" : "codicon-refresh"}`}></i> Read
          </button>
          <button class="tool-button global-action" disabled title="Persistent configuration save is not exposed by this firmware yet">
            <i class="codicon codicon-save"></i> Save
          </button>
        </div>
        <button class="problems-indicator" disabled title="Problems service is not implemented yet">
          <i class="codicon codicon-warning"></i><span>0</span>
        </button>
      </div>

      <main class="main-area">
        {#if activePage === "connection"}
          <ConnectionPage {connection} onConnected={(next) => void setConnection(next)} onDisconnected={() => void setConnection(undefined)} onError={setError} />
        {:else if activePage === "motor"}
          <div class="domain-page-container">
            <MotorPage {connection} onError={setError} />
          </div>
        {:else if activePage === "encoder"}
          <div class="domain-page-container">
            <EncoderPage {connection} onError={setError} />
          </div>
        {:else if activePage === "limits"}
          <div class="domain-page-container">
            <LimitsPage {connection} onError={setError} />
          </div>
        {:else if activePage === "parameters"}
          <div class="domain-page-container">
            <ParameterTablePage {connection} onError={setError} />
          </div>
        {:else if activePage !== "analysis"}
          <section class="page-toolbar"><div class="page-title">{pageTitle(activePage).toUpperCase()}</div></section>
          <section class="placeholder-page">
            <div class="placeholder-title">{pageTitle(activePage)}</div>
            <div class="placeholder-copy">This workflow page is reserved for the corresponding application domain. Device behavior will be added through the shared Application API rather than implemented in the shell.</div>
          </section>
        {/if}
        <div class:inactive={activePage !== "analysis"} class="scope-page-container">
          <ScopePage {connection} active={activePage === "analysis"} onSummary={(summary) => scopeSummary = summary} onError={setError} />
        </div>
        {#if errorText}<div class="error-text app-error">{errorText}</div>{/if}
      </main>
    </div>
  </div>

  <footer class="statusbar">
    <div class="connection-status"><span class:connected={!!connection} class="status-dot"></span>{connection ? "AxDr_L" : "Disconnected"}</div>
    <div class="status-spacer"></div>
    <div class="live-status"><span>Current {currentIq === null ? "—" : `${currentIq.toFixed(3)} A`}</span><span>Speed {speedWm === null ? "—" : `${speedWm.toFixed(3)} rad/s`}</span><span>Position {positionText}</span></div>
  </footer>
</div>

<style>
  .workspace {
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-rows: 34px minmax(0, 1fr);
  }

  .global-toolbar {
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 0 10px;
    border-bottom: 1px solid var(--vscode-panel-border, #2b2b2b);
  }

  .global-actions {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
  }

  .global-group-gap {
    width: 7px;
  }

  .global-action {
    min-width: 72px;
  }

  .enable-action:not(:disabled) {
    color: #3fb950;
  }

  .disable-action:not(:disabled) {
    color: #f85149;
  }

  .stop-action:not(:disabled) {
    font-weight: 600;
  }

  .problems-indicator {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 0;
    background: transparent;
    color: inherit;
    padding: 4px 7px;
  }

  .domain-page-container {
    grid-row: 1 / -1;
    min-width: 0;
    min-height: 0;
    display: grid;
  }

  .connection-status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .live-status {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 18px;
    white-space: nowrap;
    text-align: right;
  }
</style>
