import { invoke } from "@tauri-apps/api/core";
import { metrics } from "./store.svelte";
import { C } from "./theme";
import type { EnergyKind, EnergySource, GpuSnapshot, ProcInfo, Snapshot } from "./types";

const KEY = "neontop.energy";
/** Días de histórico que se conservan. */
const KEEP_DAYS = 400;
/** Cada cuánto se guarda el histórico (y al cerrar). */
const SAVE_MS = 30_000;

/** Energía de un día (Wh), en total, por fuente y por hora local. */
export interface DayEnergy {
  wh: number;
  src: Record<string, number>;
  hours: number[];
}

export const KIND_LABEL: Record<EnergyKind, string> = {
  cpu: "CPU",
  "cpu-core": "Núcleos",
  uncore: "Uncore",
  dram: "RAM",
  platform: "Plataforma",
  gpu: "GPU",
  igpu: "iGPU",
  sensor: "Sensor",
  psu: "Fuente",
  battery: "Batería",
  charge: "Carga",
  ups: "SAI",
};

export const kindColor = (k: EnergyKind): string =>
  ({
    cpu: C.cyan,
    "cpu-core": C.teal,
    uncore: C.violet,
    dram: C.lime,
    platform: C.amber,
    gpu: C.magenta,
    igpu: C.violet,
    sensor: C.orange,
    psu: C.amber,
    battery: C.lime,
    charge: C.lime,
    ups: C.amber,
  })[k];

/** Id del «resto del puesto» (SAI − componentes medidos) en el historial diario. */
export const REST_ID = "ups:rest";
export const REST_LABEL = "Resto del puesto";
export const REST_COLOR = () => C.violet;

/** Fecha local "AAAA-MM-DD". */
export function dayKey(d = new Date()): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export function fmtWatts(w: number | null | undefined): string {
  if (w == null) return "N/D";
  return w >= 100 ? `${w.toFixed(0)} W` : `${w.toFixed(1)} W`;
}

export function fmtWh(wh: number): string {
  if (wh >= 1000) return `${(wh / 1000).toFixed(2)} kWh`;
  if (wh >= 10) return `${wh.toFixed(0)} Wh`;
  return `${wh.toFixed(2)} Wh`;
}

/** Potencia estimada de un proceso (o grupo de procesos con el mismo nombre). */
export interface ProcPower {
  name: string;
  count: number;
  cpu: number;
  cpuW: number | null;
  gpuW: number | null;
  totalW: number;
}

/** Potencia de la CPU: RAPL si hay permiso; si no, sensores hwmon de la CPU (zenpower). */
export function cpuWatts(s: Snapshot): number | null {
  const pkg = s.energy.sources.filter((x) => x.kind === "cpu" && x.watts != null);
  return pkg.length ? pkg.reduce((a, x) => a + x.watts!, 0) : null;
}

const gpuBusyOf = (engines: { busy: number }[]) => engines.reduce((a, e) => a + e.busy, 0);

/**
 * Reparte la potencia medida entre procesos: la de la CPU según el tiempo de CPU de cada uno,
 * la de cada GPU según el uso de sus motores. Es una estimación (incluye el consumo en reposo
 * repartido proporcionalmente), pero ordena bien quién está gastando.
 */
export function estimateProcs(s: Snapshot, list: ProcInfo[], gpus: GpuSnapshot[]): ProcPower[] {
  const cpuW = cpuWatts(s);
  const cpuSum = list.reduce((a, p) => a + p.cpu, 0);
  const rows = new Map<string, ProcPower>();
  const row = (name: string) => {
    let r = rows.get(name);
    if (!r) rows.set(name, (r = { name, count: 0, cpu: 0, cpuW: cpuW == null ? null : 0, gpuW: null, totalW: 0 }));
    return r;
  };
  for (const p of list) {
    if (p.cpu <= 0) continue;
    const r = row(p.name);
    r.count++;
    r.cpu += p.cpu;
    if (cpuW != null && cpuSum > 0) r.cpuW! += (cpuW * p.cpu) / cpuSum;
  }
  for (const g of gpus) {
    if (g.power == null || !g.processesSampled) continue;
    const busySum = g.processes.reduce((a, p) => a + gpuBusyOf(p.engines), 0);
    if (busySum <= 0) continue;
    for (const p of g.processes) {
      const b = gpuBusyOf(p.engines);
      if (b <= 0) continue;
      const r = row(p.name);
      if (!r.count) r.count = 1;
      r.gpuW = (r.gpuW ?? 0) + (g.power * b) / busySum;
    }
  }
  for (const r of rows.values()) r.totalW = (r.cpuW ?? 0) + (r.gpuW ?? 0);
  return [...rows.values()].sort((a, b) => b.totalW - a.totalW || b.cpu - a.cpu);
}

/**
 * Estado de la pestaña Energía: precio de la electricidad, histórico diario persistente
 * (se acumula siempre que NeonTop esté midiendo, aunque la pestaña no esté abierta) y
 * estadísticas de la sesión.
 */
class Energy {
  /** Precio por kWh y símbolo de moneda. */
  price = $state(0.15);
  currency = $state("$");
  /** Se incrementa al cambiar el histórico diario (para repintar sus gráficas). */
  version = $state(0);

  days: Record<string, DayEnergy> = {};
  /** Nombre legible de cada fuente, para los días en que ya no esté presente. */
  labels: Record<string, string> = {};

