import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { memSegments } from "./store.svelte";
import type { Snapshot } from "./types";

const KEY = "neontop.alerts";
const OPTS_KEY = "neontop.alerts.opts";

/** Dónde avisa una regla. */
export type Channel = "both" | "app" | "system";

export interface AlertRule {
  id: string;
  label: string;
  unit: string;
  enabled: boolean;
  threshold: number;
  /** Segundos que debe mantenerse por encima del umbral antes de avisar. */
  sustain: number;
  channel: Channel;
  /** Minutos entre avisos mientras siga por encima; 0 = una sola vez hasta que se resuelva. */
  repeat: number;
  /** Silenciada hasta este instante (ms). */
  mutedUntil: number;
  /** Elementos que no se vigilan (puntos de montaje en «disco lleno», discos en su temperatura). */
  ignore: string[];
  /** Avisa cuando el valor BAJA del umbral (batería del SAI). */
  below?: boolean;
  /** Regla de sí/no (sin umbral editable): avisa cuando el valor es 1. */
  flag?: boolean;
}

/** Un valor vigilado: `key` distingue varios elementos de una misma regla (cada disco). */
interface Item {
  key: string;
  value: number;
  detail?: string;
}

/** Cómo obtener los valores de cada regla a partir de una muestra. */
const PROBES: Record<string, (s: Snapshot) => Item[]> = {
  cpuTemp: (s) => (s.cpu.temp != null ? [{ key: "", value: s.cpu.temp }] : []),
  gpuTemp: (s) => {
    const g = s.gpus.find((x) => x.tempHotspot != null || x.tempEdge != null);
    return g ? [{ key: "", value: g.tempHotspot ?? g.tempEdge!, detail: g.name }] : [];
  },
  cpuUsage: (s) => [{ key: "", value: s.cpu.total.usage }],
  ram: (s) => [{ key: "", value: (memSegments(s.mem).used / s.mem.total) * 100 }],
  swap: (s) => (s.mem.swapTotal ? [{ key: "", value: ((s.mem.swapTotal - s.mem.swapFree) / s.mem.swapTotal) * 100 }] : []),
  // cada sistema de ficheros por separado: uno lleno no tapa a los demás
  diskFull: (s) => s.disks.filesystems.map((f) => ({ key: f.mount, value: (f.used / Math.max(1, f.total)) * 100, detail: f.mount })),
  diskTemp: (s) => s.disks.disks.filter((d) => d.temps.length).map((d) => ({ key: d.name, value: Math.max(...d.temps.map((t) => t.celsius)), detail: d.name })),
  // SAI (apcupsd): corte de luz y batería baja
  upsOnBatt: (s) => {
    const u = s.energy.ups;
    return u ? [{ key: "", value: u.onBattery ? 1 : 0, detail: `batería ${u.batteryPct?.toFixed(0) ?? "?"} % · ${u.runtimeMin?.toFixed(0) ?? "?"} min de autonomía` }] : [];
  },
  upsBattery: (s) => {
    const u = s.energy.ups;
    return u?.batteryPct != null ? [{ key: "", value: u.batteryPct, detail: `${u.runtimeMin?.toFixed(0) ?? "?"} min de autonomía` }] : [];
  },
};

/** Reglas que vigilan varios elementos (se pueden ignorar uno a uno). */
export const MULTI: Record<string, string> = { diskFull: "sistemas de ficheros", diskTemp: "discos" };

