<script lang="ts">
  import { onDestroy } from "svelte";
  import { disconnectDevice } from "./connection/api";
  import type { ConnectionInfo } from "./connection/types";
  import ScopePage from "./analysis/scope/ScopePage.svelte";
  import type { ScopeSummary } from "./analysis/scope/types";

  let connection: ConnectionInfo | undefined;
  let errorText = "";
  let scopeSummary: ScopeSummary = { state: "STOPPED", selectedChannels: 0, lostFrames: 0 };

  function setError(error: unknown) {
    errorText = error instanceof Error ? error.message : String(error);
  }

  function setConnection(next: ConnectionInfo | undefined) {
    connection = next;
    errorText = "";
    if (!next) scopeSummary = { state: "STOPPED", selectedChannels: 0, lostFrames: 0 };
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
    <nav class="activity-bar" aria-label="Primary">
      <button class="activity active" title="Analysis / Scope"><i class="codicon codicon-graph-line"></i></button>
      <button class="activity" title="Control"><i class="codicon codicon-dashboard"></i></button>
      <button class="activity" title="Parameters"><i class="codicon codicon-settings-gear"></i></button>
      <button class="activity" title="Events"><i class="codicon codicon-warning"></i></button>
      <div class="activity-spacer"></div><button class="activity" title="Connection"><i class="codicon codicon-plug"></i></button>
    </nav>
    <main class="main-area">
      <ScopePage {connection} onConnection={setConnection} onSummary={(summary) => scopeSummary = summary} onError={setError} />
      {#if errorText}<div class="error-text app-error">{errorText}</div>{/if}
    </main>
  </div>

  <footer class="statusbar">
    <div><i class="codicon codicon-plug"></i> {connection?.port ?? "No device"}</div><div>{scopeSummary.state}</div>
    <div>{connection ? `FAST ${connection.fastRateHz / 1000} kHz` : "FAST —"}</div><div>{scopeSummary.selectedChannels} / {connection?.fastMaxChannels ?? "—"} ch</div>
    <div>loss {scopeSummary.lostFrames}</div><div class="status-spacer"></div><div>{connection ? "AxDr_L" : "NMIXX"}</div>
  </footer>
</div>
