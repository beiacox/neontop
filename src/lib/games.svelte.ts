import { invoke } from "@tauri-apps/api/core";
import { metrics } from "./store.svelte";
import type { GameSnapshot, Snapshot } from "./types";

/** Resolución del histograma de tiempos de fotograma (ms) y tope (lo que pase va al último cubo). */
const BIN_MS = 0.05;
const MAX_MS = 250;
const NBINS = Math.ceil(MAX_MS / BIN_MS) + 1;
/** Fotogramas recientes que se guardan por juego (gráfica por fotograma y mediana móvil). */
const RECENT = 6000;
/** Ventana del 1 % bajo "en vivo" (s). */
const LIVE_WINDOW = 5;
const BENCH_KEY = "neontop.benchmarks";

/**
 * Estadísticas acumuladas de tiempos de fotograma. Los percentiles salen de un histograma de
 * 0,05 ms, así una sesión de horas ocupa lo mismo que una de segundos y el 1 % / 0,1 % bajo
 * son exactos a 0,05 ms.
 */
export class FrameStats {
  bins = new Uint32Array(NBINS);
  count = 0;
  /** Suma de tiempos (ms) = duración cubierta. */
  sum = 0;
  min = Infinity;
  max = 0;
  /** Tirones: fotogramas de más del doble de la mediana reciente (y ≥ 4 ms más). */
  stutters = 0;
  /** Parones: fotogramas de más de 50 ms y 3 veces la mediana. */
  hitches = 0;
  /** Suma de |ft[i] − ft[i−1]|: irregularidad del ritmo de fotogramas. */
  jitter = 0;
  private last = 0;

  add(ft: number, median: number) {
    this.bins[Math.min(NBINS - 1, Math.floor(ft / BIN_MS))]++;
    this.count++;
    this.sum += ft;
    if (ft < this.min) this.min = ft;
    if (ft > this.max) this.max = ft;
    if (median > 0) {
      if (ft > median * 2 && ft - median >= 4) this.stutters++;
      if (ft > 50 && ft > median * 3) this.hitches++;
    }
    if (this.last) this.jitter += Math.abs(ft - this.last);
    this.last = ft;
  }

  /** Tiempo de fotograma (ms) del percentil `p` (0–100): p = 99 → base del 1 % bajo. */
  percentile(p: number): number {
    if (!this.count) return 0;
    const target = Math.ceil((p / 100) * this.count);
    let acc = 0;
    for (let i = 0; i < NBINS; i++) {
      acc += this.bins[i];
      if (acc >= target) return (i + 0.5) * BIN_MS;
    }
    return MAX_MS;
  }

  get avgFps() {
    return this.sum > 0 ? (this.count * 1000) / this.sum : 0;
  }
  /** 1 % bajo: FPS equivalente al percentil 99 del tiempo de fotograma (como MangoHud / CapFrameX). */
  get low1() {
    const p = this.percentile(99);
    return p ? 1000 / p : 0;
  }
  get low01() {
    const p = this.percentile(99.9);
    return p ? 1000 / p : 0;
  }
  /** Variación media entre fotogramas consecutivos (ms). */
  get jitterMs() {
    return this.count > 1 ? this.jitter / (this.count - 1) : 0;
  }
  get seconds() {
    return this.sum / 1000;
  }

  /**
   * Estabilidad 0–100: 70 % constancia (1 % bajo frente a la media) + 30 % suavidad del ritmo
   * (variación entre fotogramas relativa al tiempo medio), menos una penalización por parones.
   */
  get stability(): number {
    if (this.count < 30) return 0;
    const avgFt = this.sum / this.count;
    const consistency = Math.min(1, this.low1 / this.avgFps);
    const smooth = Math.max(0, 1 - this.jitterMs / avgFt);
    const hitchPenalty = Math.min(0.3, (this.hitches / Math.max(1, this.seconds / 60)) * 0.03);
    return Math.max(0, Math.min(100, (consistency * 0.7 + smooth * 0.3 - hitchPenalty) * 100));
  }

