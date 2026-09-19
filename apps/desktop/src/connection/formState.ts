import { writable } from "svelte/store";

export const connectionPort = writable("");
export const connectionSchemaPath = writable("../../../AxDr_L_Motor/build/host/axdr-host-schema.toml");
