<script lang="ts">
  import { onMount } from "svelte";
  import Split from "split.js";
  import uPlot from "uplot";

  let plotHost: HTMLDivElement;
  let plot: uPlot | undefined;

  const channels = [
    { name: "PARAM_ADC_IA", unit: "A", mode: "FAST", checked: true },
    { name: "PARAM_RUN_IQ", unit: "A", mode: "FAST", checked: true },
    { name: "PARAM_ADC_IB", unit: "A", mode: "FAST", checked: false },
    { name: "PARAM_ADC_VBUS", unit: "V", mode: "NORMAL", checked: false },
    { name: "PARAM_RUN_THETA_E", unit: "rad", mode: "FAST", checked: false },
  ];

  function buildMockData(): uPlot.AlignedData {
    const x: number[] = [];
    const ia: number[] = [];
    const iq: number[] = [];
    for (let i = 0; i <= 2000; i += 1) {
      const t = i / 200;
      x.push(t);
      ia.push(0.72 * Math.sin(t * 18.3) + 0.08 * Math.sin(t * 73));
      iq.push(0.52 + 0.22 * Math.sin(t * 5.2 + 0.8));
    }
    return [x, ia, iq];
  }

  onMount(() => {
    const split = Split(["#scope-sidebar", "#scope-workspace"], {
      sizes: [23, 77],
      minSize: [210, 420],
      gutterSize: 4,
      snapOffset: 0,
    });

    const makePlot = () => {
      const rect = plotHost.getBoundingClientRect();
      plot?.destroy();
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
          series: [
            {},
            { label: "Ia", stroke: "#7aa2c8", width: 1.4 },
            { label: "Iq", stroke: "#c8b77a", width: 1.4 },
          ],
        },
        buildMockData(),
        plotHost,
      );
    };

    makePlot();
    const observer = new ResizeObserver(makePlot);
    observer.observe(plotHost);

    return () => {
      observer.disconnect();
      plot?.destroy();
      split.destroy();
    };
  });
</script>

<div class="workbench">
  <header class="titlebar">
    <div class="brand">NMIXX Motor Studio</div>
    <div class="device-summary"><span class="status-dot"></span> AxDr_L · /dev/ttyACM1</div>
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
          <vscode-button><i class="codicon codicon-play"></i>&nbsp;Run</vscode-button>
          <vscode-button secondary><i class="codicon codicon-debug-pause"></i>&nbsp;Pause</vscode-button>
          <vscode-button secondary>Capture</vscode-button>
          <vscode-button secondary>Clear</vscode-button>
        </div>
      </section>

      <div class="scope-shell">
        <aside id="scope-sidebar" class="scope-sidebar">
          <section class="side-section">
            <div class="section-heading">CHANNELS</div>
            <div class="channel-list">
              {#each channels as channel}
                <label class="channel-row">
                  <vscode-checkbox checked={channel.checked || undefined}></vscode-checkbox>
                  <span class="channel-name">{channel.name}</span>
                  <span class="channel-unit">{channel.unit}</span>
                  <span class:normal-only={channel.mode === "NORMAL"} class="channel-mode">{channel.mode}</span>
                </label>
              {/each}
            </div>
          </section>

          <section class="side-section acquisition">
            <div class="section-heading">ACQUISITION</div>
            <div class="property-grid">
              <span>Mode</span><strong>LIVE</strong>
              <span>FAST Rate</span><strong>20.0 kHz</strong>
              <span>History</span><strong>10.000 s</strong>
              <span>Channels</span><strong>2 / 8</strong>
            </div>
          </section>

          <section class="side-section device-info">
            <div class="section-heading">DEVICE</div>
            <div class="property-grid">
              <span>State</span><strong>DISABLED</strong>
              <span>Transport</span><strong>USB CDC</strong>
              <span>Node</span><strong>1</strong>
            </div>
          </section>
        </aside>

        <section id="scope-workspace" class="scope-workspace">
          <div class="editor-tabs">
            <div class="editor-tab active"><i class="codicon codicon-graph-line"></i> Scope</div>
          </div>
          <div class="plot-header">
            <div class="trace-key"><span class="trace-mark ia"></span>Ia <span class="value">-0.184 A</span></div>
            <div class="trace-key"><span class="trace-mark iq"></span>Iq <span class="value">0.617 A</span></div>
            <div class="plot-meta">200000 samples · loss 0</div>
          </div>
          <div bind:this={plotHost} class="plot-host"></div>
        </section>
      </div>
    </main>
  </div>

  <footer class="statusbar">
    <div><i class="codicon codicon-plug"></i> /dev/ttyACM1</div>
    <div>DISABLED</div>
    <div>FAST 20 kHz</div>
    <div>2 / 8 ch</div>
    <div>loss 0</div>
    <div class="status-spacer"></div>
    <div>AxDr_L</div>
  </footer>
</div>
