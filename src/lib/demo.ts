/**
 * Modo demo: si la interfaz se abre fuera de Tauri (en un navegador, p. ej. `npm run dev`),
 * se simula el backend con datos sintéticos. Sirve para revisar temas y maquetación sin
 * la app nativa. Dentro de Tauri no hace nada.
 *
 * Parámetros de URL: ?theme=<id>&tab=<pestaña>
 */
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { EnergySource, GameSnapshot, ProcInfo, Snapshot, SystemInfo } from "./types";


const GB = 1024 ** 3;
const NCORES = 12;

const info: SystemInfo = {
  hostname: "demo",
  os: "NeonTop (modo demo)",
  kernel: "6.x-demo",
  cpuModel: "CPU de demostración · 6 núcleos",
  logicalCores: NCORES,
  physicalCores: 6,
  freqMin: Array(NCORES).fill(550),
  freqMax: Array(NCORES).fill(4650),
  memTotal: 32 * GB,
  bootTime: Math.floor(Date.now() / 1000) - 9000,
};

let t = 0;
let intervalMs = 1000;
let rx = 0,
  tx = 0,
  dr = 0,
  dw = 0;
/** Energía acumulada simulada por fuente (J). */
const ej: Record<string, number> = {};

/** Onda suave con algo de ruido, 0..1. */
const wave = (speed: number, phase = 0, noise = 0.15) =>
  Math.min(1, Math.max(0, 0.5 + 0.35 * Math.sin(t * speed + phase) + 0.15 * Math.sin(t * speed * 3.1 + phase * 2) + (Math.random() - 0.5) * noise));

