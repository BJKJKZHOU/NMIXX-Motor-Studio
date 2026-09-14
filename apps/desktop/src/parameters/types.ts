export type ParameterTypeName = "u8" | "i8" | "f32" | "i32" | "u32" | "position";

export type PositionValue = {
  turns: number;
  theta: number;
};

export type ParameterValue =
  | { type: "u8"; value: number }
  | { type: "i8"; value: number }
  | { type: "f32"; value: number }
  | { type: "i32"; value: number }
  | { type: "u32"; value: number }
  | { type: "position"; value: PositionValue };

export type ParameterRange = {
  min: number | null;
  max: number | null;
  exclusiveMin: boolean;
  exclusiveMax: boolean;
  maxSymbol: string | null;
  maxBinding: string | null;
  maxBindings: string[];
};

export type ParameterMetadata = {
  id: number;
  symbol: string;
  name: string | null;
  typeName: ParameterTypeName;
  access: string;
  unit: string | null;
  description: string;
  writeState: string | null;
  range: ParameterRange | null;
  allowed: number[];
  allowedSymbols: string[];
};

export type ParameterRead = {
  id: number;
  value: ParameterValue;
};
