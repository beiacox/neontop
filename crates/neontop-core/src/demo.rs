//! Modo demostración (`neontop --demo`): muestras sintéticas que imitan un equipo con CPU de 12
//! hilos, GPU dedicada, dos discos, SAI y un juego en marcha. Sirve para revisar temas y
//! maquetación (y hacer capturas) sin enseñar datos reales del equipo. Es determinista: con la
//! misma semilla da siempre las mismas muestras.


use crate::collectors::{
    cpu::{CpuBreakdown, CpuSnapshot},
    disks::{DiskIoSnapshot, DiskSnapshot, FsSnapshot},
    energy::{EnergySnapshot, EnergySource},
    games::{GameSnapshot, GamesSnapshot},
    gpu::{GpuEngine, GpuProc, GpuSnapshot},
    hwmon::TempReading,
    membw::MemBwSnapshot,
    memory::MemSnapshot,
    network::{IfaceSnapshot, NetSnapshot},
    pressure::{PressureSnapshot, Psi},
    processes::{ProcInfo, ProcSnapshot},
    sensors::{Chip, Reading, SensorSnapshot},
    system::SystemInfo,
    ups::UpsSnapshot,
};
use crate::sampler::Snapshot;

const GB: f64 = 1024.0 * 1024.0 * 1024.0;
const NCORES: usize = 12;

pub fn info() -> SystemInfo {
    SystemInfo {
        hostname: "demo".into(),
        os: "NeonTop (modo demo)".into(),
        kernel: "6.x-demo".into(),
        cpu_model: "CPU de demostración · 6 núcleos".into(),
        logical_cores: NCORES as u32,
        physical_cores: 6,
        freq_min: vec![550.0; NCORES],
        freq_max: vec![4650.0; NCORES],
        mem_total: (32.0 * GB) as u64,
        boot_time: crate::format::now_ms() / 1000 - 9000,
    }
}

/// Generador de muestras sintéticas.
pub struct Demo {
    t: f64,
    rng: u64,
    rx: f64,
    tx: f64,
    dr: f64,
    dw: f64,
    ej: std::collections::HashMap<&'static str, f64>,
    total_j: f64,
    game_frames: u64,
    /// El juego simulado se cierra a los N segundos (para probar el fin de partida).
    pub game_end: f64,
}

impl Default for Demo {
    fn default() -> Self {
        Self::new(0x5eed)
    }
}

impl Demo {
    pub fn new(seed: u64) -> Self {
        Demo { t: 0.0, rng: seed | 1, rx: 0.0, tx: 0.0, dr: 0.0, dw: 0.0, ej: Default::default(), total_j: 0.0, game_frames: 0, game_end: f64::INFINITY }
    }

    /// Aleatorio uniforme 0..1 (xorshift64*).
    fn rand(&mut self) -> f64 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Onda suave con algo de ruido, 0..1.
    fn wave(&mut self, speed: f64, phase: f64, noise: f64) -> f64 {
        let t = self.t;
        let r = self.rand();
        (0.5 + 0.35 * (t * speed + phase).sin() + 0.15 * (t * speed * 3.1 + phase * 2.0).sin() + (r - 0.5) * noise).clamp(0.0, 1.0)
    }

    fn src(&mut self, id: &'static str, label: &str, kind: &str, watts: f64, dt: f64) -> EnergySource {
        let j = self.ej.entry(id).or_insert(0.0);
        *j += watts * dt;
        EnergySource { id: id.into(), label: label.into(), kind: kind.into(), watts: Some(watts), joules: *j, method: "counter".into(), ..Default::default() }
    }

