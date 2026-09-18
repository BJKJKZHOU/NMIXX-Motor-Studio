<script lang="ts">
  import { onDestroy } from "svelte";
  import ConnectionPage from "./connection/ConnectionPage.svelte";
  import { disconnectDevice } from "./connection/api";
  import type { ConnectionInfo } from "./connection/types";
  import ScopePage from "./analysis/scope/ScopePage.svelte";
  import type { ScopeSummary } from "./analysis/scope/types";
  import ParameterTablePage from "./parameters/ParameterTablePage.svelte";
  import MotionPage from "./motion/MotionPage.svelte";

  type Page = "connection" | "motor" | "encoder" | "limits" | "control" | "motion" | "analysis" | "parameters" | "events" | "automation";

  let activePage: Page = "connection";
  let connection: ConnectionInfo | undefined;
  let errorText = "";
  let scopeSummary: ScopeSummary = { state: "STOPPED", selectedChannels: 0, lostFrames: 0 };

  const workflowPages: Array<{ id: Page; title: string; icon: string }> = [
    { id: "connection", title: "Connection", icon: "codicon-plug" },
    { id: "motor", title: "Motor", icon: "codicon-circuit-board" },
    { id: "encoder", title: "Encoder", icon: "codicon-record" },
    { id: "limits", title: "Limits / Safety", icon: "codicon-shield" },
    { id: "control", title: "Control", icon: "codicon-settings-gear" },
    { id: "motion", title: "Motion", icon: "codicon-play-circle" },
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

  function setConnection(next: ConnectionInfo | undefined) {
    connection = next;
    errorText = "";
    if (!next) scopeSummary = { state: "STOPPED", selectedChannels: 0, lostFrames: 0 };
  }

  function pageTitle(page: Page): string {
    return [...workflowPages, ...toolPages].find((item) => item.id === page)?.title ?? page;
  }

  onDestroy(() => {
    disconnectDevice().catch(() => undefined);
  });
</script>

<div class="workbench">
  <header class="titlebar">
    <div class="brand">NMIXX Motor Studio</div>
    <div class="device-summary"><span class:connected={!!connection} class="status-dot"></span>{connection ? `${connection.port} · Connected` : "Disconnected"}</div>
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

    <main class="main-area">
      {#if activePage === "connection"}
        <ConnectionPage {connection} onConnected={(next) => setConnection(next)} onDisconnected={() => setConnection(undefined)} onError={setError} />
      {:else if activePage === "motion"}
        <MotionPage />
      {:else if activePage === "parameters"}
        <div class="parameter-page-container">
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

  <footer class="statusbar">
    <div><i class="codicon codicon-plug"></i> {connection?.port ?? "No device"}</div>
    <div>{scopeSummary.state}</div>
    <div>{connection ? `FAST ${connection.fastRateHz / 1000} kHz` : "FAST —"}</div>
    <div>{scopeSummary.selectedChannels} / {connection?.fastMaxChannels ?? "—"} ch</div>
    <div>loss {scopeSummary.lostFrames}</div>
    <div class="status-spacer"></div>
    <div>{connection ? "AxDr_L" : "NMIXX"}</div>
  </footer>
</div>

<style>
  .parameter-page-container {
    grid-row: 1 / -1;
    min-width: 0;
    min-height: 0;
    display: grid;
  }
</style>
