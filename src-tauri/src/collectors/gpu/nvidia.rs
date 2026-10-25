//! Backend NVIDIA vía NVML. `libnvidia-ml.so.1` se carga en tiempo de ejecución: si no
//! está instalada (equipos sin NVIDIA o con nouveau) este backend simplemente no aparece.

use nvml_wrapper::bitmasks::device::ThrottleReasons;
use nvml_wrapper::enum_wrappers::device::{Clock, PcieUtilCounter, TemperatureSensor};
use nvml_wrapper::enums::device::UsedGpuMemory;
use nvml_wrapper::Nvml;
use std::collections::HashMap;
use std::fs;

use super::{pcie_bw, GpuEngine, GpuProc, GpuSnapshot};

pub struct Nvidia {
    nvml: Nvml,
    count: u32,
    /// Marca de tiempo de la última muestra de uso por proceso, por dispositivo.
    last_ts: HashMap<u32, u64>,
}

/// Velocidad por carril de cada generación PCIe (GT/s).
fn gen_speed(gen: u32) -> f32 {
    match gen {
        1 => 2.5,
        2 => 5.0,
        3 => 8.0,
        4 => 16.0,
        5 => 32.0,
        6 => 64.0,
        _ => 0.0,
    }
}

/// "00000000:01:00.0" → "0000:01:00.0" (dominio de 4 cifras, como sysfs y fdinfo).
fn normalize_bus_id(id: &str) -> String {
    let id = id.to_lowercase();
    match id.split_once(':') {
        Some((domain, rest)) if domain.len() > 4 => format!("{}:{rest}", &domain[domain.len() - 4..]),
        _ => id,
    }
}

fn throttle_names(t: ThrottleReasons) -> Vec<String> {
    let map = [
        (ThrottleReasons::SW_POWER_CAP, "Límite de potencia"),
        (ThrottleReasons::HW_SLOWDOWN, "Ralentización por hardware"),
        (ThrottleReasons::SW_THERMAL_SLOWDOWN, "Límite térmico (software)"),
        (ThrottleReasons::HW_THERMAL_SLOWDOWN, "Límite térmico (hardware)"),
        (ThrottleReasons::HW_POWER_BRAKE_SLOWDOWN, "Freno de potencia"),
        (ThrottleReasons::SYNC_BOOST, "Sync boost"),
        (ThrottleReasons::APPLICATIONS_CLOCKS_SETTING, "Relojes de aplicación"),
        (ThrottleReasons::DISPLAY_CLOCK_SETTING, "Reloj de pantalla"),
    ];
    map.iter().filter(|(f, _)| t.contains(*f)).map(|(_, n)| n.to_string()).collect()
}

impl Nvidia {
    pub fn init() -> Option<Self> {
        let nvml = Nvml::init().ok()?;
        let count = nvml.device_count().ok()?;
        (count > 0).then(|| Nvidia {
            nvml,
            count,
            last_ts: HashMap::new(),
        })
    }

