<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import Split from "split.js";
  import uPlot from "uplot";

  type PlotChannel = {
    id: number;
    symbol: string;
    unit?: string;
    supportsFast: boolean;
    supportsNormal: boolean;
    fastScale?: number;
  };

  type ConnectionInfo = {
    port: string;
    fastMaxChannels: number;
    normalMaxChannels: number;
    fastBlockSamples: number;
    fastRateHz: number;
    normalRateHz: number;
    channels: PlotChannel[];
  };

  type ScopeChannel = { id: number; symbol: string; unit?: string };
  type ScopeConfig = {
    sampleRateHz: number;
    historySeconds: number;
    channels: ScopeChannel[];
  };

  type ScopeSnapshot = {
    sampleRateHz: number;
    sampleCount: number;
    lostFrames: number;
    state: string;
    times: number[];
    series: number[][];
  };

  let plotHost: HTMLDivElement;
  let plot: uPlot | undefined;
  let split: Split.Instance | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let refreshTimer: ReturnType<typeof setInterval> | undefined;
  let snapshotBusy = false;

  let ports: string[] = [];
  let port = "/dev/ttyACM1";
  let schemaPath = "../../../AxDr_L_Motor/build/host/axdr-host-schema.toml";
  let connection: ConnectionInfo | undefined;
  let scopeConfig: ScopeConfig | undefined;
  let selectedIds = new Set<number>();
  let snapshot: ScopeSnapshot | undefined;
  let busy = false;
  let errorText = "";

  const traceColors = [
    "#7aa2c8",
    "#c8b77a",
    "#9b8ac8",
    "#7fa68a",
    "#c28b73",
    "#aa829a",
    "#79a6ad",
    "#91a77b",
  ];

  function channelMode(channel: PlotChannel): string {
    if (channel.supportsFast && channel.supportsNormal) return "FAST+N";
    if (channel.supportsFast) return "FAST";
    return "NORMAL";
  }

  function setError(error: unknown) {
    errorText = error instanceof Error ? error.message : String(error);
  }

  async function refreshPorts() {
    try {
      ports = await invoke<string[]>("device_list");
      if (ports.length > 0 && !ports.includes(port)) port = ports[0];
    } catch (error) {
      setError(error);
    }
  }

  async function connect() {
    busy = true;
    errorText = "";
    try {
      connection = await invoke<ConnectionInfo>("device_connect", {
        port,
        schemaPath,
        baud: 115200,
      });
      scopeConfig = undefined;
      snapshot = undefined;
      const defaults = connection.channels.filter((channel) => channel.supportsFast).slice(0, 2);
      selectedIds = new Set(defaults.map((channel) => channel.id));
      rebuildPlot([]);
    } catch (error) {
      connection = undefined;
      scopeConfig = undefined;
      setError(error);
    } finally {
      busy = false;
    }
  }

  async function disconnect() {
    try {
      await invoke("device_disconnect");
    } catch (error) {
      setError(error);
    }
    connection = undefined;
    scopeConfig = undefined;
    snapshot = undefined;
    selectedIds = new Set();
    rebuildPlot([]);
  }

  function toggleChannel(id: number) {
    if (!connection) return;
    const next = new Set(selectedIds);
    if (next.has(id)) {
      next.delete(id);
    } else {
      if (next.size >= connection.fastMaxChannels) return;
      next.add(id);
    }
    selectedIds = next;
    scopeConfig = undefined;
    snapshot = undefined;
  }

  async function configureScope(): Promise<boolean> {
    if (scopeConfig) return true;
    if (selectedIds.size === 0) {
      errorText = "Select at least one FAST channel.";
      return false;
    }
    try {
      scopeConfig = await invoke<ScopeConfig>("scope_configure", {
        parameterIds: Array.from(selectedIds),
        historySeconds: 10.0,
      });
      rebuildPlot(scopeConfig.channels);
      return true;
    } catch (error) {
      setError(error);
      return false;
    }
  }

  async function runScope() {
    errorText = "";
    if (!(await configureScope())) return;
    try {
      await invoke("scope_live");
      await refreshSnapshot();
    } catch (error) {
      setError(error);
    }
  }

  async function pauseScope() {
    if (!scopeConfig) return;
    try {
      await invoke("scope_pause");
      await refreshSnapshot();
    } catch (error) {
      setError(error);
    }
  }

  async function clearScope() {
    if (!scopeConfig) return;
    try {
      await invoke("scope_clear");
      await refreshSnapshot();
    } catch (error) {
      setError(error);
    }
  }

  async function refreshSnapshot() {
    if (!scopeConfig || snapshotBusy) return;
    snapshotBusy = true;
    try {
      snapshot = await invoke<ScopeSnapshot>("scope_snapshot", {
        windowSeconds: 0.5,
        maxPoints: 2500,
      });
      const data: uPlot.AlignedData = [snapshot.times, ...snapshot.series];
      plot?.setData(data);
    } catch (error) {
      setError(error);
    } finally {
      snapshotBusy = false;
    }
  }

  function rebuildPlot(channels: ScopeChannel[]) {
    if (!plotHost) return;
    const rect = plotHost.getBoundingClientRect();
    plot?.destroy();
    const series: uPlot.Series[] = [
      {},
      ...channels.map((channel, index) => ({
        label: channel.symbol,
        stroke: traceColors[index % traceColors.length],
        width: 1.25,
      })),
    ];
    const empty: uPlot.AlignedData = [[], ...channels.map(() => [])];
    plot = new uPlot(
      {
        width: Math.max(420, Math.floor(rect.width)),
        height: Math.max(260, Math.floor(rect.height)),
        legend: { show: false },
        cursor: { drag: { x: true, y: false } },
        scales: { x: { time: false } },
        axes: [
          { stroke: "#8c8c8c", grid: { stroke: "#2a2d2e", width: 1 }, ticks: { stroke: "#3a3d41" } },
          { stroke: "#8c8c8c", grid: { stroke: "#2a2d2e", width: 1 }, ticks: { stroke: "#3a3d41" } },
        ],
        series,
      },
      empty,
      plotHost,
    );
  }

  function resizePlot() {
    if (!plot || !plotHost) return;
    const rect = plotHost.getBoundingClientRect();
    plot.setSize({
      width: Math.max(420, Math.floor(rect.width)),
      height: Math.max(260, Math.floor(rect.height)),
    });
  }

  function latestValue(index: number): string {
    if (!snapshot || !scopeConfig || snapshot.series[index]?.length === 0) return "—";
    const values = snapshot.series[index];
    const value = values[values.length - 1];
    const unit = scopeConfig.channels[index]?.unit ?? "";
    return `${value.toFixed(3)}${unit ? ` ${unit}` : ""}`;
  }

  onMount(() => {
    split = Split(["#scope-sidebar", "#scope-workspace"], {
      sizes: [25, 75],
      minSize: [260, 420],
      gutterSize: 4,
      snapOffset: 0,
      onDrag: resizePlot,
    });

    rebuildPlot([]);
    resizeObserver = new ResizeObserver(resizePlot);
    resizeObserver.observe(plotHost);
    refreshTimer = setInterval(refreshSnapshot, 50);
    refreshPorts();

    return () => {
      if (refreshTimer) clearInterval(refreshTimer);
      resizeObserver?.disconnect();
      plot?.destroy();
      split?.destroy();
      invoke("device_disconnect").catch(() => undefined);
    };
  });