const base = { channel: "both" as Channel, mutedUntil: 0, ignore: [] as string[] };
const DEFAULTS: AlertRule[] = [
  { ...base, id: "cpuTemp", label: "Temperatura de CPU", unit: "°C", enabled: true, threshold: 85, sustain: 10, repeat: 5 },
  { ...base, id: "gpuTemp", label: "Temperatura de GPU (unión)", unit: "°C", enabled: true, threshold: 95, sustain: 10, repeat: 5 },
  { ...base, id: "cpuUsage", label: "Uso de CPU", unit: "%", enabled: false, threshold: 95, sustain: 60, repeat: 15 },
  { ...base, id: "ram", label: "Uso de RAM", unit: "%", enabled: true, threshold: 90, sustain: 15, repeat: 15 },
  { ...base, id: "swap", label: "Uso de swap", unit: "%", enabled: false, threshold: 50, sustain: 30, repeat: 15 },
  // un disco lleno no se arregla solo en 5 minutos: se avisa una vez
  { ...base, id: "diskFull", label: "Sistema de ficheros lleno", unit: "%", enabled: true, threshold: 90, sustain: 0, repeat: 0 },
  { ...base, id: "diskTemp", label: "Temperatura de disco", unit: "°C", enabled: true, threshold: 70, sustain: 30, repeat: 15 },
  { ...base, id: "upsOnBatt", label: "Corte de luz (SAI en batería)", unit: "", enabled: true, threshold: 1, sustain: 0, repeat: 0, flag: true },
  { ...base, id: "upsBattery", label: "Batería del SAI baja", unit: "%", enabled: true, threshold: 30, sustain: 0, repeat: 0, below: true },
];

/** Margen para dar por resuelta una alerta (evita avisos en bucle si el valor ronda el umbral). */
const HYSTERESIS: Record<string, number> = { "%": 2, "°C": 3 };

export interface Fired {
  id: number;
  rule: string;
  /** Elemento concreto (punto de montaje, disco) o "". */
  key: string;
  title: string;
  body: string;
  ts: number;
}

export interface AlertOpts {
  /** Notificaciones del escritorio. */
  desktop: boolean;
  /** Avisos emergentes dentro de NeonTop. */
  toasts: boolean;
  /** Todo silenciado hasta este instante (ms). */
  muteAll: number;
}

class Alerts {
  rules = $state<AlertRule[]>(load());
  opts = $state<AlertOpts>(loadOpts());
  /** Avisos recientes (para el registro de Ajustes). */
  log = $state<Fired[]>([]);
  toasts = $state<Fired[]>([]);
  private since = new Map<string, number>();
  private lastFired = new Map<string, number>();
  private seq = 0;

  save() {
    localStorage.setItem(KEY, JSON.stringify(this.rules));
    localStorage.setItem(OPTS_KEY, JSON.stringify(this.opts));
  }

  update(id: string, patch: Partial<AlertRule>) {
    this.rules = this.rules.map((r) => (r.id === id ? { ...r, ...patch } : r));
    this.save();
  }

  setOpts(patch: Partial<AlertOpts>) {
    this.opts = { ...this.opts, ...patch };
    this.save();
  }

  reset() {
    this.rules = DEFAULTS.map((r) => ({ ...r }));
    this.opts = { desktop: true, toasts: true, muteAll: 0 };
    this.save();
  }