  /** Histograma agrupado en cubos de `step` ms hasta `upTo` ms (para dibujar). */
  grouped(step: number, upTo: number): number[] {
    const per = Math.round(step / BIN_MS);
    const n = Math.ceil(upTo / step);
    const out = new Array(n).fill(0);
    for (let i = 0; i < NBINS; i++) out[Math.min(n - 1, Math.floor(i / per))] += this.bins[i];
    return out;
  }
}

export function stabilityLabel(score: number, frames = Infinity): { text: string; color: "lime" | "cyan" | "amber" | "red" | "muted" } {
  if (frames < 30) return { text: "sin datos", color: "muted" };
  if (score >= 90) return { text: "Excelente", color: "lime" };
  if (score >= 75) return { text: "Buena", color: "cyan" };
  if (score >= 55) return { text: "Irregular", color: "amber" };
  return { text: "Mala", color: "red" };
}

const newRes = (): ResAcc => ({ n: 0, cpu: 0, cpuMaxCore: 0, gpu: 0, gpuPower: 0, cpuTempMax: 0, gpuTempMax: 0 });

/** Recursos del sistema durante una grabación o partida (medias y máximos). */
interface ResAcc {
  n: number;
  cpu: number;
  cpuMaxCore: number;
  gpu: number;
  gpuPower: number;
  cpuTempMax: number;
  gpuTempMax: number;
}

export interface Benchmark {
  id: string;
  /** "bench": grabación manual; "session": partida completa, guardada al cerrar el juego. */
  kind?: "bench" | "session";
  title: string;
  exe: string;
  api: string;
  date: number;
  seconds: number;
  frames: number;
  avg: number;
  low1: number;
  low01: number;
  minFps: number;
  maxFps: number;
  stutters: number;
  hitches: number;
  stability: number;
  jitterMs: number;
  cpu: number;
  gpu: number;
  gpuPower: number;
  cpuTempMax: number;
  gpuTempMax: number;
  /** Tiempos de fotograma de la grabación (ms), para exportar. Solo en memoria. */
  frametimes?: number[];
}

/** Series de una partida guardada, para volver a dibujarla (ver `Timeline.export`). */
export interface SessionDetail {
  v: 1;
  /** Inicio (ms desde epoch); `t` son segundos desde ahí. */
  t0: number;
  t: number[];
  fps: number[];
  low1: number[];
  /** Peor tiempo de fotograma de cada punto (ms): los tirones se ven como picos. */
  ftMax: number[];
  cpu: number[];
  gpu: number[];
  gpuPower: number[];
  gpuTemp: number[];
  cpuTemp: number[];
  /** Histograma de tiempos de fotograma: cubos de `step` ms (el último recoge el resto). */
  hist: { step: number; counts: number[] };
  /** Percentiles del tiempo de fotograma (ms). */
  pct: { p50: number; p90: number; p95: number; p99: number; p999: number; max: number; median: number };
  /** Cada fotograma (solo benchmarks), para exportar a CSV. */
  frametimes?: number[];
}

const TL_KEYS = ["fps", "low1", "ftMax", "cpu", "gpu", "gpuPower", "gpuTemp", "cpuTemp"] as const;
type TlKey = (typeof TL_KEYS)[number];
/** Series en las que al agrupar puntos se conserva el máximo (no la media). */
const TL_MAX = new Set<TlKey>(["ftMax", "gpuTemp", "cpuTemp"]);
/** Puntos en memoria por partida; al llegar se agrupan de dos en dos (como el historial general). */
const TL_CAP = 20_000;
/** Puntos que se guardan en disco por registro. */
const TL_SAVE = 1500;

/** Evolución de una partida muestra a muestra. */
export class Timeline {
  t: number[] = [];
  s: Record<TlKey, number[]> = Object.fromEntries(TL_KEYS.map((k) => [k, []])) as unknown as Record<TlKey, number[]>;

