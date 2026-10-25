// Espejo de los structs serializados en src-tauri/src/collectors.

export interface SystemInfo {
  hostname: string;
  os: string;
  kernel: string;
  cpuModel: string;
  logicalCores: number;
  physicalCores: number;
  freqMin: number[];
  freqMax: number[];
  memTotal: number;
  bootTime: number;
}

export interface CpuBreakdown {
  usage: number;
  user: number;
  nice: number;
  system: number;
  iowait: number;
  irq: number;
  softirq: number;
  steal: number;
}

export interface TempReading {
  label: string;
  celsius: number;
}

export interface CpuSnapshot {
  total: CpuBreakdown;
  cores: CpuBreakdown[];
  freqs: number[];
  temp: number | null;
  temps: TempReading[];
  loadAvg: [number, number, number];
  procsRunning: number;
  procsBlocked: number;
  procsTotal: number;
  ctxtRate: number;
  intrRate: number;
  forkRate: number;
}

export interface MemSnapshot {
  total: number;
  free: number;
  available: number;
  buffers: number;
  cached: number;
  sReclaimable: number;
  shmem: number;
  swapTotal: number;
  swapFree: number;
  swapCached: number;
  dirty: number;
  writeback: number;
  active: number;
  inactive: number;
  activeAnon: number;
  inactiveAnon: number;
  activeFile: number;
  inactiveFile: number;
  slab: number;
  kernelStack: number;
  pageTables: number;
  mapped: number;
  anonPages: number;
  zswap: number;
  zswapped: number;
  committedAs: number;
  commitLimit: number;
}

export type IfaceKind = "ethernet" | "wifi" | "loopback" | "virtual";

export interface IfaceSnapshot {
  name: string;
  kind: IfaceKind;
  state: string;
  mac: string;
  mtu: number;
  speed: number | null;
  addrs: string[];
  rxRate: number;
  txRate: number;
  rxPktRate: number;
  txPktRate: number;
  rxTotal: number;
  txTotal: number;
  rxErrs: number;
  txErrs: number;
  rxDrop: number;
  txDrop: number;
}

export interface NetSnapshot {
  ifaces: IfaceSnapshot[];
  tcpEstablished: number;
  tcpListen: number;
  udpSockets: number;
}

export interface Psi {
  some: number;
  full: number;
}

export interface Snapshot {
  ts: number;
  uptime: number;
  cpu: CpuSnapshot;
  mem: MemSnapshot;
  net: NetSnapshot;
  pressure: { cpu: Psi; memory: Psi; io: Psi };
  gpus: GpuSnapshot[];
  disks: DiskIoSnapshot;
  sensors: SensorSnapshot;
  energy: EnergySnapshot;
  games: GamesSnapshot;
  membw: MemBwSnapshot;
}

/** Tráfico de la RAM en tiempo real (contadores del controlador de memoria). */
export interface MemBwSnapshot {
  status: "ok" | "nomodule" | "noperm" | "nopmu" | "unsupported" | "missing" | "error" | "starting";
  message: string | null;
  source: "amd_df" | "intel_imc" | null;
  /** bytes/s; lectura y escritura solo si el hardware las separa (Intel). */
  read: number | null;
  write: number | null;
  total: number | null;
  enabled: boolean;
}

export interface GameSnapshot {
  pid: number;
  title: string;
  exe: string;
  api: "vulkan" | "opengl" | "egl" | "?";
  bits: number;
  steamAppId: number | null;
  frames: number;
  /** Tiempos de fotograma nuevos desde la muestra anterior (ms). */
  frametimes: number[];
  /** FPS del último segundo. */
  fps: number;
  active: boolean;
  duration: number;
}

export interface GamesSnapshot {
  games: GameSnapshot[];
  layerInstalled: boolean;
}

export type EnergyKind = "cpu" | "cpu-core" | "uncore" | "dram" | "platform" | "gpu" | "igpu" | "sensor" | "psu" | "battery" | "charge" | "ups";

export interface EnergySource {
  id: string;
  label: string;
  kind: EnergyKind;
  /** W; null hasta tener dos lecturas del contador. */
  watts: number | null;
  /** J acumulados desde que arrancó NeonTop. */
  joules: number;
  limit: number | null;
  /** "counter": contador de energía del hardware | "sensor": potencia instantánea integrada */
  method: "counter" | "sensor";
  /** Suma en el total medido. */
  inTotal: boolean;
  /** Mide el equipo completo (plataforma, fuente, batería descargando). */
  system: boolean;
  parent: string | null;
}

