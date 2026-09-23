<script lang="ts">
  import {
    FlexRender, columnFilteringFeature, createFilteredRowModel, createSortedRowModel,
    createTable, filterFn_includesString, globalFilteringFeature, rowSortingFeature, tableFeatures,
  } from "@tanstack/svelte-table";
  import type { ColumnDef } from "@tanstack/svelte-table";
  import type { ConnectionInfo } from "../connection/types";
  import { selectParameters } from "./state";
  import { createParameterEditor } from "./editor";
  import { parameterText } from "./codec";
  import type { ParameterMetadata, ParameterValue } from "./types";
  import { modifiedParameterIds } from "./persistence";

  type Props = { connection: ConnectionInfo | undefined; onError?: (error: unknown) => void };
  type ParameterRow = { meta: ParameterMetadata; value: ParameterValue | null; error: string | null; pending: boolean };
  let { connection, onError = () => undefined }: Props = $props();
  const parameters = selectParameters();
  const edits = createParameterEditor(parameters);
  let search = $state("");
  let writeErrors = $state<Record<number, string>>({});
  let drafts = $derived($edits.drafts);
  let writing = $derived($edits.writing);
  let loadingRegistry = $derived($parameters.loading);
  let rows: ParameterRow[] = $derived(Object.values($parameters.metadata).map((meta) => ({
    meta, value: $parameters.values[meta.symbol] ?? null,
    error: $parameters.errors[meta.symbol] ?? writeErrors[meta.id] ?? null,
    pending: $parameters.loading && meta.access.includes("r") && !$parameters.values[meta.symbol],
  })));
  const features = tableFeatures({
    columnFilteringFeature, globalFilteringFeature, rowSortingFeature,
    filteredRowModel: createFilteredRowModel(), sortedRowModel: createSortedRowModel(),
  });
  const columns: Array<ColumnDef<typeof features, ParameterRow>> = [
    { id: "id", accessorFn: (row) => row.meta.id, header: "ID" },
    { id: "symbol", accessorFn: (row) => row.meta.symbol, header: "Symbol" },
    { id: "label", accessorFn: (row) => row.meta.label, header: "Label" },
    { id: "value", accessorFn: (row) => row.pending ? "…" : valueText(row.value), header: "Value" },
    { id: "unit", accessorFn: (row) => row.meta.unit ?? "", header: "Unit" },
    { id: "access", accessorFn: (row) => row.meta.access, header: "Access" },
    { id: "type", accessorFn: (row) => row.meta.typeName, header: "Type" },
    { id: "range", accessorFn: (row) => rangeText(row.meta), header: "Range" },
    { id: "state", accessorFn: (row) => row.meta.writeState ?? "", header: "Write state" },
  ];
  const table = createTable({ features, columns, get data() { return rows; }, globalFilterFn: filterFn_includesString });

  function isWritable(meta: ParameterMetadata) { return meta.access.includes("w"); }
  function valueText(value: ParameterValue | null) { return parameterText(value) || "—"; }
  function rangeText(meta: ParameterMetadata): string {
    if (!meta.range) return "";
    const left = meta.range.exclusiveMin ? "(" : "[";
    const right = meta.range.exclusiveMax ? ")" : "]";
    const min = meta.range.min ?? "−∞";
    const max = meta.range.maxSymbol ?? meta.range.max ?? "+∞";
    return `${left}${min}, ${max}${right}`;
  }
  function editValue(row: ParameterRow, text: string) {
    edits.edit(row.meta.symbol, text);
    const next = { ...writeErrors };
    delete next[row.meta.id];
    writeErrors = next;
  }
  function keydown(event: KeyboardEvent, row: ParameterRow) {
    edits.keydown(event, row.meta.symbol, (error) => {
      writeErrors = { ...writeErrors, [row.meta.id]: error instanceof Error ? error.message : String(error) };
      onError(error);
    });
  }
  function handleSearch(event: Event) {
    search = (event.currentTarget as HTMLInputElement).value;
    table.setGlobalFilter(search);
  }