</script>

<div class="workbench">
  <header class="titlebar">
    <div class="brand">NMIXX Motor Studio</div>
    <div class="device-summary">
      <span class:connected={!!connection} class="status-dot"></span>
      {connection ? `${connection.port} · Connected` : "Disconnected"}
    </div>
  </header>

  <div class="body">
    <nav class="activity-bar" aria-label="Primary">
      <button class="activity active" title="Scope"><i class="codicon codicon-graph-line"></i></button>
      <button class="activity" title="Control"><i class="codicon codicon-dashboard"></i></button>
      <button class="activity" title="Parameters"><i class="codicon codicon-settings-gear"></i></button>
      <button class="activity" title="Events"><i class="codicon codicon-warning"></i></button>
      <div class="activity-spacer"></div>
      <button class="activity" title="Connection"><i class="codicon codicon-plug"></i></button>
    </nav>

    <main class="main-area">
      <section class="page-toolbar">
        <div class="page-title">SCOPE</div>
        <div class="toolbar-actions">
          <vscode-button disabled={!connection || selectedIds.size === 0} onclick={runScope}><i class="codicon codicon-play"></i>&nbsp;Run</vscode-button>
          <vscode-button secondary disabled={!scopeConfig} onclick={pauseScope}><i class="codicon codicon-debug-pause"></i>&nbsp;Pause</vscode-button>
          <vscode-button secondary disabled={!scopeConfig} onclick={clearScope}>Clear</vscode-button>
        </div>
      </section>

      <div class="scope-shell">
        <aside id="scope-sidebar" class="scope-sidebar">
          <section class="side-section connection-section">
            <div class="section-heading">CONNECTION</div>
            <div class="connection-form">
              <label>Port</label>
              <div class="field-row">
                <input bind:value={port} class="compact-input" list="device-ports" />
                <datalist id="device-ports">{#each ports as item}<option value={item}></option>{/each}</datalist>
                <vscode-button secondary onclick={refreshPorts} title="Refresh ports"><i class="codicon codicon-refresh"></i></vscode-button>
              </div>
              <label>HostSchema</label>
              <input bind:value={schemaPath} class="compact-input mono" />
              <div class="connection-actions">
                {#if connection}
                  <vscode-button secondary onclick={disconnect}>Disconnect</vscode-button>
                {:else}
                  <vscode-button disabled={busy} onclick={connect}>Connect</vscode-button>
                {/if}
              </div>
              {#if errorText}<div class="error-text">{errorText}</div>{/if}
            </div>
          </section>

          <section class="side-section">
            <div class="section-heading">CHANNELS</div>
            <div class="channel-list">
              {#if connection}
                {#each connection.channels as channel}
                  <label class:disabled-row={!channel.supportsFast} class="channel-row" onclick={() => channel.supportsFast && toggleChannel(channel.id)}>
                    <vscode-checkbox checked={selectedIds.has(channel.id) || undefined} disabled={!channel.supportsFast}></vscode-checkbox>
                    <span class="channel-name">{channel.symbol}</span>
                    <span class="channel-unit">{channel.unit ?? ""}</span>
                    <span class:normal-only={!channel.supportsFast} class="channel-mode">{channelMode(channel)}</span>
                  </label>
                {/each}
              {:else}
                <div class="empty-hint">Connect a device to discover Plot channels.</div>
              {/if}
            </div>
          </section>

          <section class="side-section acquisition">
            <div class="section-heading">ACQUISITION</div>
            <div class="property-grid">
              <span>State</span><strong>{snapshot?.state ?? "STOPPED"}</strong>
              <span>FAST Rate</span><strong>{connection ? `${(connection.fastRateHz / 1000).toFixed(1)} kHz` : "—"}</strong>
              <span>History</span><strong>{scopeConfig ? `${scopeConfig.historySeconds.toFixed(3)} s` : "10.000 s"}</strong>
              <span>Channels</span><strong>{selectedIds.size} / {connection?.fastMaxChannels ?? "—"}</strong>
              <span>Block</span><strong>{connection?.fastBlockSamples ?? "—"}</strong>
            </div>
          </section>
        </aside>

        <section id="scope-workspace" class="scope-workspace">
          <div class="editor-tabs">
            <div class="editor-tab active"><i class="codicon codicon-graph-line"></i> Scope</div>
          </div>
          <div class="plot-header">
            {#if scopeConfig}
              {#each scopeConfig.channels as channel, index}
                <div class="trace-key">
                  <span class="trace-mark" style={`background:${traceColors[index % traceColors.length]}`}></span>
                  {channel.symbol}
                  <span class="value">{latestValue(index)}</span>
                </div>
              {/each}
            {:else}
              <div class="plot-placeholder">Select FAST channels and press Run.</div>
            {/if}
            <div class="plot-meta">{snapshot?.sampleCount ?? 0} samples · loss {snapshot?.lostFrames ?? 0}</div>
          </div>
          <div bind:this={plotHost} class="plot-host"></div>
        </section>
      </div>
    </main>
  </div>

  <footer class="statusbar">
    <div><i class="codicon codicon-plug"></i> {connection?.port ?? "No device"}</div>
    <div>{snapshot?.state ?? "STOPPED"}</div>
    <div>{connection ? `FAST ${connection.fastRateHz / 1000} kHz` : "FAST —"}</div>
    <div>{selectedIds.size} / {connection?.fastMaxChannels ?? "—"} ch</div>
    <div>loss {snapshot?.lostFrames ?? 0}</div>
    <div class="status-spacer"></div>
    <div>{connection ? "AxDr_L" : "NMIXX"}</div>
  </footer>
</div>
