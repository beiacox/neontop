import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { settings } from "./settings.svelte";
import { Series, type SeriesLike } from "./series";
import type { Snapshot, SystemInfo } from "./types";

/**
 * Tope de muestras por serie. Al alcanzarlo, la mitad más antigua se compacta a la mitad
 * de resolución (media de pares), así que el historial abarca un tiempo ilimitado con
 * memoria acotada: lo reciente se conserva a resolución completa y lo antiguo se va
 * haciendo más grueso, como en una base de datos RRD.
 */
export const MAX_POINTS = 40_000;

/** Máximo de puntos que se dibujan por serie; por encima se agrupan en cubos. */
const MAX_DRAW = 1600;

/**
 * Intervalo mínimo entre repintados de la interfaz. El muestreo puede ir a 100 ms (el
 * historial guarda todas las muestras), pero repintar decenas de gráficas más de ~4 veces
 * por segundo solo quema CPU en WebKit sin que el ojo note la diferencia.
 */
export const UI_MIN_MS = 250;

export interface GpuHistory {
  busy: Series;
  memBusy: Series;
  vramBw: Series;
  vramUsed: Series;
  gttUsed: Series;
  power: Series;
  tempEdge: Series;
  tempHotspot: Series;
  tempMem: Series;
  gfxClock: Series;
  memClock: Series;
  pcieRx: Series;
  pcieTx: Series;
  fanPct: Series;
  /** Uso de cada motor (gfx, compute, dec…), por nombre. */
  engines: Map<string, Series>;
}

export interface DiskHistory {
  r: Series;
  w: Series;
  riops: Series;
  wiops: Series;
  util: Series;
  rlat: Series;
  wlat: Series;
  queue: Series;
}

export interface NetHistory {
  rx: Series;
  tx: Series;
  rxp: Series;
  txp: Series;
  /** Totales al ver la interfaz por primera vez, para contar el tráfico de la sesión. */
  baseRx: number;
  baseTx: number;
}

const CPU_KEYS = ["usage", "user", "nice", "system", "iowait", "irq", "softirq", "steal"] as const;

/** Reparto de la RAM al estilo htop; los segmentos suman `total`. */
export function memSegments(m: Snapshot["mem"]) {
  const cache = Math.max(0, m.cached + m.sReclaimable - m.shmem);
  const used = Math.max(0, m.total - m.free - m.buffers - cache);
  const apps = Math.max(0, used - m.shmem);
  return { apps, shmem: m.shmem, buffers: m.buffers, cache, free: m.free, used };
}


/**
 * Estado global. La historia se guarda en arrays planos (no reactivos) para no pagar
 * el coste de proxies en miles de puntos; los gráficos se redibujan al cambiar `tick`.
 */
class Metrics {
  info = $state.raw<SystemInfo | null>(null);
  snap = $state.raw<Snapshot | null>(null);
  tick = $state(0);
  /** Nº de compactaciones: si es > 0, lo más antiguo está a resolución reducida. */
  compactions = $state(0);

  /** Todas las series (incluida `t`), para compactarlas a la vez. */
  private all: Series[] = [];

  /** Crea una serie registrada, rellena con `len` ceros para alinearla con `t`. */
  private series(len = 0, f64 = false): Series {
    const a = new Series(len, f64);
    this.all.push(a);
    return a;
  }

