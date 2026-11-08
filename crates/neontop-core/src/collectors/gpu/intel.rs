//! Backend Intel (i915 y xe). Estos drivers no publican un "% de uso" global en sysfs: se
//! obtiene de fdinfo (motor más ocupado). Relojes de sysfs y potencia de hwmon (solo Arc).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{driver_name, pci_name, pci_slot, pcie_bw, pcie_link, GpuSnapshot};
use crate::collectors::read_u64;

pub struct IntelGpu {
    card: PathBuf,
    dir: PathBuf,
    name: String,
    pci_slot: String,
    driver: String,
    integrated: bool,
    hwmon: Option<PathBuf>,
    /// Energía acumulada (µJ) para calcular potencia media.
    prev_energy: Option<(u64, Instant)>,
}

/// Primer valor legible de una lista de rutas candidatas (i915 y xe usan nombres distintos).
fn first_u64(paths: &[PathBuf]) -> Option<u64> {
    paths.iter().find_map(read_u64)
}

impl IntelGpu {
    pub fn open(card: &Path, dev: PathBuf) -> Option<Self> {
        let slot = pci_slot(&dev);
        Some(IntelGpu {
            card: card.to_path_buf(),
            name: pci_name(&dev).unwrap_or_else(|| "Intel Graphics".into()),
            // las integradas viven siempre en el bus 0 (00:02.0)
            integrated: slot.split(':').nth(1) == Some("00"),
            driver: driver_name(&dev),
            hwmon: fs::read_dir(dev.join("hwmon")).ok().and_then(|mut r| r.next()).and_then(|e| e.ok()).map(|e| e.path()),
            pci_slot: slot,
            prev_energy: None,
            dir: dev,
        })
    }

    pub fn sample(&mut self) -> GpuSnapshot {
        let c = &self.card;
        let gt = self.dir.join("tile0/gt0/freq0");
        let gfx_clock = first_u64(&[c.join("gt_act_freq_mhz"), gt.join("act_freq"), c.join("gt_cur_freq_mhz"), gt.join("cur_freq")]).map(|v| v as f32);
        let gfx_clock_max = first_u64(&[c.join("gt_RP0_freq_mhz"), gt.join("rp0_freq"), c.join("gt_max_freq_mhz"), gt.join("max_freq")]).map(|v| v as f32);

        let power = self.hwmon.as_ref().and_then(|h| {
            let e = read_u64(h.join("energy1_input"))?;
            let now = Instant::now();
            let w = self.prev_energy.map(|(pe, pt)| e.saturating_sub(pe) as f64 / now.duration_since(pt).as_secs_f64() / 1e6);
            self.prev_energy = Some((e, now));
            w.map(|w| w as f32)
        });

        let (pcie_speed, pcie_width, pcie_max_speed, pcie_max_width) = if self.integrated { (None, None, None, None) } else { pcie_link(&self.dir) };

        GpuSnapshot {
            name: self.name.clone(),
            vendor: "intel".into(),
            pci_slot: self.pci_slot.clone(),
            driver: self.driver.clone(),
            driver_version: None,
            integrated: self.integrated,
            gfx_clock,
            gfx_clock_max,
            power,
            power_limit: self.hwmon.as_ref().and_then(|h| read_u64(h.join("power1_max"))).map(|uw| uw as f32 / 1e6),
            temp_edge: self.hwmon.as_ref().and_then(|h| read_u64(h.join("temp1_input"))).map(|m| m as f32 / 1000.0),
            fan_rpm: self.hwmon.as_ref().and_then(|h| read_u64(h.join("fan1_input"))).map(|v| v as u32),
            pcie_speed,
            pcie_width,
            pcie_max_speed,
            pcie_max_width,
            pcie_peak_bw: pcie_speed.zip(pcie_width).map(|(s, w)| pcie_bw(s, w)),
            ..Default::default()
        }
    }
}
