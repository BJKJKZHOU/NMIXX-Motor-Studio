<script lang="ts">
  import { uiLanguage, localize } from "../i18n";
  import { onMount } from "svelte";
  import { connectDevice, disconnectDevice, listDevices } from "./api";
  import type { ConnectionInfo } from "./types";

  export let connection: ConnectionInfo | undefined;
  export let ready = false;
  export let initializing = false;
  export let onRetry: () => void = () => undefined;
  export let port: string;
  export let schemaPath: string;
  export let onConnected: (connection: ConnectionInfo) => void = () => undefined;
  export let onSchemaPathChanged: (path: string) => void = () => undefined;
  export let onDisconnected: () => void = () => undefined;
  export let onError: (error: unknown) => void = () => undefined;

  let ports: string[] = [];
  let busy = false;

  function preferredPort(candidates: string[], current: string): string {
    if (current && candidates.includes(current)) return current;
    return candidates.find((item) => /(?:ttyACM|ttyUSB|cu\.usb|tty\.usb)/i.test(item)) ?? candidates[0] ?? "";
  }

  async function refreshPorts() {
    try {
      ports = await listDevices();
      port = preferredPort(ports, connection?.port ?? port);
    } catch (error) {
      onError(error);
    }
  }

  async function connect() {
    if (!port) {
      onError("No USB CDC device detected.");
      return;
    }

    onSchemaPathChanged(schemaPath);
    busy = true;
    try {
      onConnected(await connectDevice(port, schemaPath, 115200));
    } catch (error) {
      onError(error);
    } finally {
      busy = false;
    }
  }

  async function disconnect() {
    if (busy) return;
    busy = true;
    try {
      await disconnectDevice();
      onDisconnected();
    } catch (error) {
      onError(error);
    } finally {
      busy = false;
    }
  }

  onMount(refreshPorts);
</script>

<section class="page-toolbar"><div class="page-title">CONNECTION</div></section>
<section class="page-content connection-page">
  <div class="connection-card">
    <div class="connection-card-title">{localize("Transport", $uiLanguage)}</div>
    <div class="connection-form wide-form">
      <div class="connection-form-label">{localize("Type", $uiLanguage)}</div>
      <div class="static-field">Serial / USB CDC</div>
      <label for="connection-port">{localize("Port", $uiLanguage)}</label>
      <div class="field-row">
        <input id="connection-port" bind:value={port} class="compact-input" list="device-ports" placeholder={localize("No USB CDC device detected", $uiLanguage)} />
        <datalist id="device-ports">{#each ports as item}<option value={item}></option>{/each}</datalist>
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <vscode-button secondary onclick={refreshPorts} title={localize("Refresh ports", $uiLanguage)}><i class="codicon codicon-refresh"></i></vscode-button>
      </div>
      <label for="connection-schema">HostSchema</label>
      <input id="connection-schema" bind:value={schemaPath} class="compact-input mono" onchange={() => onSchemaPathChanged(schemaPath)} />
      <div class="connection-actions">
        {#if connection}
          {#if !ready && !initializing}<vscode-button secondary disabled={busy} onclick={onRetry}>{localize("Retry initialization", $uiLanguage)}</vscode-button>{/if}
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <vscode-button secondary disabled={busy} onclick={disconnect}>{localize("Disconnect", $uiLanguage)}</vscode-button>
        {:else}
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <vscode-button disabled={busy || !port} onclick={connect}>{localize("Connect", $uiLanguage)}</vscode-button>
        {/if}
      </div>
    </div>
  </div>

  <div class="connection-card">
    <div class="connection-card-title">{localize("Device", $uiLanguage)}</div>
    <div class="property-grid connection-properties">
      <span>{localize("Status", $uiLanguage)}</span><strong>{localize(connection ? ready ? "Connected" : initializing ? "Initializing" : "Initialization incomplete" : "Disconnected", $uiLanguage)}</strong>
      <span>{localize("Endpoint", $uiLanguage)}</span><strong>{connection?.port ?? "—"}</strong>
      <span>{localize("FAST rate", $uiLanguage)}</span><strong>{connection ? `${(connection.fastRateHz / 1000).toFixed(1)} kHz` : "—"}</strong>
      <span>{localize("NORMAL rate", $uiLanguage)}</span><strong>{connection ? `${(connection.normalRateHz / 1000).toFixed(1)} kHz` : "—"}</strong>
      <span>{localize("FAST channels", $uiLanguage)}</span><strong>{connection?.fastMaxChannels ?? "—"}</strong>
      <span>{localize("NORMAL channels", $uiLanguage)}</span><strong>{connection?.normalMaxChannels ?? "—"}</strong>
      <span>{localize("FAST block", $uiLanguage)}</span><strong>{connection?.fastBlockSamples ?? "—"}</strong>
    </div>
  </div>
</section>

<style>
  .connection-form-label {
    color: #858585;
    font-size: 11px;
  }
</style>