  h = {
    t: this.series(0, true),
    cpu: Object.fromEntries(CPU_KEYS.map((k) => [k, this.series()])) as Record<(typeof CPU_KEYS)[number], Series>,
    cores: [] as Series[],
    freqs: [] as Series[],
    freqAvg: this.series(),
    temp: this.series(),
    load: [this.series(), this.series(), this.series()],
    ctxt: this.series(),
    intr: this.series(),
    fork: this.series(),
    running: this.series(),
    mem: { apps: this.series(), shmem: this.series(), buffers: this.series(), cache: this.series(), free: this.series() },
    swap: this.series(),
    dirty: this.series(),
    writeback: this.series(),
    /** Tráfico de la RAM en vivo (bytes/s). */
    memBw: { total: this.series(), read: this.series(), write: this.series() },
    psi: { cpu: this.series(), mem: this.series(), memFull: this.series(), io: this.series(), ioFull: this.series() },
    net: new Map<string, NetHistory>(),
    gpus: [] as GpuHistory[],
    disks: new Map<string, DiskHistory>(),
    /** Una serie por lectura de sensor, por id ("hwmon3:k10temp/temp1"). */
    sensors: new Map<string, Series>(),
    /** Potencia total medida y de cada fuente de energía, por id. */
    energyTotal: this.series(),
    /** SAI: tensión de red, carga de la batería y % de carga. */
    ups: { lineV: this.series(), battery: this.series(), load: this.series() },
    energy: new Map<string, Series>(),
    /** FPS y 1 % bajo (ventana móvil) de cada juego, por pid. Los rellena games.svelte.ts. */
    gameFps: new Map<number, Series>(),
    gameLow: new Map<number, Series>(),
  };

  /**
   * Serie alineada con `t` para datos que se calculan fuera del store (en un suscriptor de
   * onSample, cuando `t` ya tiene la muestra actual): nace con ceros hasta la muestra anterior.
   */
  extSeries<K>(map: Map<K, Series>, key: K): Series {
    let s = map.get(key);
    if (!s) map.set(key, (s = this.series(this.h.t.length - 1)));
    return s;
  }

  /** Suscriptores que reciben cada muestra (alertas…), también con la ventana oculta. */
  private listeners: ((s: Snapshot) => void)[] = [];
  onSample(fn: (s: Snapshot) => void) {
    this.listeners.push(fn);
  }

  private pending: Snapshot | null = null;
  private lastFlush = 0;
  private flushTimer: ReturnType<typeof setTimeout> | null = null;

  async start() {
    this.info = await invoke<SystemInfo>("get_system_info");
    settings.load();
    // la pausa también se puede cambiar desde el menú de la bandeja
    await listen<boolean>("paused-changed", (e) => (settings.paused = e.payload));
    // el muestreo también se puede cambiar desde la interfaz web
    await listen<number>("interval-changed", (e) => (settings.intervalMs = e.payload));
    // con la ventana oculta o minimizada no se repinta; al volver se muestra lo último
    document.addEventListener("visibilitychange", () => this.scheduleFlush());
    await listen<Snapshot>("metrics", (e) => this.ingest(e.payload));
  }

  /** Publica la última muestra a la UI respetando UI_MIN_MS. */
  private scheduleFlush() {
    if (!this.pending || this.flushTimer || document.hidden) return;
    const wait = Math.max(0, this.lastFlush + UI_MIN_MS - performance.now());
    this.flushTimer = setTimeout(() => {
      this.flushTimer = null;
      if (!this.pending || document.hidden) return;
      this.lastFlush = performance.now();
      this.snap = this.pending;
      this.pending = null;
      this.tick++;
    }, wait);
  }

