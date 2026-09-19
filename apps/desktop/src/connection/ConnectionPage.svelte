<script lang="ts">
  import { onMount } from "svelte";
  import { connectDevice, disconnectDevice, listDevices } from "./api";
  import type { ConnectionInfo } from "./types";
  import { connectionPort, connectionSchemaPath } from "./formState";

  export let connection: ConnectionInfo | undefined;
  export let onConnected: (connection: ConnectionInfo) => void = () => undefined;
  export let onDisconnected: () => void = () => undefined;
  export let onError: (error: unknown) => void = () => undefined;

  let ports: string[] = [];
  let busy = false;

  function preferredPort(candidates: string[], current: string): string {
    if (current) return current;
    return candidates.find((item) => /(?:ttyACM|ttyUSB|cu\.usb|tty\.usb)/i.test(item)) ?? candidates[0] ?? "";
  }

  async function refreshPorts() {
    try {
      ports = await listDevices();
      $connectionPort = preferredPort(ports, connection?.port ?? $connectionPort);
    } catch (error) {
      onError(error);
    }
  }

  async function connect() {
    if (!$connectionPort) {
      onError("No USB CDC device detected.");
      return;
    }

    busy = true;
    try {
      onConnected(await connectDevice($connectionPort, $connectionSchemaPath, 115200));
    } catch (error) {
      onError(error);
    } finally {
      busy = false;
    }
  }

  async function disconnect() {
    try {
      await disconnectDevice();
    } catch (error) {
      onError(error);
    } finally {
      onDisconnected();
    }
  }

  onMount(refreshPorts);
</script>

<section class="page-toolbar"><div class="page-title">CONNECTION</div></section>
<section class="page-content connection-page">
  <div class="connection-card">
    <div class="connection-card-title">Transport</div>
    <div class="connection-form wide-form">
      <div class="connection-form-label">Type</div>
      <div class="static-field">Serial / USB CDC</div>
      <label for="connection-port">Port</label>
      <div class="field-row">
        <input id="connection-port" bind:value={$connectionPort} class="compact-input" list="device-ports" placeholder="No USB CDC device detected" />
        <datalist id="device-ports">{#each ports as item}<option value={item}></option>{/each}</datalist>
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <vscode-button secondary onclick={refreshPorts} title="Refresh ports"><i class="codicon codicon-refresh"></i></vscode-button>
      </div>
      <label for="connection-schema">HostSchema</label>
      <input id="connection-schema" bind:value={$connectionSchemaPath} class="compact-input mono" />
      <div class="connection-actions">
        {#if connection}
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <vscode-button secondary onclick={disconnect}>Disconnect</vscode-button>
        {:else}
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <vscode-button disabled={busy || !$connectionPort} onclick={connect}>Connect</vscode-button>
        {/if}
      </div>
    </div>
  </div>

  <div class="connection-card">
    <div class="connection-card-title">Device</div>
    <div class="property-grid connection-properties">
      <span>Status</span><strong>{connection ? "Connected" : "Disconnected"}</strong>
      <span>Endpoint</span><strong>{connection?.port ?? "—"}</strong>
      <span>FAST rate</span><strong>{connection ? `${(connection.fastRateHz / 1000).toFixed(1)} kHz` : "—"}</strong>
      <span>NORMAL rate</span><strong>{connection ? `${(connection.normalRateHz / 1000).toFixed(1)} kHz` : "—"}</strong>
      <span>FAST channels</span><strong>{connection?.fastMaxChannels ?? "—"}</strong>
      <span>NORMAL channels</span><strong>{connection?.normalMaxChannels ?? "—"}</strong>
      <span>FAST block</span><strong>{connection?.fastBlockSamples ?? "—"}</strong>
    </div>
  </div>
</section>

<style>
  .connection-form-label {
    color: #858585;
    font-size: 11px;
  }
</style>
