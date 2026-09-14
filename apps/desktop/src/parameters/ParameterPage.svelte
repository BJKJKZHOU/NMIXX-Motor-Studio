<script lang="ts">
  import {
    FlexRender,
    columnFilteringFeature,
    createColumnHelper,
    createFilteredRowModel,
    createSortedRowModel,
    createTable,
    filterFn_includesString,
    globalFilteringFeature,
    rowSortingFeature,
    tableFeatures,
  } from "@tanstack/svelte-table";
  import type { ConnectionInfo } from "../connection/types";
  import { listParameters, readParameter, readParameters, writeParameter } from "./api";
  import type { ParameterMetadata, ParameterValue } from "./types";

  type Props = {
    connection: ConnectionInfo | undefined;
    onError?: (error: unknown) => void;
  };

  type ParameterRow = {
    meta: ParameterMetadata;
    value: ParameterValue | null;
    error: string | null;
    pending: boolean;
  };

  let { connection, onError = () => undefined }: Props = $props();

  let rows = $state<ParameterRow[]>([]);
  let loadingRegistry = $state(false);
  let readingValues = $state(false);
  let readDone = $state(0);
  let readTotal = $state(0);
  let search = $state("");
  let drafts = $state<Record<number, string>>({});
  let writing = $state<Set<number>>(new Set());
  let loadGeneration = 0;

  const READ_BATCH_SIZE = 8;

  const features = tableFeatures({
    columnFilteringFeature,
    globalFilteringFeature,
    rowSortingFeature,
    filteredRowModel: createFilteredRowModel(),
    sortedRowModel: createSortedRowModel(),
  });

  const columnHelper = createColumnHelper<typeof features, ParameterRow>();
  const columns = columnHelper.columns([
    columnHelper.accessor((row) => row.meta.id, { id: "id", header: "ID" }),
    columnHelper.accessor((row) => row.meta.symbol, { id: "symbol", header: "Symbol" }),
    columnHelper.accessor((row) => row.meta.name ?? "", { id: "name", header: "Name" }),
    columnHelper.accessor((row) => row.pending ? "…" : valueText(row.value), { id: "value", header: "Value" }),
    columnHelper.accessor((row) => row.meta.unit ?? "", { id: "unit", header: "Unit" }),
    columnHelper.accessor((row) => row.meta.access, { id: "access", header: "Access" }),
    columnHelper.accessor((row) => row.meta.typeName, { id: "type", header: "Type" }),
    columnHelper.accessor((row) => rangeText(row.meta), { id: "range", header: "Range" }),
    columnHelper.accessor((row) => row.meta.writeState ?? "", { id: "state", header: "Write state" }),
  ]);

  const table = createTable({
    features,
    columns,
    get data() {
      return rows;
    },
    globalFilterFn: filterFn_includesString,
  });

  $effect(() => {
    const activeConnection = connection;
    const generation = ++loadGeneration;

    if (!activeConnection) {
      rows = [];
      drafts = {};
      loadingRegistry = false;
      readingValues = false;
      readDone = 0;
      readTotal = 0;
      search = "";
      table.setGlobalFilter("");
      return;
    }

    void loadRegistry(activeConnection, generation);
  });

  function isReadable(meta: ParameterMetadata): boolean {
    return meta.access.toLowerCase().includes("r");
  }

  function isWritable(meta: ParameterMetadata): boolean {
    return meta.access.toLowerCase().includes("w");
  }

  function valueText(value: ParameterValue | null): string {
    if (!value) return "—";
    if (value.type === "position") return `${value.value.turns}, ${value.value.theta}`;
    if (value.type === "f32") return Number(value.value).toPrecision(7).replace(/(?:\.0+|(\.\d+?)0+)$/, "$1");
    return String(value.value);
  }

  function rangeText(meta: ParameterMetadata): string {
    if (!meta.range) return "";
    const left = meta.range.exclusiveMin ? "(" : "[";
    const right = meta.range.exclusiveMax ? ")" : "]";
    const min = meta.range.min ?? "−∞";
    const max = meta.range.maxSymbol ?? meta.range.max ?? "+∞";
    return `${left}${min}, ${max}${right}`;
  }

  function parseValue(meta: ParameterMetadata, text: string): ParameterValue {
    const trimmed = text.trim();
    if (meta.typeName === "position") {
      const parts = trimmed.split(/[,:]/).map((part) => part.trim());
      if (parts.length !== 2) throw new Error("Position value must be 'turns, theta'.");
      const turns = Number(parts[0]);
      const theta = Number(parts[1]);
      if (!Number.isInteger(turns) || !Number.isFinite(theta)) throw new Error("Position value contains an invalid number.");
      return { type: "position", value: { turns, theta } };
    }

    const parsed = Number(trimmed);
    if (!Number.isFinite(parsed)) throw new Error("Value must be a finite number.");

    switch (meta.typeName) {
      case "u8":
      case "u32":
        if (!Number.isInteger(parsed) || parsed < 0) throw new Error(`${meta.typeName} requires a non-negative integer.`);
        return { type: meta.typeName, value: parsed };
      case "i8":
      case "i32":
        if (!Number.isInteger(parsed)) throw new Error(`${meta.typeName} requires an integer.`);
        return { type: meta.typeName, value: parsed };
      case "f32":
        return { type: "f32", value: parsed };
    }
  }

  function setRowError(id: number, error: string | null) {
    rows = rows.map((row) => row.meta.id === id ? { ...row, error } : row);
  }

  function applyReadResults(results: Awaited<ReturnType<typeof readParameters>>) {
    const byId = new Map(results.map((result) => [result.id, result]));
    const draftPatch: Record<number, string> = {};

    rows = rows.map((row) => {
      const result = byId.get(row.meta.id);
      if (!result) return row;
      const value = result.value ?? null;
      if (value) draftPatch[row.meta.id] = valueText(value);
      return { ...row, value, error: result.error, pending: false };
    });

    drafts = { ...drafts, ...draftPatch };
  }

  async function loadRegistry(activeConnection: ConnectionInfo, generation: number) {
    loadingRegistry = true;
    readingValues = false;
    readDone = 0;
    readTotal = 0;

    try {
      const metadata = await listParameters();
      if (generation !== loadGeneration || connection !== activeConnection) return;

      const readable = metadata.filter(isReadable);
      rows = metadata.map((meta) => ({ meta, value: null, error: null, pending: isReadable(meta) }));
      drafts = {};
      loadingRegistry = false;
      readingValues = true;
      readTotal = readable.length;

      for (let offset = 0; offset < readable.length; offset += READ_BATCH_SIZE) {
        if (generation !== loadGeneration || connection !== activeConnection) return;
        const batch = readable.slice(offset, offset + READ_BATCH_SIZE);
        try {
          const results = await readParameters(batch.map((item) => item.id));
          if (generation !== loadGeneration || connection !== activeConnection) return;
          applyReadResults(results);
        } catch (error) {
          if (generation !== loadGeneration || connection !== activeConnection) return;
          const message = error instanceof Error ? error.message : String(error);
          const ids = new Set(batch.map((item) => item.id));
          rows = rows.map((row) => ids.has(row.meta.id) ? { ...row, error: message, pending: false } : row);
        }
        readDone = Math.min(offset + batch.length, readable.length);
      }
    } catch (error) {
      if (generation === loadGeneration) onError(error);
    } finally {
      if (generation === loadGeneration && connection === activeConnection) {
        loadingRegistry = false;
        readingValues = false;
      }
    }
  }

  function refreshAll() {
    const activeConnection = connection;
    if (!activeConnection || loadingRegistry || readingValues) return;
    const generation = ++loadGeneration;
    void loadRegistry(activeConnection, generation);
  }

  async function refreshOne(row: ParameterRow) {
    try {
      const result = await readParameter(row.meta.id);
      drafts = { ...drafts, [row.meta.id]: valueText(result.value) };
      rows = rows.map((item) => item.meta.id === row.meta.id ? { ...item, value: result.value, error: null, pending: false } : item);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setRowError(row.meta.id, message);
    }
  }

  async function commitValue(row: ParameterRow) {
    if (!isWritable(row.meta) || row.pending || writing.has(row.meta.id)) return;
    writing = new Set(writing).add(row.meta.id);
    setRowError(row.meta.id, null);

    try {
      const value = parseValue(row.meta, drafts[row.meta.id] ?? "");
      await writeParameter(row.meta.id, value);
      await refreshOne(row);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setRowError(row.meta.id, message);
      onError(error);
    } finally {
      const next = new Set(writing);
      next.delete(row.meta.id);
      writing = next;
    }
  }

  function handleSearch(event: Event) {
    search = (event.currentTarget as HTMLInputElement).value;
    table.setGlobalFilter(search);
  }

  function handleValueKey(event: KeyboardEvent, row: ParameterRow) {
    if (event.key === "Enter") {
      event.preventDefault();
      void commitValue(row);
    } else if (event.key === "Escape") {
      drafts = { ...drafts, [row.meta.id]: row.value ? valueText(row.value) : "" };
      (event.currentTarget as HTMLInputElement).blur();
    }
  }
