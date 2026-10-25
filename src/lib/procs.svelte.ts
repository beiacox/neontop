import { invoke } from "@tauri-apps/api/core";
import { want } from "./collect";
import { settings } from "./settings.svelte";
import { listen } from "@tauri-apps/api/event";
import type { ProcInfo } from "./types";

/** Muestras por proceso para las mini-gráficas y el detalle. */
const HIST = 120;

export interface ProcHistory {
  start: number;
  cpu: number[];
  mem: number[];
  io: number[];
}

/** Descripción de los estados de /proc/pid/stat. */
export const STATES: Record<string, { label: string; color: "cyan" | "lime" | "amber" | "red" | "violet" | "muted" | "magenta" }> = {
  R: { label: "Ejecutando", color: "lime" },
  S: { label: "Durmiendo", color: "cyan" },
  D: { label: "Espera de disco", color: "amber" },
  I: { label: "Inactivo (kernel)", color: "muted" },
  Z: { label: "Zombi", color: "red" },
  T: { label: "Detenido", color: "violet" },
  t: { label: "Trazado", color: "violet" },
  X: { label: "Muerto", color: "red" },
};

/**
 * Lista de procesos. El backend solo la recoge mientras alguna vista la pide (`want`),
 * porque recorrer /proc es bastante más caro que el resto de métricas.
 */
class Procs {
  list = $state.raw<ProcInfo[]>([]);
  tick = $state(0);
  hist = new Map<number, ProcHistory>();

  async init() {
    document.addEventListener("visibilitychange", () => !document.hidden && this.flush());
    await listen<{ procs: ProcInfo[] }>("procs", (e) => this.ingest(e.payload.procs));
  }

  /** Registra (o libera) una vista que necesita procesos. */
  want(who: string, on: boolean) {
    want("procs", who, on);
  }

  signal(pid: number, signal: "TERM" | "KILL" | "INT" | "HUP" | "STOP" | "CONT") {
    return invoke<void>("send_signal", { pid, signal, elevate: settings.adminActions });
  }

  renice(pid: number, nice: number) {
    return invoke<void>("renice_process", { pid, nice, elevate: settings.adminActions });
  }

  private ingest(list: ProcInfo[]) {
    const alive = new Set<number>();
    for (const p of list) {
      alive.add(p.pid);
      let h = this.hist.get(p.pid);
      // pid reutilizado por otro proceso: se empieza de cero
      if (!h || h.start !== p.start) {
        h = { start: p.start, cpu: [], mem: [], io: [] };
        this.hist.set(p.pid, h);
      }
      h.cpu.push(p.cpu);
      h.mem.push(p.mem);
      h.io.push((p.readRate ?? 0) + (p.writeRate ?? 0));
      if (h.cpu.length > HIST) {
        h.cpu.shift();
        h.mem.shift();
        h.io.shift();
      }
    }
    for (const pid of this.hist.keys()) if (!alive.has(pid)) this.hist.delete(pid);
    // con la ventana oculta no hace falta repintar la tabla (el historial sí se actualiza)
    this.pendingList = list;
    if (!document.hidden) this.flush();
  }

  private pendingList: ProcInfo[] | null = null;

  flush() {
    if (!this.pendingList) return;
    this.list = this.pendingList;
    this.pendingList = null;
    this.tick++;
  }
}

export const procs = new Procs();