  push(t: number, v: Record<TlKey, number>) {
    this.t.push(t);
    for (const k of TL_KEYS) this.s[k].push(v[k]);
    if (this.t.length >= TL_CAP) this.compact();
  }

  get length() {
    return this.t.length;
  }

  private compact() {
    const pair = (arr: number[], max: boolean) => {
      const out: number[] = [];
      for (let i = 0; i + 1 < arr.length; i += 2) out.push(max ? Math.max(arr[i], arr[i + 1]) : (arr[i] + arr[i + 1]) / 2);
      if (arr.length % 2) out.push(arr[arr.length - 1]);
      return out;
    };
    this.t = pair(this.t, false);
    for (const k of TL_KEYS) this.s[k] = pair(this.s[k], TL_MAX.has(k));
  }

  /** Reduce a ≤ `max` puntos (media por cubo; máximo en tirones y temperaturas) desde `from`. */
  export(from = 0, max = TL_SAVE): Omit<SessionDetail, "v" | "hist" | "pct" | "frametimes"> {
    const n = this.t.length - from;
    const k = Math.max(1, Math.ceil(n / max));
    const t0 = n > 0 ? this.t[from] : Date.now() / 1000;
    const bucket = (arr: number[], useMax: boolean) => {
      const out: number[] = [];
      for (let i = from; i < arr.length; i += k) {
        const end = Math.min(arr.length, i + k);
        let acc = useMax ? -Infinity : 0;
        for (let j = i; j < end; j++) acc = useMax ? Math.max(acc, arr[j]) : acc + arr[j];
        out.push(Math.round((useMax ? acc : acc / (end - i)) * 100) / 100);
      }
      return out;
    };
    const tt = bucket(this.t, false).map((x) => Math.round((x - t0) * 10) / 10);
    const series = Object.fromEntries(TL_KEYS.map((key) => [key, bucket(this.s[key], TL_MAX.has(key))])) as Record<TlKey, number[]>;
    return { t0: t0 * 1000, t: tt, ...series };
  }
}

function detailOf(tl: Timeline, from: number, st: FrameStats, frametimes?: number[]): SessionDetail {
  return {
    v: 1,
    ...tl.export(from),
    hist: { step: 0.25, counts: st.grouped(0.25, 100) },
    pct: { p50: st.percentile(50), p90: st.percentile(90), p95: st.percentile(95), p99: st.percentile(99), p999: st.percentile(99.9), max: st.max, median: st.percentile(50) },
    frametimes: frametimes?.slice(0, 300_000),
  };
}

export class GameTrack {
  pid: number;
  info = $state.raw<GameSnapshot>() as GameSnapshot;
  /** Últimos tiempos de fotograma (anillo) y su instante (s, reloj de la interfaz). */
  recent = new Float32Array(RECENT);
  recentAt = new Float64Array(RECENT);
  n = 0;
  session = new FrameStats();
  /** FPS por segundo mínimo/máximo de la sesión. */
  minFps = Infinity;
  maxFps = 0;
  /** Recursos del sistema durante toda la partida (mientras el juego está activo). */
  res: ResAcc = newRes();
  /** Instante (ms) en que el juego se cerró; null mientras sigue abierto. */
  ended: number | null = null;
  /** Evolución de la partida (para guardarla completa en el historial). */
  timeline = new Timeline();
  /** Id del registro guardado al cerrar el juego. */
  savedId: string | null = null;
  /** Grabación de benchmark en curso; `from` = primer punto de la línea de tiempo. */
  rec: { stats: FrameStats; frames: number[]; res: ResAcc; start: number; until: number | null; minFps: number; maxFps: number; from: number } | null = null;

  constructor(g: GameSnapshot) {
    this.pid = g.pid;
    this.info = g;
  }

  /** Mediana de los últimos `k` fotogramas. */
  median(k = 120): number {
    const m = Math.min(this.n, k, RECENT);
    if (!m) return 0;
    const a = new Float32Array(m);
    for (let i = 0; i < m; i++) a[i] = this.recent[(this.n - 1 - i) % RECENT];
    a.sort();
    return a[m >> 1];
  }