  dismiss(id: number) {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  /** Deja de vigilar un elemento concreto (p. ej. un disco que ya sabes que está lleno). */
  ignore(rule: string, key: string) {
    const r = this.rules.find((x) => x.id === rule);
    if (r && key && !r.ignore.includes(key)) this.update(rule, { ignore: [...r.ignore, key] });
    this.toasts = this.toasts.filter((t) => !(t.rule === rule && t.key === key));
  }

  unignore(rule: string, key: string) {
    const r = this.rules.find((x) => x.id === rule);
    if (r) this.update(rule, { ignore: r.ignore.filter((k) => k !== key) });
  }

  /** Silencia una regla `minutes` minutos (0 = reactivar). */
  mute(rule: string, minutes: number) {
    this.update(rule, { mutedUntil: minutes ? Date.now() + minutes * 60_000 : 0 });
    if (minutes) this.toasts = this.toasts.filter((t) => t.rule !== rule);
  }

  muteAll(until: number) {
    this.setOpts({ muteAll: until });
    if (until > Date.now()) this.toasts = [];
  }

  /** Elementos que vigila ahora una regla con varios (para elegir cuáles ignorar). */
  items(rule: string, s: Snapshot | null): Item[] {
    return s ? (PROBES[rule]?.(s) ?? []) : [];
  }

  /** Se llama con cada muestra (aunque la ventana esté oculta). */
  check(s: Snapshot) {
    const now = s.ts;
    const allMuted = now < this.opts.muteAll;
    for (const r of this.rules) {
      if (!r.enabled) continue;
      const items = PROBES[r.id]?.(s) ?? [];
      const hyst = HYSTERESIS[r.unit] ?? 0;
      for (const it of items) {
        const k = `${r.id}|${it.key}`;
        if (r.ignore.includes(it.key)) continue;
        // `below`: el problema es estar por DEBAJO del umbral (batería)
        const over = r.below ? it.value <= r.threshold : it.value >= r.threshold;
        if (!over) {
          this.since.delete(k);
          // se da por resuelta cuando se aleja del umbral con margen: entonces puede volver a avisar
          const cleared = r.below ? it.value > r.threshold + hyst : it.value < r.threshold - hyst;
          if (r.flag || cleared) this.lastFired.delete(k);
          continue;
        }
        if (!this.since.has(k)) this.since.set(k, now);
        const held = (now - this.since.get(k)!) / 1000;
        const last = this.lastFired.get(k);
        const due = last == null || (r.repeat > 0 && now - last >= r.repeat * 60_000);
        if (held < r.sustain || !due) continue;
        // silenciada: se cuenta como avisada para no soltar todo de golpe al terminar el silencio
        this.lastFired.set(k, now);
        if (allMuted || now < r.mutedUntil) continue;
        const value = `${it.value.toFixed(r.unit === "%" ? 0 : 1)}${r.unit}`;
        if (r.flag) this.fire(r, it.key, `⚠ ${r.label}`, it.detail ?? "");
        else this.fire(r, it.key, `⚠ ${r.label}: ${value}`, `${it.detail ? `${it.detail} · ` : ""}umbral ${r.below ? "<" : ">"} ${r.threshold}${r.unit}${r.sustain ? ` durante ${r.sustain} s` : ""}`);
      }
    }
  }

  private async fire(r: AlertRule, key: string, title: string, body: string) {
    const f: Fired = { id: ++this.seq, rule: r.id, key, title, body, ts: Date.now() };
    this.log = [f, ...this.log].slice(0, 50);
    if (this.opts.toasts && r.channel !== "system") {
      this.toasts = [...this.toasts, f].slice(-4);
      setTimeout(() => this.dismiss(f.id), 15_000);
    }
    if (!this.opts.desktop || r.channel === "app") return;
    try {
      let ok = await isPermissionGranted();
      if (!ok) ok = (await requestPermission()) === "granted";
      if (ok) sendNotification({ title: `NeonTop · ${title}`, body });
    } catch {
      /* sin servicio de notificaciones: queda el aviso dentro de la app */
    }
  }
}

function load(): AlertRule[] {
  try {
    const saved: Partial<AlertRule>[] = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    // se combinan con los valores por defecto: reglas y campos nuevos de versiones posteriores
    return DEFAULTS.map((d) => ({ ...d, ...(saved.find((s) => s.id === d.id) ?? {}), label: d.label, unit: d.unit, below: d.below, flag: d.flag }));
  } catch {
    return DEFAULTS.map((r) => ({ ...r }));
  }
}

function loadOpts(): AlertOpts {
  try {
    return { desktop: true, toasts: true, muteAll: 0, ...JSON.parse(localStorage.getItem(OPTS_KEY) ?? "{}") };
  } catch {
    return { desktop: true, toasts: true, muteAll: 0 };
  }
}

export const alerts = new Alerts();
