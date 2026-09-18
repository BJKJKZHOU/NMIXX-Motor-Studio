import { invoke } from "@tauri-apps/api/core";

export type IdentificationPreflightKind = "rsLs" | "flux" | "jb";

export type PreflightIssue = {
  parameterId: number | null;
  reason: string;
  suggestedDomain: "limits" | "motor" | "identification";
};

export function checkIdentificationPreflight(
  kind: IdentificationPreflightKind,
): Promise<PreflightIssue[]> {
  const wireKind = kind === "rsLs" ? "rs_ls" : kind;
  return invoke<PreflightIssue[]>("identification_preflight", { kind: wireKind });
}