    /// Siguiente muestra, `interval_ms` después de la anterior, con marca de tiempo `ts` (ms).
    pub fn sample(&mut self, interval_ms: u64, ts: u64) -> Snapshot {
        let dt = interval_ms as f64 / 1000.0;
        self.t += dt;
        let load = self.wave(0.08, 0.0, 0.15);
        let cores: Vec<CpuBreakdown> = (0..NCORES)
            .map(|i| {
                let w = self.wave(0.2, i as f64, 0.15);
                let extra = if i % 5 == 0 { 25.0 * self.wave(0.5, i as f64, 0.15) } else { 0.0 };
                let u = (load * 70.0 * w + extra).clamp(1.0, 100.0) as f32;
                CpuBreakdown { usage: u, user: u * 0.7, nice: u * 0.02, system: u * 0.22, iowait: u * 0.03, irq: u * 0.01, softirq: u * 0.02, steal: 0.0 }
            })
            .collect();
        let avg = |f: fn(&CpuBreakdown) -> f32| cores.iter().map(f).sum::<f32>() / NCORES as f32;
        let total = CpuBreakdown {
            usage: avg(|c| c.usage),
            user: avg(|c| c.user),
            nice: avg(|c| c.nice),
            system: avg(|c| c.system),
            iowait: avg(|c| c.iowait),
            irq: avg(|c| c.irq),
            softirq: avg(|c| c.softirq),
            steal: 0.0,
        };
        let cpu_temp = (42.0 + 38.0 * load) as f32;
        let rx_rate = 2e6 * self.wave(0.3, 1.0, 0.6).powi(2) + 5e4;
        let tx_rate = 4e5 * self.wave(0.25, 2.0, 0.6).powi(2) + 1e4;
        self.rx += rx_rate * dt;
        self.tx += tx_rate * dt;
        let read_rate = 60e6 * self.wave(0.15, 3.0, 0.4).powi(3);
        let write_rate = 25e6 * self.wave(0.12, 4.0, 0.4).powi(3);
        self.dr += read_rate * dt;
        self.dw += write_rate * dt;
        let gpu_busy = 100.0 * self.wave(0.1, 5.0, 0.15).powi(2);
        let mem_used = (9.0 + 5.0 * load) * GB;
        let pkg_w = 18.0 + 70.0 * load;
        let gpu_w = 20.0 + gpu_busy * 1.4;
        let dirty = 2e6 * self.wave(0.4, 0.0, 0.15);
        let vram_wave = self.wave(0.05, 0.0, 0.15);
        let enc = 6.0 * self.wave(0.3, 0.0, 0.15);
        let dec = 3.0 * self.wave(0.2, 0.0, 0.15);
        let io_wave = self.wave(0.2, 0.0, 0.15);
        let sda_util = 4.0 * self.wave(0.3, 0.0, 0.15);
        let gb = |x: f64| (x * GB) as u64;

        let mut sources = vec![
            EnergySource { in_total: true, limit: Some(88.0), ..self.src("rapl:intel-rapl:0", "CPU (paquete)", "cpu", pkg_w, dt) },
            EnergySource { parent: Some("rapl:intel-rapl:0".into()), ..self.src("rapl:intel-rapl:0:0", "Núcleos de CPU", "cpu-core", pkg_w * 0.72, dt) },
            EnergySource { in_total: true, limit: Some(160.0), method: "sensor".into(), ..self.src("gpu:0000:03:00.0", "GPU de demostración", "gpu", gpu_w, dt) },
        ];
        // SAI simulado: el PC + ~45 W de monitores y resto, en saltos de 9 W (carga en % enteros)
        let ups_load = ((pkg_w + gpu_w + 45.0) / 900.0 * 100.0).round();
        let ups_w = ups_load / 100.0 * 900.0;
        sources.push(EnergySource { system: true, limit: Some(900.0), method: "sensor".into(), ..self.src("ups:apcupsd", "SAI · Back-UPS XS 1500M", "ups", ups_w, dt) });
        self.total_j += ups_w * dt;
        let line_v = 120.0 + (self.t / 7.0).sin() * 1.5 + self.rand() * 0.6;

        let membw_read = 4e9 + 14e9 * load + 2e9 * self.rand();
        let membw_write = 1.5e9 + 6e9 * load + 1e9 * self.rand();
        let games = if self.t < self.game_end { vec![self.game(interval_ms)] } else { vec![] };

        Snapshot {
            ts,
            uptime: 9000.0 + self.t,
            cpu: CpuSnapshot {
                freqs: cores.iter().map(|c| 2000.0 + c.usage * 26.0).collect(),
                cores,
                total,
                temp: Some(cpu_temp),
                temps: vec![TempReading { label: "Tctl".into(), celsius: cpu_temp }, TempReading { label: "Tccd1".into(), celsius: cpu_temp - 3.0 }],
                load_avg: [(load * 8.0) as f32, (load * 6.0) as f32, (load * 4.0) as f32],
                procs_running: 1 + (load * 6.0).round() as u32,
                procs_blocked: 0,
                procs_total: 480,
                ctxt_rate: (20000.0 + load * 40000.0) as f32,
                intr_rate: (15000.0 + load * 20000.0) as f32,
                fork_rate: (load * 30.0).round() as f32,
            },
            mem: MemSnapshot {
                total: gb(32.0),
                free: (32.0 * GB - mem_used - 11.0 * GB) as u64,
                available: (32.0 * GB - mem_used) as u64,
                buffers: gb(0.2),
                cached: gb(10.0),
                s_reclaimable: gb(0.8),
                shmem: gb(0.7),
                swap_total: gb(16.0),
                swap_free: gb(15.5),
                swap_cached: gb(0.05),
                dirty: dirty as u64,
                writeback: 0,
                active: gb(12.0),
                inactive: gb(8.0),
                active_anon: gb(7.0),
                inactive_anon: gb(1.0),
                active_file: gb(5.0),
                inactive_file: gb(7.0),
                slab: gb(1.5),
                kernel_stack: 40_000_000,
                page_tables: 120_000_000,
                mapped: gb(2.0),
                anon_pages: gb(8.0),
                zswap: 0,
                zswapped: 0,
                committed_as: gb(20.0),
                commit_limit: gb(32.0),
            },
            net: NetSnapshot {
                ifaces: vec![
                    IfaceSnapshot {
                        name: "wlan0".into(),
                        kind: "wifi".into(),
                        state: "up".into(),
                        mac: "00:11:22:33:44:55".into(),
                        mtu: 1500,
                        addrs: vec!["192.168.1.20".into()],
                        rx_rate,
                        tx_rate,
                        rx_pkt_rate: rx_rate / 900.0,
                        tx_pkt_rate: tx_rate / 600.0,
                        rx_total: (8e9 + self.rx) as u64,
                        tx_total: (1e9 + self.tx) as u64,
                        rx_drop: 3,
                        ..Default::default()
                    },
                    IfaceSnapshot { name: "enp3s0".into(), kind: "ethernet".into(), state: "down".into(), mac: "00:11:22:33:44:66".into(), mtu: 1500, ..Default::default() },
                    IfaceSnapshot {
                        name: "lo".into(),
                        kind: "loopback".into(),
                        state: "unknown".into(),
                        mtu: 65536,
                        addrs: vec!["127.0.0.1".into()],
                        rx_rate: 2000.0,
                        tx_rate: 2000.0,
                        rx_pkt_rate: 10.0,
                        tx_pkt_rate: 10.0,
                        rx_total: 100_000_000,
                        tx_total: 100_000_000,
                        ..Default::default()
                    },
                ],
                tcp_established: 42,
                tcp_listen: 9,
                udp_sockets: 14,
            },
            pressure: PressureSnapshot {
                cpu: Psi { some: (load * 4.0) as f32, full: 0.0 },
                memory: Psi { some: 0.1, full: 0.0 },
                io: Psi { some: (io_wave * 2.0) as f32, full: io_wave as f32 },
            },
            gpus: vec![GpuSnapshot {
                name: "GPU de demostración".into(),
                vendor: "amd".into(),
                pci_slot: "0000:03:00.0".into(),
                driver: "amdgpu".into(),
                busy: Some(gpu_busy as f32),
                mem_busy: Some((gpu_busy * 0.4) as f32),
                vram_used: gb(3.0 + 4.0 * vram_wave),
                vram_total: gb(16.0),
                gtt_used: gb(0.3),
                gtt_total: gb(16.0),
                vram_type: Some("GDDR6".into()),
                bus_width: Some(128),
                mem_clock: Some(1250.0),
                mem_clock_max: Some(1258.0),
                vram_peak_bw: Some(322e9),
                vram_bw: Some(322e9 * gpu_busy * 0.004),
                pcie_speed: Some(16.0),
                pcie_width: Some(16),
                pcie_max_speed: Some(32.0),
                pcie_max_width: Some(16),
                pcie_peak_bw: Some(31.5e9),
                gfx_clock: Some((500.0 + gpu_busy * 28.0) as f32),
                gfx_clock_max: Some(3200.0),
                power: Some(gpu_w as f32),
                power_limit: Some(160.0),
                temp_edge: Some((40.0 + gpu_busy * 0.35) as f32),
                temp_hotspot: Some((44.0 + gpu_busy * 0.45) as f32),
                temp_mem: Some(46.0),
                fan_rpm: Some((gpu_busy * 18.0).round() as u32),
                fan_pct: Some((gpu_busy * 0.6) as f32),
                engines: vec![
                    GpuEngine { name: "gfx".into(), busy: gpu_busy as f32 },
                    GpuEngine { name: "enc".into(), busy: enc as f32 },
                    GpuEngine { name: "dec".into(), busy: dec as f32 },
                ],
                processes: vec![
                    GpuProc { pid: 4210, name: "juego".into(), engines: vec![GpuEngine { name: "gfx".into(), busy: (gpu_busy * 0.8) as f32 }], vram: gb(3.1), gtt: gb(0.1) },
                    GpuProc { pid: 1880, name: "firefox".into(), engines: vec![GpuEngine { name: "gfx".into(), busy: 2.0 }], vram: gb(0.45), gtt: gb(0.03) },
                    GpuProc { pid: 1201, name: "plasmashell".into(), engines: vec![GpuEngine { name: "gfx".into(), busy: 0.5 }], vram: gb(0.25), gtt: gb(0.01) },
                ],
                processes_sampled: true,
                ..Default::default()
            }],
            disks: DiskIoSnapshot {
                disks: vec![
                    DiskSnapshot {
                        name: "nvme0n1".into(),
                        model: "SSD NVMe de demostración".into(),
                        kind: "nvme".into(),
                        size: 1_000_000_000_000,
                        read_rate,
                        write_rate,
                        read_iops: read_rate / 4e4,
                        write_iops: write_rate / 3e4,
                        util: ((read_rate + write_rate) / 1e6).min(100.0) as f32,
                        read_lat: 0.2,
                        write_lat: 0.4,
                        queue: 0.3,
                        read_total: self.dr as u64,
                        write_total: self.dw as u64,
                        temps: vec![TempReading { label: "Composite".into(), celsius: (38.0 + load * 12.0) as f32 }],
                        partitions: vec!["nvme0n1p1".into(), "nvme0n1p2".into()],
                        ..Default::default()
                    },
                    DiskSnapshot {
                        name: "sda".into(),
                        model: "HDD de demostración".into(),
                        kind: "hdd".into(),
                        size: 2_000_000_000_000,
                        read_rate: read_rate * 0.1,
                        read_iops: 3.0,
                        util: sda_util as f32,
                        read_lat: 8.0,
                        queue: 0.05,
                        read_total: (self.dr / 10.0) as u64,
                        partitions: vec!["sda1".into()],
                        ..Default::default()
                    },
                ],
                filesystems: vec![
                    FsSnapshot { device: "/dev/nvme0n1p2".into(), disk: Some("nvme0n1".into()), mount: "/".into(), other_mounts: vec!["/home".into()], fstype: "btrfs".into(), total: 950_000_000_000, used: 510_000_000_000, avail: 440_000_000_000, ..Default::default() },
                    FsSnapshot { device: "/dev/nvme0n1p1".into(), disk: Some("nvme0n1".into()), mount: "/boot".into(), fstype: "vfat".into(), total: 1_000_000_000, used: 200_000_000, avail: 800_000_000, ..Default::default() },
                    FsSnapshot { device: "/dev/sda1".into(), disk: Some("sda".into()), mount: "/datos".into(), fstype: "ext4".into(), total: 2_000_000_000_000, used: 1_840_000_000_000, avail: 160_000_000_000, inodes_total: 120_000_000, inodes_free: 100_000_000, ..Default::default() },
                ],
            },
            sensors: SensorSnapshot {
                chips: vec![
                    chip("hwmon0:k10temp", "k10temp", "CPU", "cpu", vec![reading("hwmon0:k10temp/temp1", "Tctl", "temp", cpu_temp as f64, None, None)]),
                    chip(
                        "hwmon1:amdgpu",
                        "amdgpu",
                        "GPU",
                        "gpu",
                        vec![
                            reading("hwmon1:amdgpu/temp1", "edge", "temp", 40.0 + gpu_busy * 0.35, None, Some(100.0)),
                            reading("hwmon1:amdgpu/temp2", "junction", "temp", 44.0 + gpu_busy * 0.45, None, Some(110.0)),
                            reading("hwmon1:amdgpu/fan1", "fan1", "fan", (gpu_busy * 18.0).round(), Some(3500.0), None),
                            reading("hwmon1:amdgpu/power1_average", "PPT", "power", gpu_w, None, None),
                        ],
                    ),
                    chip("hwmon2:nvme", "nvme", "NVMe de demostración", "disk", vec![reading("hwmon2:nvme/temp1", "Composite", "temp", 38.0 + load * 12.0, Some(84.85), Some(94.85))]),
                ],
                ..Default::default()
            },
            energy: EnergySnapshot {
                sources,
                total_w: ups_w,
                total_j: self.total_j,
                total_method: "ups".into(),
                rapl: "ok".into(),
                rapl_rule: true,
                ups: Some(UpsSnapshot {
                    source: "apcupsd".into(),
                    model: "Back-UPS XS 1500M".into(),
                    status: "ONLINE".into(),
                    load_pct: Some(ups_load),
                    nominal_w: Some(900.0),
                    power_w: Some(ups_w),
                    line_v: Some(line_v),
                    battery_pct: Some(100.0),
                    runtime_min: Some((4200.0 / ups_w.max(50.0)).round()),
                    battery_v: Some(27.3),
                    battery_nominal_v: Some(24.0),
                    transfers: Some(0),
                    last_transfer: Some("Unacceptable line voltage changes".into()),
                    on_battery_s: Some(0.0),
                    on_battery_total_s: Some(0.0),
                    battery_date: Some("2026-04-14".into()),
                    self_test: Some("NO".into()),
                    ..Default::default()
                }),
            },
            games: GamesSnapshot { games, layer_installed: true },
            membw: MemBwSnapshot { status: "ok".into(), source: Some("intel_imc".into()), read: Some(membw_read), write: Some(membw_write), total: Some(membw_read + membw_write), enabled: true, message: None },
        }
    }

