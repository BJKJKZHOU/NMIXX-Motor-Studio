import type { ParameterMetadata, ParameterReadResult, ParameterValue } from "./types";

const metadataById = new Map<number, ParameterMetadata>();
const metadataBySymbol = new Map<string, ParameterMetadata>();
const valuesById = new Map<number, ParameterValue>();

export function observeParameterMetadata(entries: ParameterMetadata[]): void {
  for (const entry of entries) {
    metadataById.set(entry.id, entry);
    metadataBySymbol.set(entry.symbol, entry);
  }
}

export function observeSessionParameterResults(results: ParameterReadResult[]): void {
  for (const result of results) {
    if (result.value) valuesById.set(result.id, result.value);
  }
}

export function observeSessionParameterValue(id: number, value: ParameterValue): void {
  valuesById.set(id, value);
}

export function parameterMetadataSnapshot(symbols?: Iterable<string>): Record<string, ParameterMetadata> {
  if (!symbols) {
    return Object.fromEntries(Array.from(metadataBySymbol.entries()));
  }

  const result: Record<string, ParameterMetadata> = {};
  for (const symbol of symbols) {
    const metadata = metadataBySymbol.get(symbol);
    if (metadata) result[symbol] = metadata;
  }
  return result;
}

export function parameterValueSnapshot(symbols?: Iterable<string>): Record<string, ParameterValue | null> {
  const metadata = symbols
    ? Array.from(symbols, (symbol) => metadataBySymbol.get(symbol)).filter((item): item is ParameterMetadata => !!item)
    : Array.from(metadataById.values());

  return Object.fromEntries(
    metadata.map((item) => [item.symbol, valuesById.get(item.id) ?? null]),
  );
}

export function parameterValueById(id: number): ParameterValue | undefined {
  return valuesById.get(id);
}

export function parameterValueText(value: ParameterValue | null | undefined): string {
  if (!value) return "";
  if (value.type === "position") return `${value.value.turns}, ${value.value.theta}`;
  if (value.type === "f32") {
    return Number(value.value).toPrecision(7).replace(/(?:\.0+|(\.\d+?)0+)$/, "$1");
  }
  return String(value.value);
}

export function parameterDraftSnapshot(symbols: Iterable<string>): Record<string, string> {
  const values = parameterValueSnapshot(symbols);
  return Object.fromEntries(
    Object.entries(values)
      .filter(([, value]) => value !== null)
      .map(([symbol, value]) => [symbol, parameterValueText(value)]),
  );
}

export function clearParameterSession(): void {
  metadataById.clear();
  metadataBySymbol.clear();
  valuesById.clear();
}