</script>

<div class="parameter-root">
  <section class="page-toolbar">
    <div class="page-title">PARAMETERS</div>
    <div class="parameter-toolbar">
      <div class="parameter-search">
        <i class="codicon codicon-search"></i>
        <input class="compact-input" type="search" placeholder="Search parameters" value={search} disabled={!connection} oninput={handleSearch} />
      </div>
      <span class="parameter-count">
        {#if !connection}
          Not connected
        {:else if loadingRegistry}
          Reading registry…
        {:else}
          {table.getRowModel().rows.length} / {rows.length}
        {/if}
      </span>
    </div>
  </section>

  <section class="parameter-content">
    {#if !connection}
      <div class="empty-state"><i class="codicon codicon-plug"></i><div>Connect a device to inspect its Parameter registry.</div></div>
    {:else if rows.length === 0}
      <div class="empty-state"><i class="codicon codicon-loading codicon-modifier-spin"></i><div>{loadingRegistry ? "Reading Parameter registry…" : "No parameters exposed by schema."}</div></div>
    {:else}
      <div class="parameter-table-shell">
        <table class="parameter-table">
          <thead>
            {#each table.getHeaderGroups() as headerGroup (headerGroup.id)}
              <tr>
                {#each headerGroup.headers as header (header.id)}
                  <th class:parameter-value-column={header.column.id === "value"}>
                    {#if !header.isPlaceholder}
                      <button class="table-header-button" disabled={!header.column.getCanSort()} onclick={header.column.getToggleSortingHandler()}>
                        <FlexRender {header} />
                        {#if header.column.getIsSorted() === "asc"}<i class="codicon codicon-arrow-up"></i>{:else if header.column.getIsSorted() === "desc"}<i class="codicon codicon-arrow-down"></i>{/if}
                      </button>
                    {/if}
                  </th>
                {/each}
              </tr>
            {/each}
          </thead>
          <tbody>
            {#each table.getRowModel().rows as tableRow (tableRow.id)}
              {@const row = tableRow.original}
              <tr class:error-row={!!row.error} title={row.error ?? row.meta.description}>
                {#each tableRow.getAllCells() as cell (cell.id)}
                  <td class:parameter-value-column={cell.column.id === "value"}>
                    {#if cell.column.id === "id"}
                      <span class="mono parameter-id">0x{row.meta.id.toString(16).toUpperCase().padStart(4, "0")}</span>
                    {:else if cell.column.id === "symbol"}
                      <span class="mono parameter-symbol">{row.meta.symbol}</span>
                    {:else if cell.column.id === "label"}
                      <span>{row.meta.label}</span>
                    {:else if cell.column.id === "value"}
                      {#if row.pending}
                        <span class="pending-value"><i class="codicon codicon-loading codicon-modifier-spin"></i></span>
                      {:else if isWritable(row.meta)}
                        <input
                          class:ramModified={$modifiedParameterIds.has(row.meta.id)}
                          class="parameter-value-input mono"
                          value={drafts[row.meta.symbol] ?? ""}
                          disabled={$parameters.saving || writing.has(row.meta.symbol)}
                          aria-label={`Value for ${row.meta.label}`}
                          oninput={(event) => editValue(row, event.currentTarget.value)}
                          onkeydown={(event) => keydown(event, row)}
                          onblur={() => edits.discard(row.meta.symbol)}
                        />
                      {:else}
                        <span class="mono">{valueText(row.value)}</span>
                      {/if}
                    {:else if cell.column.id === "unit"}
                      <span class="muted">{row.meta.unit ?? "—"}</span>
                    {:else if cell.column.id === "access"}
                      <span class:rw={isWritable(row.meta)} class="access-badge">{row.meta.access.toUpperCase()}</span>
                    {:else if cell.column.id === "type"}
                      <span class="mono muted">{row.meta.typeName}</span>
                    {:else if cell.column.id === "range"}
                      <span class="mono muted">{rangeText(row.meta) || "—"}</span>
                    {:else if cell.column.id === "state"}
                      <span class="muted">{row.meta.writeState ?? "—"}</span>
                    {/if}
                  </td>
                {/each}
              </tr>
            {:else}
              <tr><td colspan={columns.length} class="parameter-no-results">No parameters match “{search}”.</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>
</div>

<style>
  .parameter-root { min-height: 0; display: grid; grid-template-rows: 36px minmax(0, 1fr); }
  .parameter-toolbar { margin-left: auto; display: flex; align-items: center; gap: 8px; }
  .parameter-search { width: min(360px, 32vw); display: grid; grid-template-columns: 24px minmax(0, 1fr); align-items: center; border: 1px solid var(--vscode-input-border); border-radius: 2px; background: var(--vscode-input-background); }
  .parameter-search:focus-within { border-color: var(--vscode-focusBorder); }
  .parameter-search i { text-align: center; color: #838383; }
  .parameter-search .compact-input { border: 0; background: transparent; }
  .parameter-search .compact-input:focus { border: 0; }
  .parameter-count { min-width: 105px; color: #848484; font-size: 11px; text-align: right; }
  .table-header-button { border: 0; color: #c8c8c8; background: transparent; font: inherit; }
  .table-header-button:disabled { opacity: .5; }
  .parameter-content { min-height: 0; overflow: hidden; background: #1e1e1e; }
  .parameter-table-shell { width: 100%; height: 100%; overflow: auto; }
  .parameter-table { width: 100%; min-width: 1080px; border-collapse: separate; border-spacing: 0; table-layout: auto; font-size: 12px; }
  .parameter-table th { position: sticky; top: 0; z-index: 3; height: 30px; padding: 0; border-right: 1px solid #303030; border-bottom: 1px solid #3a3a3a; color: #a7a7a7; background: #202020; font-size: 11px; font-weight: 600; text-align: left; white-space: nowrap; }
  .parameter-table td { height: 29px; padding: 3px 8px; border-right: 1px solid #292929; border-bottom: 1px solid #292929; color: #c6c6c6; white-space: nowrap; vertical-align: middle; }
  .parameter-table tbody tr:hover td { background: #24282f; }
  .parameter-table tbody tr.error-row td { background: rgba(126, 53, 53, .16); }
  .table-header-button { width: 100%; height: 29px; display: flex; align-items: center; gap: 5px; padding: 0 8px; text-align: left; }
  .table-header-button:not(:disabled) { cursor: pointer; }
  .table-header-button:not(:disabled):hover { color: #e0e0e0; background: #292d33; }
  .parameter-value-column { min-width: 170px; }
  .mono { font-family: "SFMono-Regular", Consolas, "Liberation Mono", monospace; }
  .muted, .parameter-id { color: #8c8c8c; }
  .parameter-symbol { color: #d0d0d0; }
  .pending-value { color: #777; }
  .parameter-value-input { width: 100%; min-width: 90px; height: 23px; padding: 1px 5px; border: 1px solid transparent; border-radius: 2px; outline: none; color: #d8d8d8; background: transparent; }
  .parameter-value-input:hover { border-color: #3a4049; background: #25292f; }
  .parameter-value-input:focus { border-color: var(--vscode-focusBorder); background: var(--vscode-input-background); }
  .access-badge { display: inline-flex; min-width: 28px; justify-content: center; padding: 1px 5px; border: 1px solid #3a3a3a; border-radius: 8px; color: #8d8d8d; font-size: 10px; line-height: 15px; }
  .access-badge.rw { color: #b5c2d4; border-color: #465265; background: #29303b; }
  .empty-state { height: 100%; display: grid; place-content: center; justify-items: center; gap: 10px; color: #777; }
  .empty-state i { font-size: 24px; }
  .parameter-no-results { height: 72px !important; color: #777 !important; text-align: center; }
</style>