function snapshot(): Snapshot {
  t += intervalMs / 1000;
  const load = wave(0.08);
  const cores = Array.from({ length: NCORES }, (_, i) => {
    const u = Math.min(100, Math.max(1, load * 70 * wave(0.2, i) + (i % 5 === 0 ? 25 * wave(0.5, i) : 0)));
    return { usage: u, user: u * 0.7, nice: u * 0.02, system: u * 0.22, iowait: u * 0.03, irq: u * 0.01, softirq: u * 0.02, steal: 0 };
  });
  const avg = (k: keyof (typeof cores)[0]) => cores.reduce((a, c) => a + c[k], 0) / NCORES;
  const cpuTemp = 42 + 38 * load;
  const rxRate = 2e6 * wave(0.3, 1, 0.6) ** 2 + 5e4;
  const txRate = 4e5 * wave(0.25, 2, 0.6) ** 2 + 1e4;
  rx += rxRate * (intervalMs / 1000);
  tx += txRate * (intervalMs / 1000);
  const readRate = 60e6 * wave(0.15, 3, 0.4) ** 3;
  const writeRate = 25e6 * wave(0.12, 4, 0.4) ** 3;
  dr += readRate;
  dw += writeRate;
  const gpuBusy = 100 * wave(0.1, 5) ** 2;
  const memUsed = (9 + 5 * load) * GB;
  const pkgW = 18 + 70 * load;
  const coreW = pkgW * 0.72;
  const gpuW = 20 + gpuBusy * 1.4;
  const dt = intervalMs / 1000;
  const src = (id: string, label: string, kind: EnergySource["kind"], watts: number, extra: Partial<EnergySource> = {}): EnergySource => {
    ej[id] = (ej[id] ?? 0) + watts * dt;
    return { id, label, kind, watts, joules: ej[id], limit: null, method: "counter", inTotal: false, system: false, parent: null, ...extra };
  };
  const sources = [
    src("rapl:intel-rapl:0", "CPU (paquete)", "cpu", pkgW, { inTotal: true, limit: 88 }),
    src("rapl:intel-rapl:0:0", "Núcleos de CPU", "cpu-core", coreW, { parent: "rapl:intel-rapl:0" }),
    src("gpu:0000:03:00.0", "GPU de demostración", "gpu", gpuW, { inTotal: true, limit: 160, method: "sensor" }),
  ];
  ej.total = (ej.total ?? 0) + (pkgW + gpuW) * dt;

  return {
    ts: Date.now(),
    uptime: 9000 + t,
    cpu: {
      total: { usage: avg("usage"), user: avg("user"), nice: avg("nice"), system: avg("system"), iowait: avg("iowait"), irq: avg("irq"), softirq: avg("softirq"), steal: 0 },
      cores,
      freqs: cores.map((c) => 2000 + c.usage * 26),
      temp: cpuTemp,
      temps: [
        { label: "Tctl", celsius: cpuTemp },
        { label: "Tccd1", celsius: cpuTemp - 3 },
      ],
      loadAvg: [load * 8, load * 6, load * 4],
      procsRunning: 1 + Math.round(load * 6),
      procsBlocked: 0,
      procsTotal: 480,
      ctxtRate: 20000 + load * 40000,
      intrRate: 15000 + load * 20000,
      forkRate: Math.round(load * 30),
    },
    mem: {
      total: 32 * GB, free: 32 * GB - memUsed - 11 * GB, available: 32 * GB - memUsed, buffers: 0.2 * GB, cached: 10 * GB, sReclaimable: 0.8 * GB,
      shmem: 0.7 * GB, swapTotal: 16 * GB, swapFree: 15.5 * GB, swapCached: 0.05 * GB, dirty: 2e6 * wave(0.4), writeback: 0,
      active: 12 * GB, inactive: 8 * GB, activeAnon: 7 * GB, inactiveAnon: 1 * GB, activeFile: 5 * GB, inactiveFile: 7 * GB, slab: 1.5 * GB,
      kernelStack: 40e6, pageTables: 120e6, mapped: 2 * GB, anonPages: 8 * GB, zswap: 0, zswapped: 0, committedAs: 20 * GB, commitLimit: 32 * GB,
    },
    net: {
      ifaces: [
        { name: "wlan0", kind: "wifi", state: "up", mac: "00:11:22:33:44:55", mtu: 1500, speed: null, addrs: ["192.168.1.20"], rxRate, txRate, rxPktRate: rxRate / 900, txPktRate: txRate / 600, rxTotal: 8e9 + rx, txTotal: 1e9 + tx, rxErrs: 0, txErrs: 0, rxDrop: 3, txDrop: 0 },
        { name: "enp3s0", kind: "ethernet", state: "down", mac: "00:11:22:33:44:66", mtu: 1500, speed: null, addrs: [], rxRate: 0, txRate: 0, rxPktRate: 0, txPktRate: 0, rxTotal: 0, txTotal: 0, rxErrs: 0, txErrs: 0, rxDrop: 0, txDrop: 0 },
        { name: "lo", kind: "loopback", state: "unknown", mac: "00:00:00:00:00:00", mtu: 65536, speed: null, addrs: ["127.0.0.1"], rxRate: 2000, txRate: 2000, rxPktRate: 10, txPktRate: 10, rxTotal: 1e8, txTotal: 1e8, rxErrs: 0, txErrs: 0, rxDrop: 0, txDrop: 0 },
      ],
      tcpEstablished: 42,
      tcpListen: 9,
      udpSockets: 14,
    },
    pressure: { cpu: { some: load * 4, full: 0 }, memory: { some: 0.1, full: 0 }, io: { some: wave(0.2) * 2, full: wave(0.2) } },
    gpus: [
      {
        name: "GPU de demostración", vendor: "amd", pciSlot: "0000:03:00.0", driver: "amdgpu", driverVersion: null, integrated: false,
        busy: gpuBusy, memBusy: gpuBusy * 0.4, vramUsed: (3 + 4 * wave(0.05)) * GB, vramTotal: 16 * GB, gttUsed: 0.3 * GB, gttTotal: 16 * GB,
        vramType: "GDDR6", busWidth: 128, memClock: 1250, memClockMax: 1258, vramPeakBw: 322e9, vramBw: 322e9 * gpuBusy * 0.004,
        pcieSpeed: 16, pcieWidth: 16, pcieMaxSpeed: 32, pcieMaxWidth: 16, pciePeakBw: 31.5e9, pcieRx: null, pcieTx: null,
        gfxClock: 500 + gpuBusy * 28, gfxClockMax: 3200, power: 20 + gpuBusy * 1.4, powerLimit: 160,
        tempEdge: 40 + gpuBusy * 0.35, tempHotspot: 44 + gpuBusy * 0.45, tempMem: 46, fanRpm: Math.round(gpuBusy * 18), fanPct: gpuBusy * 0.6, throttle: [],
        engines: [
          { name: "gfx", busy: gpuBusy },
          { name: "enc", busy: 6 * wave(0.3) },
          { name: "dec", busy: 3 * wave(0.2) },
        ],
        processes: [
          { pid: 4210, name: "juego", engines: [{ name: "gfx", busy: gpuBusy * 0.8 }], vram: 3.1 * GB, gtt: 0.1 * GB },
          { pid: 1880, name: "firefox", engines: [{ name: "gfx", busy: 2 }], vram: 0.45 * GB, gtt: 0.03 * GB },
          { pid: 1201, name: "plasmashell", engines: [{ name: "gfx", busy: 0.5 }], vram: 0.25 * GB, gtt: 0.01 * GB },
        ],
        processesSampled: true,
      },
    ],
    disks: {
      disks: [
        { name: "nvme0n1", model: "SSD NVMe de demostración", kind: "nvme", size: 1e12, removable: false, readRate, writeRate, readIops: readRate / 4e4, writeIops: writeRate / 3e4, util: Math.min(100, (readRate + writeRate) / 1e6), readLat: 0.2, writeLat: 0.4, queue: 0.3, readTotal: dr, writeTotal: dw, temps: [{ label: "Composite", celsius: 38 + load * 12 }], partitions: ["nvme0n1p1", "nvme0n1p2"] },
        { name: "sda", model: "HDD de demostración", kind: "hdd", size: 2e12, removable: false, readRate: readRate * 0.1, writeRate: 0, readIops: 3, writeIops: 0, util: 4 * wave(0.3), readLat: 8, writeLat: 0, queue: 0.05, readTotal: dr / 10, writeTotal: 0, temps: [], partitions: ["sda1"] },
      ],
      filesystems: [
        { device: "/dev/nvme0n1p2", disk: "nvme0n1", mount: "/", otherMounts: ["/home"], fstype: "btrfs", total: 950e9, used: 510e9, avail: 440e9, inodesTotal: 0, inodesFree: 0, readonly: false },
        { device: "/dev/nvme0n1p1", disk: "nvme0n1", mount: "/boot", otherMounts: [], fstype: "vfat", total: 1e9, used: 0.2e9, avail: 0.8e9, inodesTotal: 0, inodesFree: 0, readonly: false },
        { device: "/dev/sda1", disk: "sda", mount: "/datos", otherMounts: [], fstype: "ext4", total: 2e12, used: 1.84e12, avail: 0.16e12, inodesTotal: 1.2e8, inodesFree: 1e8, readonly: false },
      ],
    },
    sensors: {
      chips: [
        { id: "hwmon0:k10temp", driver: "k10temp", label: "CPU", category: "cpu", readings: [{ id: "hwmon0:k10temp/temp1", label: "Tctl", kind: "temp", value: cpuTemp, max: null, crit: null }] },
        {
          id: "hwmon1:amdgpu", driver: "amdgpu", label: "GPU", category: "gpu",
          readings: [
            { id: "hwmon1:amdgpu/temp1", label: "edge", kind: "temp", value: 40 + gpuBusy * 0.35, max: null, crit: 100 },
            { id: "hwmon1:amdgpu/temp2", label: "junction", kind: "temp", value: 44 + gpuBusy * 0.45, max: null, crit: 110 },
            { id: "hwmon1:amdgpu/fan1", label: "fan1", kind: "fan", value: Math.round(gpuBusy * 18), max: 3500, crit: null },
            { id: "hwmon1:amdgpu/power1_average", label: "PPT", kind: "power", value: 20 + gpuBusy * 1.4, max: null, crit: null },
          ],
        },
        { id: "hwmon2:nvme", driver: "nvme", label: "NVMe de demostración", category: "disk", readings: [{ id: "hwmon2:nvme/temp1", label: "Composite", kind: "temp", value: 38 + load * 12, max: 84.85, crit: 94.85 }] },
      ],
      batteries: [],
      acOnline: null,
    },
    // ?fin=<s>: el juego simulado se cierra a los N segundos (para probar el fin de partida)
    // ?membw=off: simula el estado sin permiso (para revisar el botón de activar)
    membw: (() => {
      if (membwOff) return { status: "nomodule" as const, message: "falta el módulo amd_uncore", source: null, read: null, write: null, total: null, enabled: false };
      const read = 4e9 + 14e9 * load + 2e9 * Math.random();
      const write = 1.5e9 + 6e9 * load + 1e9 * Math.random();
      return { status: "ok" as const, message: null, source: "intel_imc" as const, read, write, total: read + write, enabled: true };
    })(),
    games: { games: t < gameEnd ? [demoGame()] : [], layerInstalled: true },
    energy: (() => {
      // SAI simulado: el PC + ~45 W de monitores y resto, en saltos de 9 W (carga en % enteros)
      const load = Math.round(((pkgW + gpuW + 45) / 900) * 100);
      const upsW = (load / 100) * 900;
      const upsSrc = src("ups:apcupsd", "SAI · Back-UPS XS 1500M", "ups", upsW, { system: true, limit: 900, method: "sensor" });
      ej.total = (ej.total ?? 0) + (upsW - pkgW - gpuW) * dt;
      return {
        sources: [...sources, upsSrc], totalW: upsW, totalJ: ej.total, totalMethod: "ups" as const, rapl: "ok" as const, raplRule: true,
        ups: {
          source: "apcupsd", model: "Back-UPS XS 1500M", status: "ONLINE", onBattery: false, lowBattery: false, loadPct: load, nominalW: 900, powerW: upsW,
          lineV: 120 + Math.sin(t / 7) * 1.5 + Math.random() * 0.6, outputV: null, lineHz: null, batteryPct: 100, runtimeMin: Math.round(4200 / Math.max(50, upsW)),
          batteryV: 27.3, batteryNominalV: 24, transfers: 0, lastTransfer: "Unacceptable line voltage changes", onBatteryS: 0, onBatteryTotalS: 0,
          batteryDate: "2026-04-14", selfTest: "NO", tempC: null,
        },
      };
    })(),
  };
}