  /** Últimos fotogramas en orden, con su tiempo relativo al último (s, negativo). */
  lastFrames(seconds: number): { x: number[]; y: number[] } {
    const x: number[] = [];
    const y: number[] = [];
    if (!this.n) return { x, y };
    const end = this.recentAt[(this.n - 1) % RECENT];
    const m = Math.min(this.n, RECENT);
    for (let i = this.n - m; i < this.n; i++) {
      const t = this.recentAt[i % RECENT] - end;
      if (t < -seconds) continue;
      x.push(t);
      y.push(this.recent[i % RECENT]);
    }
    return { x, y };
  }

  /** 1 % bajo de los últimos `seconds` segundos. */
  liveLow(seconds = LIVE_WINDOW): number {
    const { y } = this.lastFrames(seconds);
    if (y.length < 20) return 0;
    y.sort((a, b) => a - b);
    return 1000 / y[Math.min(y.length - 1, Math.ceil(y.length * 0.99) - 1)];
  }

  ingest(g: GameSnapshot, now: number) {
    this.info = g;
    const med = this.median();
    // los fotogramas de esta muestra se reparten hacia atrás desde `now` según su duración
    let t = now - g.frametimes.reduce((a, b) => a + b, 0) / 1000;
    for (const ft of g.frametimes) {
      t += ft / 1000;
      const i = this.n % RECENT;
      this.recent[i] = ft;
      this.recentAt[i] = t;
      this.n++;
      this.session.add(ft, med);
      if (this.rec) {
        this.rec.stats.add(ft, med);
        this.rec.frames.push(ft);
      }
    }
    if (g.active && g.fps > 0) {
      this.minFps = Math.min(this.minFps, g.fps);
      this.maxFps = Math.max(this.maxFps, g.fps);
      if (this.rec) {
        this.rec.minFps = Math.min(this.rec.minFps, g.fps);
        this.rec.maxFps = Math.max(this.rec.maxFps, g.fps);
      }
    }
  }

  resetSession() {
    this.session = new FrameStats();
    this.minFps = Infinity;
    this.maxFps = 0;
    this.res = newRes();
    this.timeline = new Timeline();
    // también la gráfica por fotograma y el 1 % bajo en vivo
    this.n = 0;
  }
}

/**
 * Juegos medidos con la capa NeonTop FPS. Se procesan todas las muestras (también con la
 * pestaña cerrada o la ventana oculta) para que las estadísticas de la sesión sean completas.
 */
class Games {
  tracks = new Map<number, GameTrack>();
  /** pid del juego que se muestra. */
  selected = $state<number | null>(null);
  version = $state(0);
  benchmarks = $state.raw<Benchmark[]>([]);
  /** Última grabación terminada (se muestra su resumen). */
  lastBench = $state.raw<Benchmark | null>(null);

  /** Registro del historial que se está viendo (id) o null. */
  viewing = $state<string | null>(null);
  private details = new Map<string, SessionDetail>();

  init() {
    metrics.onSample((s) => this.ingest(s));
    this.loadList();
  }

  /** El historial está en el disco del equipo (también para la interfaz web). */
  async loadList() {
    try {
      let list = (await invoke<Benchmark[] | null>("sessions_list")) ?? [];
      // las versiones anteriores lo guardaban en el navegador: se pasa al disco una vez
      const old = localStorage.getItem(BENCH_KEY);
      if (!list.length && old) {
        const items = JSON.parse(old) as Benchmark[];
        if (items.length && (await invoke<number | null>("sessions_import", { items }))) list = (await invoke<Benchmark[] | null>("sessions_list")) ?? [];
      }
      if (list.length && old) localStorage.removeItem(BENCH_KEY);
      this.benchmarks = list;
    } catch {
      this.benchmarks = [];
    }
  }

  /** Datos completos de un registro (se cargan del disco la primera vez). */
  async detail(id: string): Promise<SessionDetail | null> {
    if (this.details.has(id)) return this.details.get(id)!;
    try {
      const d = await invoke<SessionDetail | null>("session_load", { id });
      if (d) this.details.set(id, d);
      return d;
    } catch {
      return null;
    }
  }

