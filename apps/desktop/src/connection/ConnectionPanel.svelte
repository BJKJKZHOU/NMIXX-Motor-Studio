<script lang="ts">
  import { onMount } from "svelte";
  import { connectDevice, disconnectDevice, listDevices } from "./api";
  import type { ConnectionInfo } from "./types";

  export let connection: ConnectionInfo | undefined;
  export let onConnected: (connection: ConnectionInfo) => void = () => undefined;
  export let onDisconnected: () => void = () => undefined;
  export let onError: (error: unknown) => void = () => undefined;

  let ports: string[] = [];
  let port = "/dev/ttyACM1";
  let schemaPath = "../../../AxDr_L_Motor/build/host/axdr-host-schema.toml";
  let busy = false;

  async function refreshPorts() {
    try {
      ports = await listDevices();
      if (ports.length > 0 && !ports.includes(port)) port = ports[0];
    } catch (error) { onError(error); }
  }

  async function connect() {
    busy = true;
    try { onConnected(await connectDevice(port, schemaPath, 115200)); }
    catch (error) { onError(error); }
    finally { busy = false; }
  }

  async function disconnect() {
    try { await disconnectDevice(); }
    catch (error) { onError(error); }
    finally { onDisconnected(); }
  }

  onMount(refreshPorts);
</script>

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
      {#if connection}<vscode-button secondary onclick={disconnect}>Disconnect</vscode-button>
      {:else}<vscode-button disabled={busy} onclick={connect}>Connect</vscode-button>{/if}
    </div>
  </div>
</section>
