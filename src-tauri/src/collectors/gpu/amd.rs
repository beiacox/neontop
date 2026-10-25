//! Backend AMD (amdgpu): sysfs, la tabla binaria `gpu_metrics` y el ioctl AMDGPU_INFO.

use std::fs;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use super::{driver_name, pci_name, pci_slot, pcie_bw, pcie_link, render_node, GpuEngine, GpuSnapshot};
use crate::collectors::{read_trim, read_u64};

/// AMDGPU_IDS_FLAGS_FUSION: APU (GPU integrada en el procesador).
const IDS_FLAGS_FUSION: u64 = 0x1;

pub struct AmdGpu {
    dir: PathBuf,
    name: String,
    pci_slot: String,
    driver: String,
    vram_type: Option<u32>,
    bus_width: Option<u32>,
    max_mem_khz: Option<u64>,
    integrated: bool,
}

impl AmdGpu {
    pub fn open(dev: PathBuf) -> Option<Self> {
        let info = render_node(&dev).and_then(|n| amdgpu_dev_info(&n));
        Some(AmdGpu {
            name: pci_name(&dev).unwrap_or_else(|| "AMD Radeon".into()),
            pci_slot: pci_slot(&dev),
            driver: driver_name(&dev),
            vram_type: info.map(|i| i.vram_type),
            bus_width: info.map(|i| i.vram_bit_width).filter(|w| *w > 0),
            max_mem_khz: info.map(|i| i.max_memory_clock).filter(|c| *c > 0),
            integrated: info.is_some_and(|i| i.ids_flags & IDS_FLAGS_FUSION != 0),
            dir: dev,
        })
    }

    pub fn sample(&self) -> GpuSnapshot {
        let d = &self.dir;
        let metrics = fs::read(d.join("gpu_metrics")).ok().and_then(|b| parse_gpu_metrics(&b));
        let (mem_clock, mem_clock_max) = dpm_current_and_max(&d.join("pp_dpm_mclk"));
        let (gfx_clock_dpm, gfx_clock_max) = dpm_current_and_max(&d.join("pp_dpm_sclk"));
        let ops = self.vram_type.map(ops_per_clock);
        let bytes_per_s = |mhz: f64| -> Option<f64> { Some(mhz * 1e6 * ops? as f64 * self.bus_width? as f64 / 8.0) };

        let vram_peak_bw = self.max_mem_khz.and_then(|k| bytes_per_s(k as f64 / 1000.0));
        let mem_busy = metrics
            .as_ref()
            .and_then(|m| m.umc_activity)
            .or_else(|| read_u64(d.join("mem_busy_percent")).map(|v| v as f32));
        // en una APU la "VRAM" es RAM del sistema: la estimación no tendría sentido
        let vram_bw = match (mem_busy, mem_clock, self.integrated) {
            (Some(b), Some(clk), false) => bytes_per_s(clk as f64).map(|bw| bw * b as f64 / 100.0),
            _ => None,
        };
        let (pcie_speed, pcie_width, pcie_max_speed, pcie_max_width) = pcie_link(d);
        let hwmon = fs::read_dir(d.join("hwmon")).ok().and_then(|mut r| r.next()).and_then(|e| e.ok()).map(|e| e.path());
        let busy = read_u64(d.join("gpu_busy_percent")).map(|v| v as f32);

        // motores globales; si se leen los fdinfo, se sustituyen por su desglose
        let mut engines = Vec::new();
        if let Some(b) = busy {
            engines.push(GpuEngine { name: "gfx".into(), busy: b });
        }
        if let Some(mm) = metrics.as_ref().and_then(|m| m.mm_activity) {
            engines.push(GpuEngine { name: "vcn".into(), busy: mm });
        }

        let fan_max = hwmon.as_ref().and_then(|h| read_u64(h.join("fan1_max")));
        let fan_rpm = hwmon.as_ref().and_then(|h| read_u64(h.join("fan1_input"))).map(|v| v as u32);

        GpuSnapshot {
            name: self.name.clone(),
            vendor: "amd".into(),
            pci_slot: self.pci_slot.clone(),
            driver: self.driver.clone(),
            driver_version: read_trim("/sys/module/amdgpu/version"),
            integrated: self.integrated,
            busy,
            mem_busy,
            vram_used: read_u64(d.join("mem_info_vram_used")).unwrap_or(0),
            vram_total: read_u64(d.join("mem_info_vram_total")).unwrap_or(0),
            gtt_used: read_u64(d.join("mem_info_gtt_used")).unwrap_or(0),
            gtt_total: read_u64(d.join("mem_info_gtt_total")).unwrap_or(0),
            vram_type: self.vram_type.map(|t| vram_type_name(t).to_string()),
            bus_width: self.bus_width,
            mem_clock,
            mem_clock_max: mem_clock_max.or(self.max_mem_khz.map(|k| k as f32 / 1000.0)),
            vram_peak_bw,
            vram_bw,
            pcie_speed,
            pcie_width,
            pcie_max_speed,
            pcie_max_width,
            pcie_peak_bw: pcie_speed.zip(pcie_width).map(|(s, w)| pcie_bw(s, w)),
            gfx_clock: metrics.as_ref().and_then(|m| m.gfx_clock).or(gfx_clock_dpm),
            gfx_clock_max,
            power: metrics
                .as_ref()
                .and_then(|m| m.power)
                .or_else(|| hwmon.as_ref().and_then(|h| read_u64(h.join("power1_average")).map(|uw| uw as f32 / 1e6))),
            power_limit: hwmon.as_ref().and_then(|h| read_u64(h.join("power1_cap"))).map(|uw| uw as f32 / 1e6),
            temp_edge: metrics.as_ref().and_then(|m| m.temp_edge),
            temp_hotspot: metrics.as_ref().and_then(|m| m.temp_hotspot),
            temp_mem: metrics.as_ref().and_then(|m| m.temp_mem),
            fan_rpm,
            fan_pct: fan_rpm.zip(fan_max).filter(|(_, m)| *m > 0).map(|(r, m)| r as f32 / m as f32 * 100.0),
            engines,
            ..Default::default()
        }
    }
}