  get list(): GameTrack[] {
    this.version;
    const rank = (t: GameTrack) => (t.info.active ? 2 : t.ended ? 0 : 1);
    return [...this.tracks.values()].sort((a, b) => rank(b) - rank(a) || (b.ended ?? 0) - (a.ended ?? 0) || b.info.fps - a.info.fps);
  }

  get current(): GameTrack | null {
    this.version;
    return (this.selected != null && this.tracks.get(this.selected)) || this.list[0] || null;
  }

  private ingest(s: Snapshot) {
    const now = s.ts / 1000;
    const seen = new Set<number>();
    for (const g of s.games.games) {
      seen.add(g.pid);
      let tr = this.tracks.get(g.pid);
      // un pid que vuelve tras cerrarse es otro proceso: se empieza de cero
      if (!tr || tr.ended) this.tracks.set(g.pid, (tr = new GameTrack(g)));
      tr.ingest(g, now);
      metrics.extSeries(metrics.h.gameFps, g.pid).push(g.active ? g.fps : 0);
      metrics.extSeries(metrics.h.gameLow, g.pid).push(g.active ? tr.liveLow() : 0);
      if (g.active) {
        this.sampleResources(tr.res, s);
        const gpu = s.gpus.find((x) => !x.integrated) ?? s.gpus[0];
        tr.timeline.push(now, {
          fps: g.fps,
          low1: tr.liveLow(),
          ftMax: g.frametimes.length ? Math.max(...g.frametimes) : 0,
          cpu: s.cpu.total.usage,
          gpu: gpu?.busy ?? 0,
          gpuPower: gpu?.power ?? 0,
          gpuTemp: gpu?.tempHotspot ?? gpu?.tempEdge ?? 0,
          cpuTemp: s.cpu.temp ?? 0,
        });
      }
      if (tr.rec) this.sampleResources(tr.rec.res, s);
    }
    // juegos que ya no están: se conservan sus series (con ceros) y sus estadísticas
    for (const [pid, arr] of metrics.h.gameFps) if (!seen.has(pid)) arr.push(0);
    for (const [pid, arr] of metrics.h.gameLow) if (!seen.has(pid)) arr.push(0);
    for (const [pid, tr] of this.tracks) {
      if (!seen.has(pid) && !tr.ended) this.end(tr);
      if (tr.rec?.until && now >= tr.rec.until) this.stop(tr);
    }
    this.version++;
  }

  /** El juego se ha cerrado: se termina la grabación y se guarda la partida en el historial. */
  private end(tr: GameTrack) {
    tr.ended = Date.now();
    tr.info = { ...tr.info, active: false, fps: 0 };
    if (tr.rec) this.stop(tr);
    const st = tr.session;
    // partidas de menos de 10 s (lanzadores, pantallas de carga) no se guardan
    if (st.count >= 100 && st.seconds >= 10) {
      const id = Math.random().toString(36).slice(2, 10);
      tr.savedId = id;
      this.save_(
        {
          id,
          kind: "session",
          title: tr.info.title,
          exe: tr.info.exe,
          api: tr.info.api,
          date: tr.ended,
          seconds: st.seconds,
          frames: st.count,
          avg: st.avgFps,
          low1: st.low1,
          low01: st.low01,
          minFps: Number.isFinite(tr.minFps) ? tr.minFps : 0,
          maxFps: tr.maxFps,
          stutters: st.stutters,
          hitches: st.hitches,
          stability: st.stability,
          jitterMs: st.jitterMs,
          cpu: tr.res.cpu / Math.max(1, tr.res.n),
          gpu: tr.res.gpu / Math.max(1, tr.res.n),
          gpuPower: tr.res.gpuPower / Math.max(1, tr.res.n),
          cpuTempMax: tr.res.cpuTempMax,
          gpuTempMax: tr.res.gpuTempMax,
        },
        detailOf(tr.timeline, 0, st),
      );
    }
  }