</script>

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
      {:else if readingValues}
        {readDone} / {readTotal} values
      {:else}
        {table.getRowModel().rows.length} / {rows.length}
      {/if}
    </span>
    <button class="tool-button" disabled={!connection || loadingRegistry || readingValues} onclick={refreshAll} title="Refresh all parameters">
      <i class={`codicon ${loadingRegistry || readingValues ? "codicon-loading codicon-modifier-spin" : "codicon-refresh"}`}></i>
      Refresh
    </button>
  </div>
</section>

<section class="page-content parameter-page">
  {#if !connection}
    <div class="empty-state"><i class="codicon codicon-plug"></i><div>Connect a device to inspect its Parameter registry.</div></div>
  {:else if loadingRegistry && rows.length === 0}
    <div class="empty-state"><i class="codicon codicon-loading codicon-modifier-spin"></i><div>Reading Parameter registry…</div></div>
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
                  {:else if cell.column.id === "name"}
                    <span>{row.meta.name ?? "—"}</span>
                  {:else if cell.column.id === "value"}
                    {#if row.pending}
                      <span class="pending-value"><i class="codicon codicon-loading codicon-modifier-spin"></i></span>
                    {:else if isWritable(row.meta)}
                      <div class="parameter-value-editor">
                        <input class="parameter-value-input mono" value={drafts[row.meta.id] ?? ""} disabled={writing.has(row.meta.id)} aria-label={`Value for ${row.meta.symbol}`} oninput={(event) => drafts = { ...drafts, [row.meta.id]: event.currentTarget.value }} onkeydown={(event) => handleValueKey(event, row)} />
                        <button class="cell-action" disabled={writing.has(row.meta.id)} onclick={() => void commitValue(row)} title="Write value (Enter)" aria-label={`Write ${row.meta.symbol}`}>
                          <i class={`codicon ${writing.has(row.meta.id) ? "codicon-loading codicon-modifier-spin" : "codicon-check"}`}></i>
                        </button>
                      </div>
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

<style>
  .parameter-toolbar { margin-left: auto; display: flex; align-items: center; gap: 8px; }
  .parameter-search { width: min(360px, 32vw); display: grid; grid-template-columns: 24px minmax(0, 1fr); align-items: center; border: 1px solid var(--vscode-input-border); border-radius: 2px; background: var(--vscode-input-background); }
  .parameter-search:focus-within { border-color: var(--vscode-focusBorder); }
  .parameter-search i { text-align: center; color: #838383; }
  .parameter-search .compact-input { border: 0; background: transparent; }
  .parameter-search .compact-input:focus { border: 0; }
  .parameter-count { min-width: 105px; color: #848484; font-size: 11px; text-align: right; }
  .tool-button, .cell-action, .table-header-button { border: 0; color: #c8c8c8; background: transparent; font: inherit; }
  .tool-button { height: 27px; display: inline-flex; align-items: center; gap: 6px; padding: 0 9px; border: 1px solid #3a3d42; border-radius: 2px; background: #2a2d32; }
  .tool-button:not(:disabled):hover, .cell-action:not(:disabled):hover { background: #383c43; }
  .tool-button:disabled, .cell-action:disabled, .table-header-button:disabled { opacity: .5; }
  .parameter-page { min-height: 0; padding: 0; overflow: hidden; }
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
  .parameter-value-editor { display: grid; grid-template-columns: minmax(90px, 1fr) 25px; gap: 4px; }
  .parameter-value-input { width: 100%; min-width: 0; height: 23px; padding: 1px 5px; border: 1px solid transparent; border-radius: 2px; outline: none; color: #d8d8d8; background: transparent; }
  .parameter-value-input:hover { border-color: #3a4049; background: #25292f; }
  .parameter-value-input:focus { border-color: var(--vscode-focusBorder); background: var(--vscode-input-background); }
  .cell-action { width: 25px; height: 23px; border-radius: 2px; }
  .access-badge { display: inline-flex; min-width: 28px; justify-content: center; padding: 1px 5px; border: 1px solid #3a3a3a; border-radius: 8px; color: #8d8d8d; font-size: 10px; line-height: 15px; }
  .access-badge.rw { color: #b5c2d4; border-color: #465265; background: #29303b; }
  .empty-state { height: 100%; display: grid; place-content: center; justify-items: center; gap: 10px; color: #777; }
  .empty-state i { font-size: 24px; }
  .parameter-no-results { height: 72px !important; color: #777 !important; text-align: center; }
</style>
