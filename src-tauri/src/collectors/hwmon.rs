use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use super::{read_trim, read_u64};

#[derive(Serialize, Clone, Debug)]
pub struct TempReading {
    pub label: String,
    pub celsius: f32,
}

/// Busca el primer dispositivo hwmon cuyo `name` esté en la lista (por orden de preferencia).
pub fn find_by_name(names: &[&str]) -> Option<PathBuf> {
    let entries: Vec<PathBuf> = fs::read_dir("/sys/class/hwmon")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    for wanted in names {
        for p in &entries {
            if read_trim(p.join("name")).as_deref() == Some(*wanted) {
                return Some(p.clone());
            }
        }
    }
    None
}

/// Todas las temperaturas de un hwmon, ordenadas por índice.
pub fn read_temps(dir: &Path) -> Vec<TempReading> {
    let mut idx: Vec<u32> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let n = e.file_name().into_string().ok()?;
                    n.strip_prefix("temp")?.strip_suffix("_input")?.parse().ok()
                })
                .collect()
        })
        .unwrap_or_default();
    idx.sort_unstable();
    idx.into_iter()
        .filter_map(|i| {
            let milli = read_u64(dir.join(format!("temp{i}_input")))?;
            let label = read_trim(dir.join(format!("temp{i}_label"))).unwrap_or_else(|| format!("temp{i}"));
            Some(TempReading { label, celsius: milli as f32 / 1000.0 })
        })
        .collect()
}