    pub fn sample(&mut self, per_process: bool) -> Vec<GpuSnapshot> {
        let driver_version = self.nvml.sys_driver_version().ok();
        let mut out = Vec::new();
        for i in 0..self.count {
            let Ok(d) = self.nvml.device_by_index(i) else { continue };
            let util = d.utilization_rates().ok();
            let mem = d.memory_info().ok();
            let pci = d.pci_info().ok();
            // NVML da "00000000:01:00.0"; sysfs y fdinfo usan dominio de 4 cifras
            let slot = pci.as_ref().map(|p| normalize_bus_id(&p.bus_id)).unwrap_or_default();
            let bus_width = d.memory_bus_width().ok();
            let mem_clock = d.clock_info(Clock::Memory).ok().map(|v| v as f32);
            let mem_clock_max = d.max_clock_info(Clock::Memory).ok().map(|v| v as f32);
            // el reloj de memoria de NVML ya es la mitad de la tasa efectiva: × 2 transferencias
            let bw = |mhz: f32| bus_width.map(|w| mhz as f64 * 1e6 * 2.0 * w as f64 / 8.0);
            let vram_bw = match (mem_clock.and_then(bw), util.as_ref()) {
                (Some(peak), Some(u)) => Some(peak * u.memory as f64 / 100.0),
                _ => None,
            };
            let (gen, width) = (d.current_pcie_link_gen().ok(), d.current_pcie_link_width().ok());
            let pcie_speed = gen.map(gen_speed);

            let mut engines = Vec::new();
            if let Some(u) = &util {
                engines.push(GpuEngine {
                    name: "3d".into(),
                    busy: u.gpu as f32,
                });
            }
            if let Ok(e) = d.encoder_utilization() {
                engines.push(GpuEngine {
                    name: "enc".into(),
                    busy: e.utilization as f32,
                });
            }
            if let Ok(e) = d.decoder_utilization() {
                engines.push(GpuEngine {
                    name: "dec".into(),
                    busy: e.utilization as f32,
                });
            }

            let processes = if per_process { processes(&d, &mut self.last_ts, i) } else { Vec::new() };

            out.push(GpuSnapshot {
                name: d.name().unwrap_or_else(|_| "NVIDIA".into()),
                vendor: "nvidia".into(),
                pci_slot: slot,
                driver: "nvidia".into(),
                driver_version: driver_version.clone(),
                integrated: false,
                busy: util.as_ref().map(|u| u.gpu as f32),
                mem_busy: util.as_ref().map(|u| u.memory as f32),
                vram_used: mem.as_ref().map(|m| m.used).unwrap_or(0),
                vram_total: mem.as_ref().map(|m| m.total).unwrap_or(0),
                bus_width,
                mem_clock,
                mem_clock_max,
                vram_peak_bw: mem_clock_max.and_then(bw),
                vram_bw,
                pcie_speed,
                pcie_width: width,
                pcie_max_speed: d.max_pcie_link_gen().ok().map(gen_speed),
                pcie_max_width: d.max_pcie_link_width().ok(),
                pcie_peak_bw: pcie_speed.zip(width).map(|(s, w)| pcie_bw(s, w)),
                // NVML da KB/s
                pcie_rx: d.pcie_throughput(PcieUtilCounter::Receive).ok().map(|kb| kb as f64 * 1024.0),
                pcie_tx: d.pcie_throughput(PcieUtilCounter::Send).ok().map(|kb| kb as f64 * 1024.0),
                gfx_clock: d.clock_info(Clock::Graphics).ok().map(|v| v as f32),
                gfx_clock_max: d.max_clock_info(Clock::Graphics).ok().map(|v| v as f32),
                power: d.power_usage().ok().map(|mw| mw as f32 / 1000.0),
                power_limit: d.enforced_power_limit().ok().map(|mw| mw as f32 / 1000.0),
                temp_edge: d.temperature(TemperatureSensor::Gpu).ok().map(|t| t as f32),
                fan_pct: d.fan_speed(0).ok().map(|p| p as f32),
                throttle: d.current_throttle_reasons().map(throttle_names).unwrap_or_default(),
                engines,
                processes_sampled: per_process,
                processes,
                ..Default::default()
            });
        }
        out
    }
}

/// Procesos con memoria en la GPU y su uso por motor desde la última muestra.
fn processes(d: &nvml_wrapper::Device, last_ts: &mut HashMap<u32, u64>, idx: u32) -> Vec<GpuProc> {
    let mut procs: HashMap<u32, GpuProc> = HashMap::new();
    let mut add = |pid: u32, mem: &UsedGpuMemory| {
        let p = procs.entry(pid).or_insert_with(|| GpuProc {
            pid,
            name: fs::read_to_string(format!("/proc/{pid}/comm"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            ..Default::default()
        });
        if let UsedGpuMemory::Used(b) = mem {
            p.vram = p.vram.max(*b);
        }
    };
    for p in d
        .running_graphics_processes()
        .unwrap_or_default()
        .iter()
        .chain(d.running_compute_processes().unwrap_or_default().iter())
    {
        add(p.pid, &p.used_gpu_memory);
    }
    let since = last_ts.get(&idx).copied();
    if let Ok(samples) = d.process_utilization_stats(since) {
        for s in samples {
            last_ts.insert(idx, s.timestamp.max(last_ts.get(&idx).copied().unwrap_or(0)));
            let p = procs.entry(s.pid).or_insert_with(|| GpuProc {
                pid: s.pid,
                ..Default::default()
            });
            p.engines = vec![
                GpuEngine {
                    name: "3d".into(),
                    busy: s.sm_util as f32,
                },
                GpuEngine {
                    name: "enc".into(),
                    busy: s.enc_util as f32,
                },
                GpuEngine {
                    name: "dec".into(),
                    busy: s.dec_util as f32,
                },
            ];
        }
    }
    procs.into_values().collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn bus_id() {
        assert_eq!(super::normalize_bus_id("00000000:01:00.0"), "0000:01:00.0");
        assert_eq!(super::normalize_bus_id("0000:2D:00.0"), "0000:2d:00.0");
    }
}