let gameFrames = 0;
const demoSessions: { summary: any; detail: any }[] = [];
const membwOff = typeof location !== "undefined" && new URLSearchParams(location.search).get("membw") === "off";
const gameEnd = Number(new URLSearchParams(typeof location !== "undefined" ? location.search : "").get("fin")) || Infinity;
/** Juego simulado: ~140 FPS con variación, caídas en escenas pesadas y algún tirón. */
function demoGame(): GameSnapshot {
  const dt = intervalMs;
  const heavy = wave(0.05, 7) > 0.75;
  const base = heavy ? 11 : 7;
  const fts: number[] = [];
  for (let acc = 0; acc < dt; ) {
    let ft = base + (Math.random() - 0.5) * 1.6 + Math.sin(t * 3) * 0.6;
    if (Math.random() < 0.004) ft *= 3 + Math.random() * 3;
    fts.push(ft);
    acc += ft;
  }
  gameFrames += fts.length;
  return {
    pid: 4210, title: "Juego de demostración", exe: "Demo.exe", api: "vulkan", bits: 64, steamAppId: 480,
    frames: gameFrames, frametimes: fts, fps: Math.round((fts.length * 1000) / dt), active: true, duration: t,
  };
}

const PROC_NAMES = ["firefox", "plasmashell", "kwin_wayland", "code", "juego", "neontop", "pipewire", "systemd", "Xwayland", "discord", "steam", "node", "cargo", "rustc", "bash", "kworker/0:1"];

