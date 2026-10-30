<script lang="ts">
  import { uiLanguage, localize, translate } from "../i18n";
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { connectDevice, disconnectDevice, listDevices, defaultSchemaPath } from "./api";
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
  let choosingSchema = false;
  const LEGACY_SCHEMA_PATH = "../../../AxDr_L_Motor/build/host/axdr-host-schema.toml";

  async function discoverSchema() {
    if (schemaPath.trim() && schemaPath !== LEGACY_SCHEMA_PATH) return;
    const previous = schemaPath;
    try {
      const discovered = await defaultSchemaPath();
      if (!busy && !connection && !choosingSchema && schemaPath === previous) {
        schemaPath = discovered ?? "";
        onSchemaPathChanged(schemaPath);
      }
    } catch (error) { onError(error); }
  }

  async function chooseSchema() {
    if (busy || connection || choosingSchema) return;
    choosingSchema = true;
    try {
      const selected = await open({ multiple: false, directory: false,
        filters: [{ name: "HostSchema", extensions: ["toml"] }] });
      if (typeof selected === "string") {
        schemaPath = selected;
        onSchemaPathChanged(schemaPath);
      }
    } catch (error) { onError(error); }
    finally { choosingSchema = false; }
  }

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

  function selectPort(select: HTMLSelectElement) {
    if (!busy && !connection && ports.includes(select.value)) port = select.value;
    select.value = "";
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

  onMount(() => {
    void refreshPorts();
    // Parent onMount restores the saved path before discovery reads it.
    queueMicrotask(() => void discoverSchema());
  });
</script>

<section class="page-toolbar"><div class="page-title">{translate("ui.connection", $uiLanguage).toUpperCase()}</div></section>
<section class="page-content connection-page">
  <div class="connection-card">
    <div class="connection-card-title">{translate("ui.transport", $uiLanguage)}</div>
    <div class="connection-form wide-form">
      <div class="connection-form-label">{translate("ui.type", $uiLanguage)}</div>
      <div class="static-field">Serial / USB CDC</div>
      <label for="connection-port">{translate("ui.port", $uiLanguage)}</label>
      <div class="field-row port-row">
        <input id="connection-port" bind:value={port} class="compact-input" disabled={busy || !!connection} placeholder={translate("ui.no.usb.cdc.device.detected", $uiLanguage)} />
        <select id="connection-port-list" class="port-picker" value="" disabled={busy || !!connection || ports.length === 0} aria-label={translate("connection.choose.port", $uiLanguage)} onchange={(event) => selectPort(event.currentTarget)}>
          <option value="" disabled>{translate("connection.choose.port", $uiLanguage)} ({ports.length})</option>
          {#each ports as item}<option value={item}>{item}</option>{/each}
        </select>
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (vscode-button provides button semantics and keyboard handling) -->
        <vscode-button secondary onclick={refreshPorts} title={translate("ui.refresh.ports", $uiLanguage)}><i class="codicon codicon-refresh"></i></vscode-button>
      </div>
      <label for="connection-schema">HostSchema</label>
      <div class="field-row">
        <input id="connection-schema" bind:value={schemaPath} class="compact-input mono" disabled={busy || !!connection || choosingSchema} placeholder="axdr-host-schema.toml" onchange={() => onSchemaPathChanged(schemaPath)} />
        <button type="button" class="schema-browse" disabled={busy || !!connection || choosingSchema} onclick={() => void chooseSchema()} aria-label={translate("connection.choose.schema", $uiLanguage)} title={translate("connection.choose.schema", $uiLanguage)}><i class="codicon codicon-folder-opened"></i></button>
      </div>
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
  .port-row { grid-template-columns: minmax(0, 1fr) 130px 30px; }
  .port-picker { min-width: 0; width: 100%; height: 28px; font: inherit; font-size: 11px; color: var(--vscode-foreground); background: var(--vscode-input-background); border: 1px solid var(--vscode-input-border); }
  .schema-browse { height: 28px; padding: 0 10px; color: var(--vscode-foreground); background: var(--vscode-button-secondaryBackground); border: 1px solid var(--vscode-input-border); cursor: pointer; }
  .schema-browse:disabled { opacity: 0.5; cursor: default; }
  .connection-form-label {
    color: #858585;
    font-size: 11px;
  }
</style>
