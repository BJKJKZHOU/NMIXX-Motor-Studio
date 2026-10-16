<script lang="ts">
  import { uiLanguage, localize, translate } from "../i18n";
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
    const selected = port;
    try {
      const discovered = await listDevices();
      ports = discovered;
      // Port discovery must not overwrite a port typed while the request ran.
      if (!busy && port === selected) port = preferredPort(discovered, connection?.port ?? selected);
    } catch (error) {
      onError(error);
    }
  }

  async function connect() {
    if (busy || connection) return;
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

<section class="page-toolbar"><div class="page-title">{translate("ui.connection", $uiLanguage).toUpperCase()}</div></section>
<section class="page-content connection-page">
  <div class="connection-card">
    <div class="connection-card-title">{translate("ui.transport", $uiLanguage)}</div>
    <div class="connection-form wide-form">
      <div class="connection-form-label">{translate("ui.type", $uiLanguage)}</div>
      <div class="static-field">Serial / USB CDC</div>
      <label for="connection-port">{translate("ui.port", $uiLanguage)}</label>
      <div class="field-row">
        <input id="connection-port" bind:value={port} class="compact-input" list="device-ports" placeholder={translate("ui.no.usb.cdc.device.detected", $uiLanguage)} />
        <datalist id="device-ports">{#each ports as item}<option value={item}></option>{/each}</datalist>
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (vscode-button provides button semantics and keyboard handling) -->
        <vscode-button secondary onclick={refreshPorts} title={translate("ui.refresh.ports", $uiLanguage)}><i class="codicon codicon-refresh"></i></vscode-button>
      </div>
      <label for="connection-schema">HostSchema</label>
      <input id="connection-schema" bind:value={schemaPath} class="compact-input mono" onchange={() => onSchemaPathChanged(schemaPath)} />
      <div class="connection-actions">
        {#if connection}
          {#if !ready && !initializing}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (vscode-button provides button semantics and keyboard handling) -->
            <vscode-button secondary disabled={busy} onclick={onRetry}>{translate("ui.retry.initialization", $uiLanguage)}</vscode-button>
          {/if}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (vscode-button provides button semantics and keyboard handling) -->
          <vscode-button secondary disabled={busy} onclick={disconnect}>{translate("ui.disconnect", $uiLanguage)}</vscode-button>
        {:else}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (vscode-button provides button semantics and keyboard handling) -->
          <vscode-button disabled={busy || !port} onclick={connect}>{translate("ui.connect", $uiLanguage)}</vscode-button>
        {/if}
      </div>
    </div>
  </div>

  <div class="connection-card">
    <div class="connection-card-title">{translate("ui.device", $uiLanguage)}</div>
    <div class="property-grid connection-properties">
      <span>{translate("ui.status", $uiLanguage)}</span><strong>{localize(connection ? ready ? "Connected" : initializing ? "Initializing" : "Initialization incomplete" : "Disconnected", $uiLanguage)}</strong>
      <span>{translate("ui.endpoint", $uiLanguage)}</span><strong>{connection?.port ?? "—"}</strong>
      <span>{translate("ui.fast.rate", $uiLanguage)}</span><strong>{connection ? `${(connection.fastRateHz / 1000).toFixed(1)} kHz` : "—"}</strong>
      <span>{translate("ui.normal.rate", $uiLanguage)}</span><strong>{connection ? `${(connection.normalRateHz / 1000).toFixed(1)} kHz` : "—"}</strong>
      <span>{translate("ui.fast.channels", $uiLanguage)}</span><strong>{connection?.fastMaxChannels ?? "—"}</strong>
      <span>{translate("ui.normal.channels", $uiLanguage)}</span><strong>{connection?.normalMaxChannels ?? "—"}</strong>
      <span>{translate("ui.fast.block", $uiLanguage)}</span><strong>{connection?.fastBlockSamples ?? "—"}</strong>
    </div>
  </div>
</section>

<style>
  .connection-form-label {
    color: #858585;
    font-size: 11px;
  }
</style>
