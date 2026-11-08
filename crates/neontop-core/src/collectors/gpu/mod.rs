//! GPU: un backend por fabricante (AMD vía amdgpu, NVIDIA vía NVML, Intel vía i915/xe) más
//! el uso por proceso y por motor leído de /proc/*/fdinfo (estándar DRM, AMD e Intel).

mod amd;
mod fdinfo;
mod intel;
mod nvidia;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::read_trim;

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GpuEngine {
    /// Nombre del motor tal como lo da el driver (gfx, compute, dec, render, video…).
    pub name: String,
    pub busy: f32,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GpuProc {
    pub pid: u32,
    pub name: String,
    /// Uso por motor (%).
    pub engines: Vec<GpuEngine>,
    pub vram: u64,
    /// Memoria del sistema mapeada por la GPU (GTT en AMD).
    pub gtt: u64,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GpuSnapshot {
    pub name: String,
    /// "amd" | "nvidia" | "intel"
    pub vendor: String,
    pub pci_slot: String,
    pub driver: String,
    pub driver_version: Option<String>,
    /// iGPU integrada (comparte la RAM del sistema).
    pub integrated: bool,
    /// % de uso del motor gráfico.
    pub busy: Option<f32>,
    /// % de actividad del controlador de memoria.
    pub mem_busy: Option<f32>,
    pub vram_used: u64,
    pub vram_total: u64,
    pub gtt_used: u64,
    pub gtt_total: u64,
    pub vram_type: Option<String>,
    pub bus_width: Option<u32>,
    pub mem_clock: Option<f32>,
    pub mem_clock_max: Option<f32>,
    /// Ancho de banda teórico de la VRAM a reloj máximo (bytes/s).
    pub vram_peak_bw: Option<f64>,
    /// Estimación del tráfico de VRAM: actividad del controlador × ancho de banda al reloj actual.
    pub vram_bw: Option<f64>,
    pub pcie_speed: Option<f32>,
    pub pcie_width: Option<u32>,
    pub pcie_max_speed: Option<f32>,
    pub pcie_max_width: Option<u32>,
    pub pcie_peak_bw: Option<f64>,
    /// Tráfico PCIe real (bytes/s); solo NVIDIA lo expone.
    pub pcie_rx: Option<f64>,
    pub pcie_tx: Option<f64>,
    pub gfx_clock: Option<f32>,
    pub gfx_clock_max: Option<f32>,
    pub power: Option<f32>,
    pub power_limit: Option<f32>,
    pub temp_edge: Option<f32>,
    pub temp_hotspot: Option<f32>,
    pub temp_mem: Option<f32>,
    pub fan_rpm: Option<u32>,
    pub fan_pct: Option<f32>,
    /// Motivos por los que la GPU está bajando relojes (NVIDIA).
    pub throttle: Vec<String>,
    pub engines: Vec<GpuEngine>,
    pub processes: Vec<GpuProc>,
    /// false si el uso por proceso no se ha medido (pestaña GPU no visible).
    pub processes_sampled: bool,
}

/// Mínimo entre dos lecturas de fdinfo: recorrer todos los descriptores cuesta ~10 ms.
const FDINFO_MIN: Duration = Duration::from_secs(1);

pub struct GpuCollector {
    amd: Vec<amd::AmdGpu>,
    intel: Vec<intel::IntelGpu>,
    nvidia: Option<nvidia::Nvidia>,
    fdinfo: fdinfo::FdInfo,
    last_fdinfo: Option<Instant>,
    cached: fdinfo::Usage,
}

impl Default for GpuCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuCollector {
    pub fn new() -> Self {
        let mut amd = Vec::new();
        let mut intel = Vec::new();
        for (card, dev) in drm_cards() {
            match read_trim(dev.join("vendor")).as_deref() {
                Some("0x1002") => amd.extend(amd::AmdGpu::open(dev)),
                Some("0x8086") => intel.extend(intel::IntelGpu::open(&card, dev)),
                _ => {}
            }
        }
        GpuCollector { amd, intel, nvidia: nvidia::Nvidia::init(), fdinfo: fdinfo::FdInfo::new(), last_fdinfo: None, cached: Default::default() }
    }

    /// `per_process`: medir uso por proceso (solo cuando la vista lo necesita).
    pub fn sample(&mut self, per_process: bool) -> Vec<GpuSnapshot> {
        // En Intel el uso global solo se obtiene de fdinfo, así que se lee siempre que haya una.
        let need = per_process || !self.intel.is_empty();
        if need && self.last_fdinfo.is_none_or(|t| t.elapsed() >= FDINFO_MIN) {
            self.cached = self.fdinfo.sample();
            self.last_fdinfo = Some(Instant::now());
        }
        if !need {
            self.last_fdinfo = None;
        }

        let mut out: Vec<GpuSnapshot> = self.amd.iter().map(|g| g.sample()).collect();
        out.extend(self.intel.iter_mut().map(|g| g.sample()));
        for s in &mut out {
            self.attach(s, per_process);
        }
        if let Some(nv) = &mut self.nvidia {
            out.extend(nv.sample(per_process));
        }
        out
    }

    /// Añade motores y procesos (de fdinfo) a la GPU con esa ranura PCI.
    fn attach(&self, s: &mut GpuSnapshot, per_process: bool) {
        let Some(dev) = self.cached.devices.get(&s.pci_slot) else { return };
        // los motores de fdinfo son más detallados que los contadores globales del driver
        if !dev.engines.is_empty() {
            s.engines = dev.engines.clone();
        }
        // Intel no tiene contador de uso global: el motor más ocupado hace de "uso GPU"
        if s.busy.is_none() {
            s.busy = dev.engines.iter().map(|e| e.busy).reduce(f32::max);
        }
        // Intel no publica la memoria total en uso: se suma la de los procesos visibles
        if s.vendor == "intel" {
            s.vram_used = dev.processes.iter().map(|p| p.vram).sum();
            s.gtt_used = dev.processes.iter().map(|p| p.gtt).sum();
        }
        if per_process {
            s.processes = dev.processes.clone();
            s.processes_sampled = true;
        }
    }
}

/// Tarjetas DRM (`cardN`) con su directorio de dispositivo.
fn drm_cards() -> Vec<(PathBuf, PathBuf)> {
    let Ok(rd) = fs::read_dir("/sys/class/drm") else { return Vec::new() };
    let mut cards: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            n.starts_with("card") && n.len() > 4 && n[4..].chars().all(|c| c.is_ascii_digit())
        })
        .collect();
    cards.sort();
    cards.into_iter().map(|c| (c.clone(), c.join("device"))).collect()
}