  /** Sesión (desde que arrancó NeonTop). */
  startTs = 0;
  peakW = $state(0);
  minW = $state(Infinity);

  /** Energía estimada acumulada por proceso (J), mientras se mide la lista de procesos. */
  procJ = new Map<string, number>();
  private procLast = 0;

  private prevTotal: number | null = null;
  private prevSrc: Record<string, number> = {};
  private lastSave = 0;
  private lastBump = 0;

  init() {
    try {
      const d = JSON.parse(localStorage.getItem(KEY) ?? "{}");
      if (typeof d.price === "number") this.price = d.price;
      if (typeof d.currency === "string") this.currency = d.currency;
      if (d.days && typeof d.days === "object") this.days = d.days;
      if (d.labels && typeof d.labels === "object") this.labels = d.labels;
    } catch {
      /* histórico corrupto: se empieza de cero */
    }
    metrics.onSample((s) => this.ingest(s));
    addEventListener("pagehide", () => this.save());
    addEventListener("beforeunload", () => this.save());
  }

  setPrice(price: number, currency: string) {
    this.price = Math.max(0, price);
    this.currency = currency.slice(0, 5);
    this.save();
  }

  cost(wh: number) {
    return (wh / 1000) * this.price;
  }

  fmtCost(wh: number) {
    const c = this.cost(wh);
    return `${this.currency}${c < 1 ? c.toFixed(3) : c.toFixed(2)}`;
  }

  /** Dar o quitar permiso de lectura a RAPL (pide contraseña con polkit). */
  raplAccess(enable: boolean, persistent: boolean) {
    return invoke<void>("rapl_access", { enable, persistent });
  }

  /** Acumula en el día y la hora actuales la energía gastada desde la muestra anterior. */
  private ingest(s: Snapshot) {
    const e = s.energy;
    if (!this.startTs) this.startTs = s.ts;
    if (e.totalMethod !== "none") {
      this.peakW = Math.max(this.peakW, e.totalW);
      if (e.totalW > 0) this.minW = Math.min(this.minW, e.totalW);
    }
    const now = new Date(s.ts);
    const key = dayKey(now);
    const day = (this.days[key] ??= { wh: 0, src: {}, hours: new Array(24).fill(0) });
    if (this.prevTotal != null) {
      const d = e.totalJ - this.prevTotal;
      if (d > 0) {
        day.wh += d / 3600;
        day.hours[now.getHours()] += d / 3600;
      }
    }
    this.prevTotal = e.totalJ;
    let partsWh = 0;
    let upsWh: number | null = null;
    for (const x of e.sources) {
      this.labels[x.id] = x.label;
      const prev = this.prevSrc[x.id];
      const d = prev != null && x.joules > prev ? (x.joules - prev) / 3600 : 0;
      // por día solo se guardan las fuentes que suman al total (sus barras se apilan)
      if (x.inTotal && d) {
        day.src[x.id] = (day.src[x.id] ?? 0) + d;
        partsWh += d;
      }
      if (x.kind === "ups" && prev != null) upsWh = d;
      this.prevSrc[x.id] = x.joules;
    }
    // con SAI, lo que mide de más que los componentes es el resto del puesto (monitores, placa…)
    if (upsWh != null && upsWh > partsWh) {
      day.src[REST_ID] = (day.src[REST_ID] ?? 0) + (upsWh - partsWh);
      this.labels[REST_ID] = REST_LABEL;
    }
    const t = performance.now();
    if (t - this.lastBump > 2000) {
      this.lastBump = t;
      this.version++;
    }
    if (t - this.lastSave > SAVE_MS) this.save();
  }

  /** Suma la energía estimada de cada proceso desde la última lista. */
  accumulateProcs(rows: ProcPower[]) {
    const t = performance.now();
    const dt = this.procLast ? (t - this.procLast) / 1000 : 0;
    this.procLast = t;
    // un hueco largo (pestaña cerrada, pausa) no se cuenta
    if (dt <= 0 || dt > 5) return;
    for (const r of rows) if (r.totalW > 0) this.procJ.set(r.name, (this.procJ.get(r.name) ?? 0) + r.totalW * dt);
  }

  /** Energía (Wh) del mes actual. */
  monthWh(d = new Date()) {
    const prefix = dayKey(d).slice(0, 7);
    return Object.entries(this.days).reduce((a, [k, v]) => (k.startsWith(prefix) ? a + v.wh : a), 0);
  }

  clearHistory() {
    this.days = {};
    this.procJ.clear();
    this.save();
    this.version++;
  }

  save() {
    this.lastSave = performance.now();
    const keys = Object.keys(this.days).sort();
    for (const k of keys.slice(0, Math.max(0, keys.length - KEEP_DAYS))) delete this.days[k];
    try {
      localStorage.setItem(KEY, JSON.stringify({ price: this.price, currency: this.currency, days: this.days, labels: this.labels }));
    } catch {
      /* sin espacio: se reintenta en el próximo guardado */
    }
  }
}

export const energy = new Energy();

/** Fuentes de primer nivel (sin padre) ordenadas: primero las que suman al total. */
export function topSources(list: EnergySource[]) {
  return list.filter((s) => !s.parent).sort((a, b) => Number(b.inTotal) - Number(a.inTotal) || (b.watts ?? 0) - (a.watts ?? 0));
}
