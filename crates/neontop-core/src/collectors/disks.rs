use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::hwmon::{self, TempReading};
use super::{read_trim, read_u64};

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiskSnapshot {
    pub name: String,
    pub model: String,
    /// "nvme" | "ssd" | "hdd" | "usb" | "virtual"
    pub kind: String,
    pub size: u64,
    pub removable: bool,
    pub read_rate: f64,
    pub write_rate: f64,
    pub read_iops: f64,
    pub write_iops: f64,
    /// % del tiempo con E/S en curso.
    pub util: f32,
    /// Latencia media por operación en el intervalo (ms).
    pub read_lat: f32,
    pub write_lat: f32,
    /// Profundidad media de cola.
    pub queue: f32,
    pub read_total: u64,
    pub write_total: u64,
    pub temps: Vec<TempReading>,
    pub partitions: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FsSnapshot {
    pub device: String,
    /// Disco al que pertenece (p. ej. nvme1n1), si se conoce.
    pub disk: Option<String>,
    pub mount: String,
    /// Otros puntos de montaje del mismo sistema de ficheros (subvolúmenes btrfs…).
    pub other_mounts: Vec<String>,
    pub fstype: String,
    pub total: u64,
    pub used: u64,
    pub avail: u64,
    pub inodes_total: u64,
    pub inodes_free: u64,
    pub readonly: bool,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiskIoSnapshot {
    pub disks: Vec<DiskSnapshot>,
    pub filesystems: Vec<FsSnapshot>,
}

#[derive(Clone, Copy, Default)]
struct Stat {
    reads: u64,
    read_sectors: u64,
    read_ms: u64,
    writes: u64,
    write_sectors: u64,
    write_ms: u64,
    io_ms: u64,
    weighted_ms: u64,
}

struct DiskStatic {
    name: String,
    model: String,
    kind: String,
    removable: bool,
    partitions: Vec<String>,
    hwmon: Option<PathBuf>,
}

pub struct DiskCollector {
    disks: Vec<DiskStatic>,
    prev: HashMap<String, Stat>,
    prev_at: Instant,
    last_scan: Instant,
}

/// Sistemas de ficheros locales que se muestran (se omiten tmpfs, proc, overlay…). Los de red
/// (nfs, cifs) quedan fuera a propósito: un statvfs sobre un servidor caído bloquearía el muestreo.
const REAL_FS: &[&str] = &["ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "vfat", "exfat", "ntfs", "ntfs3", "fuseblk", "zfs", "jfs", "reiserfs", "bcachefs"];

fn scan() -> Vec<DiskStatic> {
    let Ok(rd) = fs::read_dir("/sys/block") else { return Vec::new() };
    let hwmons: Vec<(PathBuf, PathBuf)> = fs::read_dir("/sys/class/hwmon")
        .map(|r| r.flatten().filter_map(|e| Some((fs::canonicalize(e.path().join("device")).ok()?, e.path()))).collect())
        .unwrap_or_default();

    let mut out: Vec<DiskStatic> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if name.starts_with("loop") || name.starts_with("ram") {
                return None;
            }
            let base = e.path();
            let real = fs::canonicalize(&base).ok()?;
            let real_s = real.to_string_lossy();
            let rotational = read_u64(base.join("queue/rotational")) == Some(1);
            let virt = real_s.contains("/virtual/");
            let kind = if virt {
                "virtual"
            } else if real_s.contains("/usb") {
                "usb"
            } else if name.starts_with("nvme") {
                "nvme"
            } else if rotational {
                "hdd"
            } else {
                "ssd"
            };
            let vendor = read_trim(base.join("device/vendor")).unwrap_or_default();
            let model = read_trim(base.join("device/model")).unwrap_or_default();
            let model = match (vendor.as_str(), model.as_str()) {
                (_, "") if name.starts_with("zram") => "zram (swap comprimida)".to_string(),
                ("", m) => m.to_string(),
                // los discos NVMe/ATA no suelen traer vendor útil; los USB sí ("JMicron", "Seagate"…)
                (v, m) if v.eq_ignore_ascii_case("ATA") || m.starts_with(v) => m.to_string(),
                (v, m) => format!("{v} {m}"),
            };
            let mut partitions: Vec<String> = fs::read_dir(&base)
                .map(|r| r.flatten().filter_map(|p| p.file_name().into_string().ok()).filter(|p| p.starts_with(&name)).collect())
                .unwrap_or_default();
            partitions.sort();
            // el sensor de un NVMe cuelga del controlador PCI; el del disco está por debajo en el árbol
            let dev = fs::canonicalize(base.join("device")).ok();
            let hwmon = dev.and_then(|d| hwmons.iter().find(|(hd, _)| d.starts_with(hd)).map(|(_, h)| h.clone()));
            Some(DiskStatic {
                removable: read_u64(base.join("removable")) == Some(1),
                name,
                model,
                kind: kind.into(),
                partitions,
                hwmon,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn statvfs(path: &str) -> Option<libc::statvfs> {
    let c = CString::new(path).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` es una cadena C válida y `s` una estructura del tamaño correcto.
    (unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0).then_some(s)
}

/// Octales como `\040` (espacio) en /proc/mounts. Se trabaja en bytes para no romper
/// rutas con caracteres UTF-8 (p. ej. "Música").
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && b.len() >= i + 4 && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
            out.push((b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0'));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn filesystems(disks: &[DiskStatic]) -> Vec<FsSnapshot> {
    let text = fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut by_dev: Vec<FsSnapshot> = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 || !REAL_FS.contains(&f[2]) {
            continue;
        }
        let device = unescape(f[0]);
        let mount = unescape(f[1]);
        if let Some(existing) = by_dev.iter_mut().find(|x| x.device == device) {
            existing.other_mounts.push(mount);
            continue;
        }
        let Some(st) = statvfs(&mount) else { continue };
        let frsize = st.f_frsize as u64;
        let total = st.f_blocks as u64 * frsize;
        let avail = st.f_bavail as u64 * frsize;
        let free = st.f_bfree as u64 * frsize;
        let dev_name = device.rsplit('/').next().unwrap_or("");
        let disk = disks
            .iter()
            .find(|d| d.name == dev_name || d.partitions.iter().any(|p| p == dev_name))
            .map(|d| d.name.clone());
        by_dev.push(FsSnapshot {
            disk,
            mount,
            other_mounts: Vec::new(),
            fstype: f[2].to_string(),
            total,
            used: total.saturating_sub(free),
            avail,
            inodes_total: st.f_files as u64,
            inodes_free: st.f_ffree as u64,
            readonly: f[3].split(',').any(|o| o == "ro"),
            device,
        });
    }
    by_dev
}

fn read_stats() -> HashMap<String, Stat> {
    let text = fs::read_to_string("/proc/diskstats").unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 14 {
                return None;
            }
            let n = |i: usize| f[i].parse::<u64>().unwrap_or(0);
            Some((
                f[2].to_string(),
                Stat {
                    reads: n(3),
                    read_sectors: n(5),
                    read_ms: n(6),
                    writes: n(7),
                    write_sectors: n(9),
                    write_ms: n(10),
                    io_ms: n(12),
                    weighted_ms: n(13),
                },
            ))
        })
        .collect()
}

impl DiskCollector {
    pub fn new() -> Self {
        DiskCollector { disks: scan(), prev: read_stats(), prev_at: Instant::now(), last_scan: Instant::now() }
    }

    pub fn sample(&mut self) -> DiskIoSnapshot {
        // los discos USB van y vienen: se reescanea cada 5 s
        if self.last_scan.elapsed().as_secs() >= 5 {
            self.disks = scan();
            self.last_scan = Instant::now();
        }
        let now = Instant::now();
        let secs = now.duration_since(self.prev_at).as_secs_f64().max(1e-3);
        let ms = secs * 1000.0;
        self.prev_at = now;
        let cur = read_stats();

        let disks = self
            .disks
            .iter()
            .map(|d| {
                let c = cur.get(&d.name).copied().unwrap_or_default();
                let p = self.prev.get(&d.name).copied().unwrap_or(c);
                let dl = |a: u64, b: u64| a.saturating_sub(b);
                let reads = dl(c.reads, p.reads);
                let writes = dl(c.writes, p.writes);
                DiskSnapshot {
                    name: d.name.clone(),
                    model: d.model.clone(),
                    kind: d.kind.clone(),
                    size: read_u64(Path::new("/sys/block").join(&d.name).join("size")).unwrap_or(0) * 512,
                    removable: d.removable,
                    // /proc/diskstats cuenta siempre en sectores de 512 bytes
                    read_rate: dl(c.read_sectors, p.read_sectors) as f64 * 512.0 / secs,
                    write_rate: dl(c.write_sectors, p.write_sectors) as f64 * 512.0 / secs,
                    read_iops: reads as f64 / secs,
                    write_iops: writes as f64 / secs,
                    util: (dl(c.io_ms, p.io_ms) as f64 / ms * 100.0).min(100.0) as f32,
                    read_lat: if reads > 0 { dl(c.read_ms, p.read_ms) as f32 / reads as f32 } else { 0.0 },
                    write_lat: if writes > 0 { dl(c.write_ms, p.write_ms) as f32 / writes as f32 } else { 0.0 },
                    queue: (dl(c.weighted_ms, p.weighted_ms) as f64 / ms) as f32,
                    read_total: c.read_sectors * 512,
                    write_total: c.write_sectors * 512,
                    temps: d.hwmon.as_deref().map(hwmon::read_temps).unwrap_or_default(),
                    partitions: d.partitions.clone(),
                }
            })
            .collect();
        self.prev = cur;
        DiskIoSnapshot { disks, filesystems: filesystems(&self.disks) }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn samples_disks() {
        let mut c = super::DiskCollector::new();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let s = c.sample();
        for d in &s.disks {
            eprintln!(
                "{:<8} {:<5} {:<28} {:>6.0} GB util {:>5.1}% r {:>9.0} B/s w {:>9.0} B/s temps {:?}",
                d.name,
                d.kind,
                d.model,
                d.size as f64 / 1e9,
                d.util,
                d.read_rate,
                d.write_rate,
                d.temps.iter().map(|t| (&t.label, t.celsius)).collect::<Vec<_>>()
            );
        }
        for f in &s.filesystems {
            eprintln!("{:<16} {:<22} {:<6} {:>6.0}/{:<6.0} GB disco {:?} +{} montajes", f.device, f.mount, f.fstype, f.used as f64 / 1e9, f.total as f64 / 1e9, f.disk, f.other_mounts.len());
        }
    }
}
