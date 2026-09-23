<script lang="ts">
  import type { ConnectionInfo } from "../connection/types";
  import type { ParameterValue } from "../parameters/types";
  import { selectParameters } from "../parameters/state";
  import { createParameterEditor } from "../parameters/editor";
  import { modifiedParameterIds } from "../parameters/persistence";
  type ControlLoopPage = "current" | "speed" | "position";
  type Props = { connection: ConnectionInfo | undefined; motorState: number | null; loop?: ControlLoopPage; onError?: (error: unknown) => void };
  const MOTOR_RUN = 2;
  const CURRENT_BW = "PARAM_CTRL_CURRENT_BW_HZ";
  const CURRENT_SOURCE = "PARAM_CTRL_CURRENT_SOURCE";
  const ID_KP = "PARAM_CTRL_ID_KP";
  const ID_KI = "PARAM_CTRL_ID_KI";
  const IQ_KP = "PARAM_CTRL_IQ_KP";
  const IQ_KI = "PARAM_CTRL_IQ_KI";
  const SPEED_BW = "PARAM_CTRL_SPEED_BW_HZ";
  const SPEED_SOURCE = "PARAM_CTRL_SPEED_SOURCE";
  const SPEED_KP = "PARAM_CTRL_SPEED_KP";
  const SPEED_KI = "PARAM_CTRL_SPEED_KI";
  const POSITION_KP = "PARAM_CTRL_POSITION_KP";
  const ESO_BW = "PARAM_CTRL_MECH_ESO_BW_HZ";
  const SYMBOLS = [CURRENT_BW, CURRENT_SOURCE, ID_KP, ID_KI, IQ_KP, IQ_KI, SPEED_BW, SPEED_SOURCE, SPEED_KP, SPEED_KI, POSITION_KP, ESO_BW];
  let { connection, motorState, loop = "current", onError = () => undefined }: Props = $props();
  const parameters = selectParameters(SYMBOLS);
  const edits = createParameterEditor(parameters);
  let metadata = $derived($parameters.metadata);
  let values = $derived($parameters.values);
  let drafts = $derived($edits.drafts);
  let writing = $derived($edits.writing);
  let loading = $derived($parameters.loading);
  function label(symbol: string, fallback: string): string { return metadata[symbol]?.label ?? fallback; }
  function unit(symbol: string): string { return metadata[symbol]?.unit ?? ""; }
  function locked(symbol: string): boolean { return $parameters.loading || $parameters.saving || motorState === MOTOR_RUN || writing.has(symbol) || !metadata[symbol]?.access.includes("w"); }
  function dirty(symbol: string): boolean { return $edits.dirty.has(symbol); }
  function numeric(value: ParameterValue | null | undefined): number | null { return !value || value.type === "position" ? null : Number(value.value); }
  function enumValue(symbol: string, enumSymbol: string): number | null {
    const meta = metadata[symbol];
    const index = meta?.allowedSymbols.indexOf(enumSymbol) ?? -1;
    return index < 0 ? null : meta.allowed[index] ?? index;
  }
  function sourceText(symbol: string): string {
    const value = numeric(values[symbol]);
    if (value === null) return "—";
    if (value === enumValue(symbol, "CTRL_TUNE_BANDWIDTH")) return "Bandwidth";
    if (value === enumValue(symbol, "CTRL_TUNE_MANUAL")) return "Manual";
    return String(value);
  }
  async function setSource(symbol: string, mode: "Bandwidth" | "Manual") {
    const value = enumValue(symbol, mode === "Manual" ? "CTRL_TUNE_MANUAL" : "CTRL_TUNE_BANDWIDTH");
    if (value === null || locked(symbol)) return;
    try { await edits.select(symbol, { type: "u8", value }); } catch (error) { onError(error); }
  }
  function keydown(event: KeyboardEvent, symbol: string) { if (!locked(symbol)) edits.keydown(event, symbol, onError); }
</script>