/// Nivel DPM activo (marcado con `*`) y el máximo, en MHz.
fn dpm_current_and_max(path: &Path) -> (Option<f32>, Option<f32>) {
    let Some(text) = read_trim(path) else { return (None, None) };
    let mhz = |l: &str| l.split_whitespace().nth(1)?.trim_end_matches("Mhz").trim_end_matches("MHz").parse::<f32>().ok();
    let cur = text.lines().filter(|l| l.ends_with('*')).find_map(mhz);
    let max = text.lines().filter_map(mhz).reduce(f32::max);
    (cur, max)
}

/// Transferencias por ciclo de reloj de memoria según el tipo de VRAM (mismos valores que Mesa).
fn ops_per_clock(vram_type: u32) -> u32 {
    match vram_type {
        5 => 4,   // GDDR5
        9 => 16,  // GDDR6
        12 => 4,  // LPDDR5
        _ => 2,   // DDR*, HBM, GDDR1-4
    }
}

fn vram_type_name(t: u32) -> &'static str {
    match t {
        1 => "GDDR1",
        2 => "DDR2",
        3 => "GDDR3",
        4 => "GDDR4",
        5 => "GDDR5",
        6 => "HBM",
        7 => "DDR3",
        8 => "DDR4",
        9 => "GDDR6",
        10 => "DDR5",
        11 => "LPDDR4",
        12 => "LPDDR5",
        13 => "HBM3E",
        _ => "desconocida",
    }
}

// ---------------------------------------------------------------- ioctl AMDGPU_INFO