  /** Índice del primer punto dentro de la ventana visible. */
  get startIdx(): number {
    const t = this.h.t;
    if (!t.length || !Number.isFinite(settings.windowSec)) return 0;
    const from = t.at(-1) - settings.windowSec;
    let lo = 0;
    let hi = t.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (t.at(mid) < from) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  /** Tamaño de cubo para que la ventana visible no pase de MAX_DRAW puntos. */
  get stride(): number {
    return Math.max(1, Math.ceil((this.h.t.length - this.startIdx) / MAX_DRAW));
  }

  /** Serie recortada a la ventana y reducida a ≤ MAX_DRAW puntos (media por cubo). */
  view(arr: SeriesLike): number[] {
    const start = this.startIdx;
    const k = this.stride;
    if (k === 1) return arr.slice(start);
    const src = arr instanceof Series ? arr.view() : arr;
    const out: number[] = [];
    for (let i = start; i < src.length; i += k) {
      const end = Math.min(i + k, src.length);
      let sum = 0;
      for (let j = i; j < end; j++) sum += src[j];
      out.push(sum / (end - i));
    }
    return out;
  }

  /** Serie recortada a la ventana, sin reducir (para picos exactos). */
  raw(arr: SeriesLike): number[] {
    return arr.slice(this.startIdx);
  }

  /** Muestras guardadas y marca de tiempo más antigua. */
  get stored() {
    return {
      points: this.h.t.length,
      since: this.h.t.length ? this.h.t.at(0) : 0,
      bytes: this.all.reduce((a, s) => a + s.bytes, 0),
      series: this.all.length,
    };
  }

  clearHistory() {
    for (const a of this.all) a.clear();
    this.compactions = 0;
    this.tick++;
  }

  private ingest(s: Snapshot) {
    const h = this.h;
    // las series que aparezcan ahora (núcleos, interfaces) se alinean con las `n` muestras previas
    const n = h.t.length;
    h.t.push(s.ts / 1000);

    for (const k of CPU_KEYS) h.cpu[k].push(s.cpu.total[k]);
    s.cpu.cores.forEach((c, i) => (h.cores[i] ??= this.series(n)).push(c.usage));
    s.cpu.freqs.forEach((f, i) => (h.freqs[i] ??= this.series(n)).push(f));
    h.freqAvg.push(s.cpu.freqs.reduce((a, b) => a + b, 0) / Math.max(1, s.cpu.freqs.length));
    h.temp.push(s.cpu.temp ?? 0);
    s.cpu.loadAvg.forEach((l, i) => h.load[i].push(l));
    h.ctxt.push(s.cpu.ctxtRate);
    h.intr.push(s.cpu.intrRate);
    h.fork.push(s.cpu.forkRate);
    h.running.push(s.cpu.procsRunning);

    const seg = memSegments(s.mem);
    h.mem.apps.push(seg.apps);
    h.mem.shmem.push(seg.shmem);
    h.mem.buffers.push(seg.buffers);
    h.mem.cache.push(seg.cache);
    h.mem.free.push(seg.free);
    h.swap.push(s.mem.swapTotal - s.mem.swapFree);
    h.dirty.push(s.mem.dirty);
    h.writeback.push(s.mem.writeback);
    h.memBw.total.push(s.membw?.total ?? 0);
    h.memBw.read.push(s.membw?.read ?? 0);
    h.memBw.write.push(s.membw?.write ?? 0);

    h.psi.cpu.push(s.pressure.cpu.some);
    h.psi.mem.push(s.pressure.memory.some);
    h.psi.memFull.push(s.pressure.memory.full);
    h.psi.io.push(s.pressure.io.some);
    h.psi.ioFull.push(s.pressure.io.full);

    s.gpus.forEach((g, i) => {
      const gh = (h.gpus[i] ??= {
        busy: this.series(n), memBusy: this.series(n), vramBw: this.series(n), vramUsed: this.series(n), gttUsed: this.series(n),
        power: this.series(n), tempEdge: this.series(n), tempHotspot: this.series(n), tempMem: this.series(n), gfxClock: this.series(n), memClock: this.series(n),
        pcieRx: this.series(n), pcieTx: this.series(n), fanPct: this.series(n), engines: new Map(),
      });
      gh.busy.push(g.busy ?? 0);
      gh.memBusy.push(g.memBusy ?? 0);
      gh.vramBw.push(g.vramBw ?? 0);
      gh.vramUsed.push(g.vramUsed);
      gh.gttUsed.push(g.gttUsed);
      gh.power.push(g.power ?? 0);
      gh.tempEdge.push(g.tempEdge ?? 0);
      gh.tempHotspot.push(g.tempHotspot ?? 0);
      gh.tempMem.push(g.tempMem ?? 0);
      gh.gfxClock.push(g.gfxClock ?? 0);
      gh.memClock.push(g.memClock ?? 0);
      gh.pcieRx.push(g.pcieRx ?? 0);
      gh.pcieTx.push(g.pcieTx ?? 0);
      gh.fanPct.push(g.fanPct ?? 0);
      // los motores pueden aparecer más tarde (p. ej. al empezar a medir fdinfo)
      const seenEng = new Set<string>();
      for (const e of g.engines) {
        seenEng.add(e.name);
        let arr = gh.engines.get(e.name);
        if (!arr) gh.engines.set(e.name, (arr = this.series(n)));
        arr.push(e.busy);
      }
      for (const [name, arr] of gh.engines) if (!seenEng.has(name)) arr.push(0);
    });

    // Discos y sensores pueden aparecer/desaparecer (USB, módulos): las series ausentes reciben 0
    const seenDisks = new Set<string>();
    for (const d of s.disks.disks) {
      seenDisks.add(d.name);
      let dh = h.disks.get(d.name);
      if (!dh) {
        dh = { r: this.series(n), w: this.series(n), riops: this.series(n), wiops: this.series(n), util: this.series(n), rlat: this.series(n), wlat: this.series(n), queue: this.series(n) };
        h.disks.set(d.name, dh);
      }
      dh.r.push(d.readRate);
      dh.w.push(d.writeRate);
      dh.riops.push(d.readIops);
      dh.wiops.push(d.writeIops);
      dh.util.push(d.util);
      dh.rlat.push(d.readLat);
      dh.wlat.push(d.writeLat);
      dh.queue.push(d.queue);
    }
    for (const [name, dh] of h.disks) {
      if (!seenDisks.has(name)) for (const a of Object.values(dh)) a.push(0);
    }

    const seenSensors = new Set<string>();
    for (const c of s.sensors.chips) {
      for (const r of c.readings) {
        seenSensors.add(r.id);
        let arr = h.sensors.get(r.id);
        if (!arr) h.sensors.set(r.id, (arr = this.series(n)));
        arr.push(r.value);
      }
    }
    for (const [id, arr] of h.sensors) if (!seenSensors.has(id)) arr.push(0);

    h.energyTotal.push(s.energy.totalW);
    h.ups.lineV.push(s.energy.ups?.lineV ?? 0);
    h.ups.battery.push(s.energy.ups?.batteryPct ?? 0);
    h.ups.load.push(s.energy.ups?.loadPct ?? 0);
    const seenEnergy = new Set<string>();
    for (const e of s.energy.sources) {
      seenEnergy.add(e.id);
      let arr = h.energy.get(e.id);
      if (!arr) h.energy.set(e.id, (arr = this.series(n)));
      arr.push(e.watts ?? 0);
    }
    for (const [id, arr] of h.energy) if (!seenEnergy.has(id)) arr.push(0);

    const seen = new Set<string>();
    for (const i of s.net.ifaces) {
      seen.add(i.name);
      let nh = h.net.get(i.name);
      if (!nh) {
        nh = { rx: this.series(n), tx: this.series(n), rxp: this.series(n), txp: this.series(n), baseRx: i.rxTotal, baseTx: i.txTotal };
        h.net.set(i.name, nh);
      }
      nh.rx.push(i.rxRate);
      nh.tx.push(i.txRate);
      nh.rxp.push(i.rxPktRate);
      nh.txp.push(i.txPktRate);
    }
    // Interfaces que han desaparecido (p. ej. un veth de Docker) siguen alineadas con ceros.
    for (const [name, nh] of h.net) {
      if (!seen.has(name)) for (const a of [nh.rx, nh.tx, nh.rxp, nh.txp]) a.push(0);
    }

    if (h.t.length >= MAX_POINTS) {
      const half = h.t.length >> 1;
      for (const a of this.all) a.compact(half & ~1);
      this.compactions++;
    }

    for (const fn of this.listeners) fn(s);

    this.pending = s;
    // la primera muestra se publica ya, para no esperar con la pantalla de carga
    if (!this.snap) {
      this.snap = s;
      this.pending = null;
      this.tick++;
    } else this.scheduleFlush();
  }
}

export const metrics = new Metrics();