<div class="control-root">
  <section class="page-toolbar">
    <div class="page-title">CONTROL ARCHITECTURE · {loop === "current" ? "CURRENT LOOP" : loop === "speed" ? "SPEED LOOP" : "POSITION LOOP"}</div>
    {#if loading}<div class="toolbar-note"><i class="codicon codicon-loading codicon-modifier-spin"></i> Reading control parameters…</div>{/if}
  </section>
  <section class="control-content">
    {#if !connection}
      <div class="empty-state"><i class="codicon codicon-plug"></i><div>Connect a device to configure control structure.</div></div>
    {:else}
      <div class="control-sheet">
        {#if loop === "current"}
        <section class="loop-section">
          <div class="loop-heading"><div><div class="section-title">Current Loop</div><div class="section-subtitle">d/q current regulation</div></div><label class="algorithm-select">Controller<select disabled><option>PI</option></select></label></div>
          <div class="diagram current-diagram">
            <div class="signal-chip">Id / Iq Ref</div><div class="arrow">→</div><div class="sum-node">Σ</div><div class="arrow">→</div>
            <div class="control-block wide-block"><div class="block-title">PI Current Controller</div>
{#if metadata[CURRENT_SOURCE]}<div class="block-field"><span>{label(CURRENT_SOURCE, "Gain Source")}</span><select class:ramModified={$modifiedParameterIds.has(metadata[CURRENT_SOURCE].id)} class="compact-select" disabled={locked(CURRENT_SOURCE)} value={sourceText(CURRENT_SOURCE)} onchange={(event) => void setSource(CURRENT_SOURCE, event.currentTarget.value as "Bandwidth" | "Manual")}><option value="Bandwidth">Bandwidth</option><option value="Manual">Manual</option></select></div>{/if}
{#if metadata[CURRENT_BW]}<div class="block-field"><span>{label(CURRENT_BW, "Bandwidth")}</span><span class="editor"><input class:ramModified={$modifiedParameterIds.has(metadata[CURRENT_BW].id)} class:dirty={dirty(CURRENT_BW)} class="compact-input mono" value={drafts[CURRENT_BW] ?? ""} disabled={locked(CURRENT_BW)} oninput={(event) => edits.edit(CURRENT_BW, event.currentTarget.value)} onkeydown={(event) => keydown(event, CURRENT_BW)} onblur={() => edits.discard(CURRENT_BW)} /><span class="unit">{unit(CURRENT_BW)}</span></span></div>{/if}
<div class="gain-grid">
{#if metadata[ID_KP]}<div><span>Id Kp</span><input class:ramModified={$modifiedParameterIds.has(metadata[ID_KP].id)} class:dirty={dirty(ID_KP)} class="compact-input mono gain-input" value={drafts[ID_KP] ?? ""} disabled={locked(ID_KP)} oninput={(event) => edits.edit(ID_KP, event.currentTarget.value)} onkeydown={(event) => keydown(event, ID_KP)} onblur={() => edits.discard(ID_KP)} /></div>{/if}
{#if metadata[ID_KI]}<div><span>Id Ki</span><input class:ramModified={$modifiedParameterIds.has(metadata[ID_KI].id)} class:dirty={dirty(ID_KI)} class="compact-input mono gain-input" value={drafts[ID_KI] ?? ""} disabled={locked(ID_KI)} oninput={(event) => edits.edit(ID_KI, event.currentTarget.value)} onkeydown={(event) => keydown(event, ID_KI)} onblur={() => edits.discard(ID_KI)} /></div>{/if}
{#if metadata[IQ_KP]}<div><span>Iq Kp</span><input class:ramModified={$modifiedParameterIds.has(metadata[IQ_KP].id)} class:dirty={dirty(IQ_KP)} class="compact-input mono gain-input" value={drafts[IQ_KP] ?? ""} disabled={locked(IQ_KP)} oninput={(event) => edits.edit(IQ_KP, event.currentTarget.value)} onkeydown={(event) => keydown(event, IQ_KP)} onblur={() => edits.discard(IQ_KP)} /></div>{/if}
{#if metadata[IQ_KI]}<div><span>Iq Ki</span><input class:ramModified={$modifiedParameterIds.has(metadata[IQ_KI].id)} class:dirty={dirty(IQ_KI)} class="compact-input mono gain-input" value={drafts[IQ_KI] ?? ""} disabled={locked(IQ_KI)} oninput={(event) => edits.edit(IQ_KI, event.currentTarget.value)} onkeydown={(event) => keydown(event, IQ_KI)} onblur={() => edits.discard(IQ_KI)} /></div>{/if}
</div>
            </div><div class="arrow">→</div><div class="signal-chip">Ud / Uq</div>
          </div>
          <div class="feedback-row"><span>Current Feedback</span><span class="feedback-line">───────────────↩</span></div>
        </section>
        {:else if loop === "speed"}
        <section class="loop-section">
          <div class="loop-heading"><div><div class="section-title">Speed Loop</div><div class="section-subtitle">mechanical speed to q-axis current reference</div></div><label class="algorithm-select">Controller<select disabled><option>PI</option></select></label></div>
          <div class="diagram speed-diagram">
            <div class="signal-chip">ωm Ref</div><div class="arrow">→</div><div class="sum-node">Σ</div><div class="arrow">→</div>
            <div class="control-block"><div class="block-title">PI Speed Controller</div>
{#if metadata[SPEED_SOURCE]}<div class="block-field"><span>{label(SPEED_SOURCE, "Gain Source")}</span><select class:ramModified={$modifiedParameterIds.has(metadata[SPEED_SOURCE].id)} class="compact-select" disabled={locked(SPEED_SOURCE)} value={sourceText(SPEED_SOURCE)} onchange={(event) => void setSource(SPEED_SOURCE, event.currentTarget.value as "Bandwidth" | "Manual")}><option value="Bandwidth">Bandwidth</option><option value="Manual">Manual</option></select></div>{/if}
{#if metadata[SPEED_BW]}<div class="block-field"><span>{label(SPEED_BW, "Bandwidth")}</span><span class="editor"><input class:ramModified={$modifiedParameterIds.has(metadata[SPEED_BW].id)} class:dirty={dirty(SPEED_BW)} class="compact-input mono" value={drafts[SPEED_BW] ?? ""} disabled={locked(SPEED_BW)} oninput={(event) => edits.edit(SPEED_BW, event.currentTarget.value)} onkeydown={(event) => keydown(event, SPEED_BW)} onblur={() => edits.discard(SPEED_BW)} /><span class="unit">{unit(SPEED_BW)}</span></span></div>{/if}
<div class="gain-grid two">
{#if metadata[SPEED_KP]}<div><span>Kp</span><input class:ramModified={$modifiedParameterIds.has(metadata[SPEED_KP].id)} class:dirty={dirty(SPEED_KP)} class="compact-input mono gain-input" value={drafts[SPEED_KP] ?? ""} disabled={locked(SPEED_KP)} oninput={(event) => edits.edit(SPEED_KP, event.currentTarget.value)} onkeydown={(event) => keydown(event, SPEED_KP)} onblur={() => edits.discard(SPEED_KP)} /></div>{/if}
{#if metadata[SPEED_KI]}<div><span>Ki</span><input class:ramModified={$modifiedParameterIds.has(metadata[SPEED_KI].id)} class:dirty={dirty(SPEED_KI)} class="compact-input mono gain-input" value={drafts[SPEED_KI] ?? ""} disabled={locked(SPEED_KI)} oninput={(event) => edits.edit(SPEED_KI, event.currentTarget.value)} onkeydown={(event) => keydown(event, SPEED_KI)} onblur={() => edits.discard(SPEED_KI)} /></div>{/if}
</div>
            </div><div class="arrow">→</div><div class="signal-chip">Iq Ref</div>
          </div>
          <div class="speed-feedback-path"><div class="feedback-source-note">Feedback / Observer Path</div><div class="feedback-path-line">
            <div class="signal-chip feedback-signal">Observer inputs</div><div class="arrow">→</div><div class="control-block observer-block"><div class="block-title">Mechanical ESO</div>
{#if metadata[ESO_BW]}<div class="block-field"><span>{label(ESO_BW, "Observer Bandwidth")}</span><span class="editor"><input class:ramModified={$modifiedParameterIds.has(metadata[ESO_BW].id)} class:dirty={dirty(ESO_BW)} class="compact-input mono" value={drafts[ESO_BW] ?? ""} disabled={locked(ESO_BW)} oninput={(event) => edits.edit(ESO_BW, event.currentTarget.value)} onkeydown={(event) => keydown(event, ESO_BW)} onblur={() => edits.discard(ESO_BW)} /><span class="unit">{unit(ESO_BW)}</span></span></div>{/if}
            </div><div class="arrow">→</div><div class="signal-chip feedback-signal">Wm estimate</div><div class="feedback-return">↩ to speed error summing point</div>
          </div></div>
        </section>
        {:else}
        <section class="loop-section">
          <div class="loop-heading"><div><div class="section-title">Position Loop</div><div class="section-subtitle">position error to mechanical speed reference</div></div><label class="algorithm-select">Controller<select disabled><option>P</option></select></label></div>
          <div class="diagram position-diagram">
            <div class="signal-chip">Position Ref</div><div class="arrow">→</div><div class="sum-node">Σ</div><div class="arrow">→</div>
            <div class="control-block compact-block"><div class="block-title">P Position Controller</div>
{#if metadata[POSITION_KP]}<div class="block-field"><span>{label(POSITION_KP, "Kp")}</span><span class="editor"><input class:ramModified={$modifiedParameterIds.has(metadata[POSITION_KP].id)} class:dirty={dirty(POSITION_KP)} class="compact-input mono" value={drafts[POSITION_KP] ?? ""} disabled={locked(POSITION_KP)} oninput={(event) => edits.edit(POSITION_KP, event.currentTarget.value)} onkeydown={(event) => keydown(event, POSITION_KP)} onblur={() => edits.discard(POSITION_KP)} /><span class="unit">{unit(POSITION_KP)}</span></span></div>{/if}
            </div><div class="arrow">→</div><div class="signal-chip">ωm Ref</div>
          </div>
          <div class="feedback-row"><span>Encoder Position</span><span class="feedback-line">───────────────↩</span></div>
        </section>
        {/if}
        {#if motorState === MOTOR_RUN}<div class="state-note">Control parameter writes are locked while the motor is RUN.</div>{/if}
      </div>
    {/if}
  </section>
</div>

<style>
  .control-root { min-width: 0; min-height: 0; display: grid; grid-template-rows: auto 1fr; }
  .control-content { min-width: 0; min-height: 0; overflow: auto; padding: 20px 28px 36px; }
  .control-sheet { min-width: 900px; max-width: 1180px; display: grid; gap: 20px; }
  .loop-section { border: 1px solid var(--vscode-panel-border); border-radius: 4px; background: color-mix(in srgb, var(--vscode-editor-background) 97%, var(--vscode-foreground) 3%); padding: 16px 18px 14px; }
  .loop-heading { display: flex; justify-content: space-between; align-items: start; gap: 18px; margin-bottom: 16px; }
  .section-title { font-size: 13px; font-weight: 600; }
  .section-subtitle, .toolbar-note, .state-note { margin-top: 3px; color: var(--vscode-descriptionForeground); font-size: 11px; }
  .toolbar-note { margin-left: auto; margin-top: 0; }
  .algorithm-select { display: flex; align-items: center; gap: 8px; color: var(--vscode-descriptionForeground); font-size: 11px; }
  .algorithm-select select, .compact-select { height: 26px; border: 1px solid var(--vscode-dropdown-border, var(--vscode-input-border)); background: var(--vscode-dropdown-background, var(--vscode-input-background)); color: var(--vscode-dropdown-foreground, var(--vscode-input-foreground)); padding: 0 7px; font: inherit; font-size: 12px; }
  .diagram { display: grid; grid-template-columns: auto 28px 32px 28px minmax(300px, 1fr) 28px auto; align-items: center; gap: 5px; min-height: 142px; }
  .signal-chip { min-width: 94px; padding: 10px 12px; text-align: center; border: 1px solid var(--vscode-panel-border); background: var(--vscode-input-background); border-radius: 3px; font-size: 12px; font-weight: 600; white-space: nowrap; }
  .arrow { text-align: center; color: var(--vscode-descriptionForeground); font-size: 18px; }
  .sum-node { width: 30px; height: 30px; display: grid; place-items: center; border: 1px solid var(--vscode-foreground); border-radius: 50%; font-size: 13px; font-weight: 600; }
  .control-block { min-width: 0; border: 1px solid color-mix(in srgb, var(--vscode-focusBorder) 52%, var(--vscode-panel-border)); border-radius: 4px; padding: 11px 12px; background: color-mix(in srgb, var(--vscode-input-background) 88%, transparent); }
  .wide-block { min-width: 390px; }
  .compact-block { max-width: 430px; }
  .block-title { margin-bottom: 9px; font-size: 12px; font-weight: 600; }
  .block-field { min-height: 34px; display: grid; grid-template-columns: minmax(110px, 1fr) minmax(130px, 1.1fr); gap: 12px; align-items: center; border-top: 1px solid color-mix(in srgb, var(--vscode-panel-border) 55%, transparent); font-size: 11px; }
  .editor { min-width: 0; display: grid; grid-template-columns: minmax(80px, 1fr) auto; align-items: center; gap: 7px; }
  .compact-input { width: 100%; min-width: 0; }
  .compact-input.dirty { border-color: var(--vscode-inputValidation-warningBorder, var(--vscode-focusBorder)); }
  .unit { color: var(--vscode-descriptionForeground); font-size: 11px; white-space: nowrap; }
  .gain-grid { display: grid; grid-template-columns: repeat(4, minmax(82px, 1fr)); gap: 7px; margin-top: 9px; }
  .gain-grid.two { grid-template-columns: repeat(2, minmax(110px, 1fr)); }
  .gain-grid > div { display: grid; gap: 3px; padding: 7px 8px; border: 1px solid color-mix(in srgb, var(--vscode-panel-border) 70%, transparent); border-radius: 3px; }
  .gain-grid span { color: var(--vscode-descriptionForeground); font-size: 10px; }
  .gain-input { height: 25px; font-size: 11px; }
  .feedback-row { display: flex; align-items: center; justify-content: center; gap: 14px; min-height: 34px; color: var(--vscode-descriptionForeground); font-size: 11px; }
  .speed-feedback-path { margin-top: 18px; padding-top: 14px; border-top: 1px solid color-mix(in srgb, var(--vscode-panel-border) 70%, transparent); }
  .feedback-source-note { margin-bottom: 9px; color: var(--vscode-descriptionForeground); font-size: 11px; font-weight: 600; }
  .feedback-path-line { display: grid; grid-template-columns: minmax(120px, auto) 28px minmax(320px, 420px) 28px minmax(110px, auto) minmax(180px, 1fr); align-items: center; gap: 6px; }
  .feedback-signal { min-width: 0; }
  .observer-block { width: auto; }
  .feedback-return { color: var(--vscode-descriptionForeground); font-size: 11px; white-space: nowrap; }
  .feedback-line { letter-spacing: 1px; white-space: nowrap; }
  .state-note { padding: 2px 4px; }
  @media (max-width: 980px) { .control-sheet { min-width: 0; } .diagram { grid-template-columns: 1fr; justify-items: stretch; } .diagram .arrow { transform: rotate(90deg); } .sum-node { justify-self: center; } .wide-block, .compact-block { min-width: 0; max-width: none; width: auto; } .feedback-row { padding-left: 0; flex-wrap: wrap; } .feedback-path-line { grid-template-columns: 1fr; } .feedback-path-line .arrow { transform: rotate(90deg); text-align: center; } .feedback-return { white-space: normal; } }
</style>