export interface EnergySnapshot {
  sources: EnergySource[];
  totalW: number;
  totalJ: number;
  totalMethod: "ups" | "platform" | "psu" | "battery" | "sum" | "none";
  rapl: "ok" | "denied" | "absent";
  raplRule: boolean;
  /** SAI conectado (apcupsd). */
  ups: UpsSnapshot | null;
}

export interface UpsSnapshot {
  source: string;
  model: string;
  status: string;
  onBattery: boolean;
  lowBattery: boolean;
  loadPct: number | null;
  nominalW: number | null;
  powerW: number | null;
  lineV: number | null;
  outputV: number | null;
  lineHz: number | null;
  batteryPct: number | null;
  runtimeMin: number | null;
  batteryV: number | null;
  batteryNominalV: number | null;
  transfers: number | null;
  lastTransfer: string | null;
  onBatteryS: number | null;
  onBatteryTotalS: number | null;
  batteryDate: string | null;
  selfTest: string | null;
  tempC: number | null;
}

export type DiskKind = "nvme" | "ssd" | "hdd" | "usb" | "virtual";

export interface DiskSnapshot {
  name: string;
  model: string;
  kind: DiskKind;
  size: number;
  removable: boolean;
  readRate: number;
  writeRate: number;
  readIops: number;
  writeIops: number;
  util: number;
  readLat: number;
  writeLat: number;
  queue: number;
  readTotal: number;
  writeTotal: number;
  temps: TempReading[];
  partitions: string[];
}

export interface FsSnapshot {
  device: string;
  disk: string | null;
  mount: string;
  otherMounts: string[];
  fstype: string;
  total: number;
  used: number;
  avail: number;
  inodesTotal: number;
  inodesFree: number;
  readonly: boolean;
}

export interface DiskIoSnapshot {
  disks: DiskSnapshot[];
  filesystems: FsSnapshot[];
}

export type SensorKind = "temp" | "fan" | "in" | "power" | "curr" | "pwm";

export interface SensorReading {
  id: string;
  label: string;
  kind: SensorKind;
  value: number;
  max: number | null;
  crit: number | null;
}

export interface SensorChip {
  id: string;
  driver: string;
  label: string;
  category: "cpu" | "gpu" | "disk" | "network" | "board" | "other";
  readings: SensorReading[];
}

export interface Battery {
  name: string;
  capacity: number;
  status: string;
  power: number | null;
  energyNow: number | null;
  energyFull: number | null;
  energyDesign: number | null;
  cycles: number | null;
}

export interface SensorSnapshot {
  chips: SensorChip[];
  batteries: Battery[];
  acOnline: boolean | null;
}

export interface GpuEngine {
  name: string;
  busy: number;
}

export interface GpuProc {
  pid: number;
  name: string;
  engines: GpuEngine[];
  vram: number;
  gtt: number;
}

export interface GpuSnapshot {
  name: string;
  vendor: "amd" | "nvidia" | "intel";
  pciSlot: string;
  driver: string;
  driverVersion: string | null;
  integrated: boolean;
  busy: number | null;
  memBusy: number | null;
  vramUsed: number;
  vramTotal: number;
  gttUsed: number;
  gttTotal: number;
  vramType: string | null;
  busWidth: number | null;
  memClock: number | null;
  memClockMax: number | null;
  /** bytes/s teóricos a reloj máximo */
  vramPeakBw: number | null;
  /** bytes/s estimados ahora (actividad del controlador × ancho de banda al reloj actual) */
  vramBw: number | null;
  pcieSpeed: number | null;
  pcieWidth: number | null;
  pcieMaxSpeed: number | null;
  pcieMaxWidth: number | null;
  pciePeakBw: number | null;
  /** tráfico PCIe real (bytes/s), solo NVIDIA */
  pcieRx: number | null;
  pcieTx: number | null;
  gfxClock: number | null;
  gfxClockMax: number | null;
  power: number | null;
  powerLimit: number | null;
  tempEdge: number | null;
  tempHotspot: number | null;
  tempMem: number | null;
  fanRpm: number | null;
  fanPct: number | null;
  throttle: string[];
  engines: GpuEngine[];
  processes: GpuProc[];
  processesSampled: boolean;
}

export interface MemBench {
  read: number;
  write: number;
  copy: number;
  readSingle: number;
  latencyNs: number;
  threads: number;
  bufferBytes: number;
  durationMs: number;
  ts: number;
}

export interface ProcInfo {
  pid: number;
  ppid: number;
  name: string;
  cmd: string;
  user: string;
  uid: number;
  state: string;
  threads: number;
  nice: number;
  priority: number;
  /** % de un núcleo; puede pasar de 100. */
  cpu: number;
  mem: number;
  virt: number;
  readRate: number | null;
  writeRate: number | null;
  cpuTime: number;
  start: number;
  kernel: boolean;
}
