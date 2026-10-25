import { invoke } from "@tauri-apps/api/core";

export type CollectKind = "procs" | "gpu_procs";

const wanted: Record<CollectKind, Set<string>> = { procs: new Set(), gpu_procs: new Set() };

/**
 * Registra (o libera) una vista que necesita una recogida cara del backend. La recogida
 * se activa mientras al menos una vista la pida.
 */
export function want(kind: CollectKind, who: string, on: boolean) {
  const set = wanted[kind];
  const before = set.size > 0;
  if (on) set.add(who);
  else set.delete(who);
  const after = set.size > 0;
  if (before !== after) invoke("set_collect", { kind, enabled: after });
}