    /// Juego simulado: ~140 FPS con variación, caídas en escenas pesadas y algún tirón.
    fn game(&mut self, interval_ms: u64) -> GameSnapshot {
        let dt = interval_ms as f64;
        let heavy = self.wave(0.05, 7.0, 0.15) > 0.75;
        let base = if heavy { 11.0 } else { 7.0 };
        let mut fts = Vec::new();
        let mut acc = 0.0;
        while acc < dt {
            let mut ft = base + (self.rand() - 0.5) * 1.6 + (self.t * 3.0).sin() * 0.6;
            if self.rand() < 0.004 {
                ft *= 3.0 + self.rand() * 3.0;
            }
            fts.push(ft as f32);
            acc += ft;
        }
        self.game_frames += fts.len() as u64;
        GameSnapshot {
            pid: 4210,
            title: "Juego de demostración".into(),
            exe: "Demo.exe".into(),
            api: "vulkan".into(),
            bits: 64,
            steam_app_id: Some(480),
            frames: self.game_frames,
            fps: (fts.len() as f64 * 1000.0 / dt).round() as f32,
            frametimes: fts,
            active: true,
            duration: self.t,
        }
    }

    /// Lista de procesos simulada.
    pub fn procs(&mut self) -> ProcSnapshot {
        const NAMES: [&str; 16] = [
            "firefox", "plasmashell", "kwin_wayland", "code", "juego", "neontop", "pipewire", "systemd", "Xwayland", "discord", "steam", "node", "cargo", "rustc", "bash", "kworker/0:1",
        ];
        let procs = (0..60u32)
            .map(|i| {
                let name = NAMES[i as usize % NAMES.len()];
                let cpu = if i < 6 { 60.0 * self.wave(0.2, i as f64, 0.15).powi(2) } else { self.rand() * 1.5 };
                let read = if i % 4 == 0 { 1e5 * self.rand() } else { 0.0 };
                ProcInfo {
                    pid: 1000 + i * 37,
                    ppid: if i < 3 { 1 } else { 1000 + (i % 3) * 37 },
                    name: name.into(),
                    cmd: format!("/usr/bin/{name} --demo"),
                    user: if i % 7 == 0 { "root".into() } else { "usuario".into() },
                    uid: if i % 7 == 0 { 0 } else { 1000 },
                    state: if cpu > 10.0 { "R".into() } else { "S".into() },
                    threads: 1 + (i * 7) % 40,
                    nice: 0,
                    priority: 20,
                    cpu: cpu as f32,
                    mem: ((0.02 + ((i * 131) % 97) as f64 / 60.0) * GB * if i < 8 { 1.0 } else { 0.2 }) as u64,
                    virt: (4.0 * GB) as u64,
                    read_rate: Some(read),
                    write_rate: Some(0.0),
                    cpu_time: 100.0 + i as f64 * 13.0,
                    start: 1000.0 + i as f64 * 50.0,
                    kernel: name.starts_with("kworker"),
                }
            })
            .collect();
        ProcSnapshot { procs }
    }
}

