use serde::{Deserialize, Serialize};
use std::fs;

/// Pressure Stall Information (avg10, en %). `full` no existe para CPU en kernels antiguos.
#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Psi {
    pub some: f32,
    pub full: f32,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PressureSnapshot {
    pub cpu: Psi,
    pub memory: Psi,
    pub io: Psi,
}

fn read(res: &str) -> Psi {
    let mut p = Psi::default();
    let Ok(text) = fs::read_to_string(format!("/proc/pressure/{res}")) else { return p };
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let kind = it.next();
        let avg10 = it
            .find_map(|kv| kv.strip_prefix("avg10="))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        match kind {
            Some("some") => p.some = avg10,
            Some("full") => p.full = avg10,
            _ => {}
        }
    }
    p
}

pub fn sample() -> PressureSnapshot {
    PressureSnapshot { cpu: read("cpu"), memory: read("memory"), io: read("io") }
}