function procs(): ProcInfo[] {
  return Array.from({ length: 60 }, (_, i) => {
    const name = PROC_NAMES[i % PROC_NAMES.length];
    const cpu = i < 6 ? 60 * wave(0.2, i) ** 2 : Math.random() * 1.5;
    return {
      pid: 1000 + i * 37, ppid: i < 3 ? 1 : 1000 + (i % 3) * 37, name, cmd: `/usr/bin/${name} --demo`, user: i % 7 === 0 ? "root" : "usuario", uid: i % 7 === 0 ? 0 : 1000,
      state: cpu > 10 ? "R" : "S", threads: 1 + (i * 7) % 40, nice: 0, priority: 20, cpu, mem: (0.02 + ((i * 131) % 97) / 60) * GB * (i < 8 ? 1 : 0.2), virt: 4 * GB,
      readRate: i % 4 === 0 ? 1e5 * Math.random() : 0, writeRate: 0, cpuTime: 100 + i * 13, start: 1000 + i * 50, kernel: name.startsWith("kworker"),
    };
  });
}

/** Activa la simulación (solo fuera de Tauri). */
export function startDemo() {
  let timer: ReturnType<typeof setTimeout>;
  let procsOn = false;
  const loop = () => {
    emit("metrics", snapshot());
    if (procsOn) emit("procs", { procs: procs() });
    timer = setTimeout(loop, intervalMs);
  };
  const demoFpsAuto = {
    enabled: true,
    launchers: [
      { id: "steam", name: "Steam", total: 19, applied: 3, running: true, scope: "opciones de lanzamiento de cada juego" },
      { id: "heroic", name: "Heroic", total: 30, applied: 30, running: false, scope: "envoltorio de cada juego y de los juegos nuevos" },
      { id: "lutris", name: "Lutris", total: 1, applied: 1, running: false, scope: "prefijo de comando global" },
    ],
  };
  mockIPC(
    (cmd, args: any) => {
      switch (cmd) {
        case "get_system_info":
          return info;
        case "set_interval":
          intervalMs = Math.max(250, args?.ms ?? 1000);
          return;
        case "set_collect":
          if (args?.kind === "procs") procsOn = !!args.enabled;
          return;
        case "rapl_access":
        case "membw_access":
          return;
        // historial de partidas en memoria (en la app va a disco)
        case "sessions_list":
          return demoSessions.map((x) => x.summary);
        case "session_save":
          demoSessions.unshift({ summary: args.summary, detail: args.detail });
          return;
        case "session_load":
          return demoSessions.find((x) => x.summary.id === args.id)?.detail ?? null;
        case "session_delete":
          demoSessions.splice(demoSessions.findIndex((x) => x.summary.id === args.id) >>> 0, 1);
          return;
        case "sessions_import":
          return 0;
        case "fps_auto_status":
        case "set_fps_auto":
        case "fps_auto_restart_steam":
          if (cmd === "set_fps_auto") demoFpsAuto.enabled = !!args.enabled;
          return demoFpsAuto;
        case "bench_memory":
          return { read: 41e9, write: 19e9, copy: 20e9, readSingle: 27e9, latencyNs: 78, threads: NCORES, bufferBytes: 512 * 1024 ** 2, durationMs: 1200, ts: Date.now() };
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  clearTimeout(timer!);
  setTimeout(loop, 50);

  // ?theme=…&tab=… para revisar un tema o una pestaña concretos
  const q = new URLSearchParams(location.search);
  const stored = JSON.parse(localStorage.getItem("neontop.settings") ?? "{}");
  if (q.get("theme")) stored.theme = q.get("theme");
  if (q.get("tab")) stored.tab = q.get("tab");
  localStorage.setItem("neontop.settings", JSON.stringify(stored));
}
