pub mod cpu;
pub mod disks;
pub mod energy;
pub mod games;
pub mod gpu;
pub mod hwmon;
pub mod membench;
pub mod membw;
pub mod memory;
pub mod network;
pub mod pressure;
pub mod sensors;
pub mod processes;
pub mod system;
pub mod ups;

use std::fs;
use std::path::Path;

/// Lee un fichero de /proc o /sys y devuelve su contenido sin espacios finales.
pub fn read_trim<P: AsRef<Path>>(path: P) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

pub fn read_u64<P: AsRef<Path>>(path: P) -> Option<u64> {
    read_trim(path)?.parse().ok()
}
