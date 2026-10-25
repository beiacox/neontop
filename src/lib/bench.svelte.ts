import { invoke } from "@tauri-apps/api/core";
import type { MemBench } from "./types";

const KEY = "neontop.membench";

/** Resultados del test de RAM (se guardan los últimos 10 entre sesiones). */
class Bench {
  runs = $state<MemBench[]>(load());
  running = $state(false);
  error = $state<string | null>(null);

  get last(): MemBench | undefined {
    return this.runs[this.runs.length - 1];
  }

  async run() {
    if (this.running) return;
    this.running = true;
    this.error = null;
    try {
      const r = await invoke<MemBench>("bench_memory");
      this.runs = [...this.runs, r].slice(-10);
      localStorage.setItem(KEY, JSON.stringify(this.runs));
    } catch (e) {
      this.error = String(e);
    } finally {
      this.running = false;
    }
  }
}

function load(): MemBench[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(v) ? v : [];
  } catch {
    return [];
  }
}

export const bench = new Bench();
