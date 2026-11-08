use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use super::hwmon::{self, TempReading};
use super::read_u64;

#[derive(Clone, Copy, Default)]
struct Times {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
    steal: u64,
}

impl Times {
    fn parse<'a>(mut fields: impl Iterator<Item = &'a str>) -> Self {
        let mut g = || fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        Times {
            user: g(),
            nice: g(),
            system: g(),
            idle: g(),
            iowait: g(),
            irq: g(),
            softirq: g(),
            steal: g(),
        }
    }

    fn total(&self) -> u64 {
        self.user + self.nice + self.system + self.idle + self.iowait + self.irq + self.softirq + self.steal
    }
}

/// Reparto porcentual del tiempo de CPU entre dos lecturas.
#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CpuBreakdown {
    pub usage: f32,
    pub user: f32,
    pub nice: f32,
    pub system: f32,
    pub iowait: f32,
    pub irq: f32,
    pub softirq: f32,
    pub steal: f32,
}

fn breakdown(prev: &Times, cur: &Times) -> CpuBreakdown {
    let dt = cur.total().saturating_sub(prev.total()) as f32;
    if dt <= 0.0 {
        return CpuBreakdown::default();
    }
    let p = |a: u64, b: u64| a.saturating_sub(b) as f32 / dt * 100.0;
    let idle = p(cur.idle, prev.idle);
    let iowait = p(cur.iowait, prev.iowait);
    CpuBreakdown {
        usage: (100.0 - idle - iowait).clamp(0.0, 100.0),
        user: p(cur.user, prev.user),
        nice: p(cur.nice, prev.nice),
        system: p(cur.system, prev.system),
        iowait,
        irq: p(cur.irq, prev.irq),
        softirq: p(cur.softirq, prev.softirq),
        steal: p(cur.steal, prev.steal),
    }
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CpuSnapshot {
    pub total: CpuBreakdown,
    pub cores: Vec<CpuBreakdown>,
    /// MHz por núcleo lógico.
    pub freqs: Vec<f32>,
    /// Temperatura principal del paquete (Tctl/Tdie/Package).
    pub temp: Option<f32>,
    pub temps: Vec<TempReading>,
    pub load_avg: [f32; 3],
    pub procs_running: u32,
    pub procs_blocked: u32,
    pub procs_total: u32,
    pub ctxt_rate: f32,
    pub intr_rate: f32,
    pub fork_rate: f32,
}

pub struct CpuCollector {
    prev_total: Times,
    prev_cores: Vec<Times>,
    prev_ctxt: u64,
    prev_intr: u64,
    prev_forks: u64,
    prev_at: Instant,
    temp_dir: Option<PathBuf>,
}

impl CpuCollector {
    pub fn new() -> Self {
        let mut c = CpuCollector {
            prev_total: Times::default(),
            prev_cores: Vec::new(),
            prev_ctxt: 0,
            prev_intr: 0,
            prev_forks: 0,
            prev_at: Instant::now(),
            temp_dir: hwmon::find_by_name(&["k10temp", "zenpower", "coretemp", "cpu_thermal", "acpitz"]),
        };
        c.sample(); // cebar los contadores
        c
    }

    pub fn sample(&mut self) -> CpuSnapshot {
        let now = Instant::now();
        let secs = now.duration_since(self.prev_at).as_secs_f32().max(1e-3);
        let stat = fs::read_to_string("/proc/stat").unwrap_or_default();

        let mut snap = CpuSnapshot::default();
        let mut cores = Vec::new();
        let (mut ctxt, mut intr, mut forks) = (0u64, 0u64, 0u64);

        for line in stat.lines() {
            let mut it = line.split_whitespace();
            let Some(key) = it.next() else { continue };
            match key {
                "cpu" => {
                    let t = Times::parse(it);
                    snap.total = breakdown(&self.prev_total, &t);
                    self.prev_total = t;
                }
                k if k.starts_with("cpu") => cores.push(Times::parse(it)),
                "ctxt" => ctxt = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
                "intr" => intr = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
                "processes" => forks = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
                "procs_running" => snap.procs_running = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
                "procs_blocked" => snap.procs_blocked = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
                _ => {}
            }
        }

        snap.cores = cores
            .iter()
            .enumerate()
            .map(|(i, t)| self.prev_cores.get(i).map(|p| breakdown(p, t)).unwrap_or_default())
            .collect();
        self.prev_cores = cores;

        let rate = |cur: u64, prev: u64| if prev == 0 { 0.0 } else { cur.saturating_sub(prev) as f32 / secs };
        snap.ctxt_rate = rate(ctxt, self.prev_ctxt);
        snap.intr_rate = rate(intr, self.prev_intr);
        snap.fork_rate = rate(forks, self.prev_forks);
        (self.prev_ctxt, self.prev_intr, self.prev_forks) = (ctxt, intr, forks);
        self.prev_at = now;

        snap.freqs = (0..snap.cores.len())
            .map(|i| {
                read_u64(format!("/sys/devices/system/cpu/cpu{i}/cpufreq/scaling_cur_freq"))
                    .map(|khz| khz as f32 / 1000.0)
                    .unwrap_or(0.0)
            })
            .collect();

        if let Ok(la) = fs::read_to_string("/proc/loadavg") {
            let f: Vec<&str> = la.split_whitespace().collect();
            for i in 0..3 {
                snap.load_avg[i] = f.get(i).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            }
            if let Some((_, total)) = f.get(3).and_then(|v| v.split_once('/')) {
                snap.procs_total = total.parse().unwrap_or(0);
            }
        }

        if let Some(dir) = &self.temp_dir {
            snap.temps = hwmon::read_temps(dir);
            let main = ["Tctl", "Tdie", "Package id 0"];
            snap.temp = main
                .iter()
                .find_map(|m| snap.temps.iter().find(|t| t.label == *m))
                .or_else(|| snap.temps.first())
                .map(|t| t.celsius);
        }

        snap
    }
}
