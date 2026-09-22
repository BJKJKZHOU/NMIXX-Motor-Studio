import type { ParameterMetadata, ParameterValue } from "./types";

export function parameterText(value: ParameterValue | null | undefined): string {
  if (!value) return "";
  if (value.type === "position") return `${value.value.turns}, ${value.value.theta}`;
  if (value.type === "f32") return Number(value.value).toPrecision(7).replace(/(?:\.0+|(\.\d+?)0+)$/, "$1");
  return String(value.value);
}

export function parseParameterText(meta: ParameterMetadata, text: string): ParameterValue {
  const trimmed = text.trim();
  if (!trimmed) throw new Error(`${meta.label}: enter a value.`);
  if (meta.typeName === "position") {
    const parts = trimmed.split(/[,:]/).map((part) => part.trim());
    if (parts.length !== 2 || parts.some((part) => !part)) {
      throw new Error(`${meta.label}: enter 'turns, theta'.`);
    }
    const turns = Number(parts[0]);
    const theta = Number(parts[1]);
    if (!Number.isInteger(turns) || turns < -2147483648 || turns > 2147483647
      || !Number.isFinite(Math.fround(theta))) {
      throw new Error(`${meta.label}: invalid position.`);
    }
    return { type: "position", value: { turns, theta } };
  }
  const value = Number(trimmed);
  if (!Number.isFinite(value)) throw new Error(`${meta.label}: value must be finite.`);
  if (meta.typeName === "f32") {
    if (!Number.isFinite(Math.fround(value))) throw new Error(`${meta.label}: value is outside f32 range.`);
    return { type: "f32", value };
  }
  const ranges = { u8: [0, 255], i8: [-128, 127], u32: [0, 4294967295], i32: [-2147483648, 2147483647] };
  const [min, max] = ranges[meta.typeName];
  if (!Number.isInteger(value) || value < min || value > max) {
    throw new Error(`${meta.label}: expected ${meta.typeName}.`);
  }
  return { type: meta.typeName, value };
}

export function equalParameterValue(a: ParameterValue | null | undefined, b: ParameterValue | null | undefined): boolean {
  if (!a || !b) return a === b;
  if (a.type !== b.type) return false;
  if (a.type === "position") {
    return b.type === "position" && a.value.turns === b.value.turns && a.value.theta === b.value.theta;
  }
  return a.value === b.value;
}
