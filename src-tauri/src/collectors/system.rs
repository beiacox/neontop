use serde::Serialize;
use std::collections::HashSet;
use std::fs;

use super::{read_trim, read_u64};

/// Información estática del equipo; se pide una vez al arrancar.
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub cpu_model: String,
    pub logical_cores: u32,
    pub physical_cores: u32,
    /// MHz por núcleo lógico.
    pub freq_min: Vec<f32>,
    pub freq_max: Vec<f32>,
    pub mem_total: u64,
    pub boot_time: u64,
}

pub fn info() -> SystemInfo {
    let os = fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_string()))
        })
        .unwrap_or_else(|| "Linux".into());

    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let mut cpu_model = String::new();
    let mut logical = 0u32;
    let mut phys: HashSet<(String, String)> = HashSet::new();
    let mut cur_pkg = String::new();
    for line in cpuinfo.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "processor" => logical += 1,
            "model name" if cpu_model.is_empty() => cpu_model = v.to_string(),
            "physical id" => cur_pkg = v.to_string(),
            "core id" => {
                phys.insert((cur_pkg.clone(), v.to_string()));
            }
            _ => {}
        }
    }

    let khz = |i: u32, f: &str| {
        read_u64(format!("/sys/devices/system/cpu/cpu{i}/cpufreq/{f}")).map(|k| k as f32 / 1000.0).unwrap_or(0.0)
    };

    let boot_time = fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.strip_prefix("btime ").and_then(|v| v.trim().parse().ok())))
        .unwrap_or(0);

    let mem_total = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|t| t.lines().next().and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()))
        .unwrap_or(0)
        * 1024;

    SystemInfo {
        hostname: read_trim("/proc/sys/kernel/hostname").unwrap_or_default(),
        kernel: read_trim("/proc/sys/kernel/osrelease").unwrap_or_default(),
        os,
        cpu_model,
        logical_cores: logical,
        physical_cores: if phys.is_empty() { logical } else { phys.len() as u32 },
        freq_min: (0..logical).map(|i| khz(i, "cpuinfo_min_freq")).collect(),
        freq_max: (0..logical).map(|i| khz(i, "cpuinfo_max_freq")).collect(),
        mem_total,
        boot_time,
    }
}

pub fn uptime() -> f64 {
    read_trim("/proc/uptime")
        .and_then(|s| s.split_whitespace().next()?.parse().ok())
        .unwrap_or(0.0)
}