fn chip(id: &str, driver: &str, label: &str, category: &str, readings: Vec<Reading>) -> Chip {
    Chip { id: id.into(), driver: driver.into(), label: label.into(), category: category.into(), readings }
}

fn reading(id: &str, label: &str, kind: &str, value: f64, max: Option<f64>, crit: Option<f64>) -> Reading {
    Reading { id: id.into(), label: label.into(), kind: kind.into(), value, max, crit }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use std::sync::Arc;

    #[test]
    fn deterministic_and_plausible() {
        let (mut a, mut b) = (Demo::new(7), Demo::new(7));
        let (sa, sb) = (a.sample(1000, 1), b.sample(1000, 1));
        assert_eq!(sa, sb, "misma semilla, misma muestra");
        assert_eq!(sa.cpu.cores.len(), NCORES);
        assert!((0.0..=100.0).contains(&sa.cpu.total.usage));
        assert!(sa.mem.free < sa.mem.total);
        let g = &sa.games.games[0];
        let sum: f32 = g.frametimes.iter().sum();
        assert!((990.0..1200.0).contains(&sum), "los fotogramas cubren el intervalo: {sum}");
        assert_eq!(a.procs().procs.len(), 60);
    }

    #[test]
    fn feeds_the_whole_model() {
        let mut d = Demo { game_end: 30.0, ..Default::default() };
        let mut m = Model::new(info(), false);
        let mut fired = 0;
        for i in 0..40u64 {
            fired += m.ingest(Arc::new(d.sample(1000, 1_700_000_000_000 + i * 1000))).len();
            if i % 5 == 0 {
                m.ingest_procs(Arc::new(d.procs()));
            }
        }
        assert_eq!(m.samples, 40);
        assert_eq!(m.history.len(), 40);
        assert!(m.history.get("core.11").is_some());
        assert!(m.history.get("gpu.0.eng.gfx").is_some());
        assert!(m.history.get("net.wlan0.rx").is_some());
        assert!(m.history.get("game.4210.fps").is_some_and(|s| s[10] > 50.0));
        // el juego terminó a los 30 s: la partida se guardó
        assert_eq!(m.games.benchmarks.len(), 1);
        assert!(m.games.benchmarks[0].avg > 80.0);
        // /datos está al 92 %: la alerta de disco lleno salta una vez
        assert_eq!(fired, 1);
        assert!(m.energy.today(m.snap.ts).is_some_and(|d| d.wh > 0.0));
        assert!(!m.procs.list.procs.is_empty());
    }
}