#[derive(Clone, Copy, Debug)]
struct DevInfo {
    /// Solo se usa para comprobar en los tests que el ioctl devolvió la tarjeta correcta.
    #[allow(dead_code)]
    device_id: u32,
    max_memory_clock: u64,
    vram_type: u32,
    vram_bit_width: u32,
    ids_flags: u64,
}

#[repr(C)]
struct DrmAmdgpuInfo {
    return_pointer: u64,
    return_size: u32,
    query: u32,
    _union: [u32; 4],
}

const AMDGPU_INFO_DEV_INFO: u32 = 0x16;
/// _IOW('d', DRM_COMMAND_BASE + DRM_AMDGPU_INFO, struct drm_amdgpu_info)
const DRM_IOCTL_AMDGPU_INFO: libc::c_ulong = 0x4020_6445;

/// Pide al driver la información del dispositivo (ancho de bus y tipo de VRAM no están en sysfs).
fn amdgpu_dev_info(render: &Path) -> Option<DevInfo> {
    let file = fs::File::open(render).ok()?;
    let mut buf = [0u8; 512];
    let mut req = DrmAmdgpuInfo {
        return_pointer: buf.as_mut_ptr() as u64,
        return_size: buf.len() as u32,
        query: AMDGPU_INFO_DEV_INFO,
        _union: [0; 4],
    };
    // SAFETY: el kernel escribe como mucho `return_size` bytes en `buf`, que vive durante la llamada.
    let r = unsafe { libc::ioctl(file.as_raw_fd(), DRM_IOCTL_AMDGPU_INFO, &mut req as *mut DrmAmdgpuInfo) };
    if r != 0 {
        return None;
    }
    let u32_at = |o: usize| u32::from_ne_bytes(buf[o..o + 4].try_into().unwrap());
    let u64_at = |o: usize| u64::from_ne_bytes(buf[o..o + 8].try_into().unwrap());
    // desplazamientos de struct drm_amdgpu_info_device (include/uapi/drm/amdgpu_drm.h)
    Some(DevInfo {
        device_id: u32_at(0),
        max_memory_clock: u64_at(40),
        vram_type: u32_at(176),
        vram_bit_width: u32_at(180),
        ids_flags: u64_at(136),
    })
}

// ---------------------------------------------------------------- gpu_metrics

#[derive(Default, Debug)]
struct Metrics {
    temp_edge: Option<f32>,
    temp_hotspot: Option<f32>,
    temp_mem: Option<f32>,
    umc_activity: Option<f32>,
    power: Option<f32>,
    gfx_clock: Option<f32>,
    /// Actividad del bloque multimedia (VCN: codificación/decodificación de vídeo).
    mm_activity: Option<f32>,
}

/// Interpreta la tabla binaria `gpu_metrics` (formato 1.1–1.3 de GPUs dedicadas).
/// Los campos no soportados por la tarjeta valen 0xFFFF.
fn parse_gpu_metrics(b: &[u8]) -> Option<Metrics> {
    if b.len() < 78 {
        return None;
    }
    let (format, content) = (b[2], b[3]);
    if format != 1 || !(1..=3).contains(&content) {
        return None;
    }
    let u16_at = |o: usize| {
        let v = u16::from_ne_bytes([b[o], b[o + 1]]);
        (v != 0xFFFF).then_some(v as f32)
    };
    Some(Metrics {
        temp_edge: u16_at(4),
        temp_hotspot: u16_at(6),
        temp_mem: u16_at(8),
        umc_activity: u16_at(18),
        power: u16_at(22),
        gfx_clock: u16_at(54),
        mm_activity: u16_at(20),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amd_dev_info() {
        for (_, dev) in super::super::drm_cards() {
            if read_trim(dev.join("vendor")).as_deref() == Some("0x1002") {
                let info = render_node(&dev).and_then(|n| amdgpu_dev_info(&n));
                eprintln!("{:?} sysfs device={:?}", info, read_trim(dev.join("device")));
                assert_eq!(info.map(|i| format!("0x{:04x}", i.device_id)), read_trim(dev.join("device")));
            }
        }
    }
}
