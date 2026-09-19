import { writable } from "svelte/store";

export type EncoderViewState = {
  protocol?: number;
  spiType?: number;
  motorDirection?: "normal" | "reversed";
};

export const encoderViewState = writable<EncoderViewState>({});