  /** Quita de la vista un juego ya cerrado. */
  dismiss(pid: number) {
    const tr = this.tracks.get(pid);
    if (tr?.ended) this.tracks.delete(pid);
    if (this.selected === pid) this.selected = null;
    this.version++;
  }

  /** Añade un registro al historial (resumen en el índice, datos completos en su fichero). */
  private save_(summary: Benchmark, detail: SessionDetail) {
    const { frametimes: _, ...sum } = summary;
    this.benchmarks = [sum, ...this.benchmarks];
    this.details.set(sum.id, detail);
    invoke("session_save", { summary: sum, detail }).catch(() => {});
  }

  private sampleResources(r: ResAcc, s: Snapshot) {
    const g = s.gpus.find((x) => !x.integrated) ?? s.gpus[0];
    const cpuT = s.cpu.temp ?? 0;
    r.n++;
    r.cpu += s.cpu.total.usage;
    r.cpuMaxCore += Math.max(0, ...s.cpu.cores.map((c) => c.usage));
    r.gpu += g?.busy ?? 0;
    r.gpuPower += g?.power ?? 0;
    r.cpuTempMax = Math.max(r.cpuTempMax, cpuT);
    r.gpuTempMax = Math.max(r.gpuTempMax, g?.tempHotspot ?? g?.tempEdge ?? 0);
  }

  /** Empieza a grabar un benchmark (`seconds` = null: hasta que se detenga a mano). */
  start(tr: GameTrack, seconds: number | null) {
    const now = (metrics.snap?.ts ?? Date.now()) / 1000;
    tr.rec = {
      stats: new FrameStats(),
      frames: [],
      res: newRes(),
      start: now,
      until: seconds ? now + seconds : null,
      minFps: Infinity,
      maxFps: 0,
      from: tr.timeline.length,
    };
    this.version++;
  }

  stop(tr: GameTrack) {
    const rec = tr.rec;
    tr.rec = null;
    if (!rec || rec.stats.count < 30) {
      this.version++;
      return;
    }
    const st = rec.stats;
    const r = rec.res;
    const n = Math.max(1, r.n);
    const b: Benchmark = {
      id: Math.random().toString(36).slice(2, 10),
      kind: "bench",
      title: tr.info.title,
      exe: tr.info.exe,
      api: tr.info.api,
      date: Date.now(),
      seconds: st.seconds,
      frames: st.count,
      avg: st.avgFps,
      low1: st.low1,
      low01: st.low01,
      minFps: Number.isFinite(rec.minFps) ? rec.minFps : 0,
      maxFps: rec.maxFps,
      stutters: st.stutters,
      hitches: st.hitches,
      stability: st.stability,
      jitterMs: st.jitterMs,
      cpu: r.cpu / n,
      gpu: r.gpu / n,
      gpuPower: r.gpuPower / n,
      cpuTempMax: r.cpuTempMax,
      gpuTempMax: r.gpuTempMax,
      frametimes: rec.frames,
    };
    this.lastBench = b;
    this.save_(b, detailOf(tr.timeline, rec.from, st, rec.frames));
    this.version++;
  }

  remove(id: string) {
    this.benchmarks = this.benchmarks.filter((b) => b.id !== id);
    if (this.lastBench?.id === id) this.lastBench = null;
    if (this.viewing === id) this.viewing = null;
    this.details.delete(id);
    invoke("session_delete", { id }).catch(() => {});
  }
}

export const games = new Games();

/** CSV con los tiempos de fotograma de una grabación (de memoria o de su fichero). */
export async function benchCsv(b: Benchmark): Promise<string> {
  const frames = b.frametimes ?? (await games.detail(b.id))?.frametimes ?? [];
  const lines = ["fotograma,tiempo_ms,fps"];
  frames.forEach((ft, i) => lines.push(`${i + 1},${ft.toFixed(3)},${(1000 / ft).toFixed(1)}`));
  return lines.join("\n") + "\n";
}