/// Ranura PCI ("0000:2d:00.0") del dispositivo.
fn pci_slot(dev: &Path) -> String {
    fs::canonicalize(dev).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default()
}

/// Nombre comercial desde pci.ids: "Navi 44 [Radeon RX 9060 XT]" → "Radeon RX 9060 XT".
fn pci_name(dev: &Path) -> Option<String> {
    let vendor = read_trim(dev.join("vendor"))?.trim_start_matches("0x").to_lowercase();
    let device = read_trim(dev.join("device"))?.trim_start_matches("0x").to_lowercase();
    // pci.ids trae algunos nombres que no son UTF-8 válido: lectura tolerante
    let raw = ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids"].iter().find_map(|p| fs::read(p).ok())?;
    let db = String::from_utf8_lossy(&raw);
    let mut in_vendor = false;
    for line in db.lines() {
        // hay comentarios dentro de la lista de un fabricante: no cierran su sección
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            in_vendor = line.starts_with(&format!("{vendor} "));
            continue;
        }
        if in_vendor && !line.starts_with("\t\t") {
            let l = line.trim_start();
            if let Some(full) = l.strip_prefix(&device) {
                let full = full.trim();
                return Some(match (full.find('['), full.rfind(']')) {
                    (Some(a), Some(b)) if b > a => full[a + 1..b].to_string(),
                    _ => full.to_string(),
                });
            }
        }
    }
    None
}

fn render_node(dev: &Path) -> Option<PathBuf> {
    fs::read_dir(dev.join("drm")).ok()?.flatten().find_map(|e| {
        let n = e.file_name().into_string().ok()?;
        n.starts_with("renderD").then(|| PathBuf::from("/dev/dri").join(n))
    })
}

/// Ancho de banda teórico de un enlace PCIe (bytes/s por sentido).
fn pcie_bw(gt_s: f32, width: u32) -> f64 {
    // PCIe 1/2 codifican 8b/10b; desde PCIe 3, 128b/130b
    gt_s as f64 * 1e9 * width as f64 * if gt_s < 8.0 { 0.8 } else { 128.0 / 130.0 } / 8.0
}

/// Enlace PCIe actual y máximo desde sysfs.
fn pcie_link(dev: &Path) -> (Option<f32>, Option<u32>, Option<f32>, Option<u32>) {
    let speed = |f: &str| read_trim(dev.join(f)).and_then(|v| v.split_whitespace().next()?.parse::<f32>().ok());
    let width = |f: &str| read_trim(dev.join(f)).and_then(|v| v.parse::<u32>().ok());
    (speed("current_link_speed"), width("current_link_width"), speed("max_link_speed"), width("max_link_width"))
}

fn driver_name(dev: &Path) -> String {
    fs::read_link(dev.join("driver")).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn samples_gpus() {
        let mut c = super::GpuCollector::new();
        c.sample(true);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        for g in c.sample(true) {
            eprintln!("{} [{} {}] busy {:?} mem {:?} vram {:.0} MB", g.name, g.vendor, g.driver, g.busy, g.mem_busy, g.vram_used as f64 / 1e6);
            for e in &g.engines {
                eprintln!("   motor {:<10} {:>5.1}%", e.name, e.busy);
            }
            let mut ps = g.processes.clone();
            ps.sort_by(|a, b| b.vram.cmp(&a.vram));
            for p in ps.iter().take(6) {
                eprintln!("   {:>7} {:<18} vram {:>6.0} MB gtt {:>5.0} MB {:?}", p.pid, p.name, p.vram as f64 / 1e6, p.gtt as f64 / 1e6, p.engines.iter().map(|e| (&e.name, e.busy)).collect::<Vec<_>>());
            }
        }
    }
}
