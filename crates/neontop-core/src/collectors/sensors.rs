//! Todos los sensores hwmon del sistema (temperaturas, ventiladores, voltajes, potencia)
//! más las baterías de /sys/class/power_supply.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use super::{read_trim, read_u64};

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    /// Identificador estable: "<chip>/<sensor>", p. ej. "k10temp/temp1".
    pub id: String,
    pub label: String,
    /// "temp" (°C) | "fan" (RPM) | "in" (V) | "power" (W) | "curr" (A) | "pwm" (%)
    pub kind: String,
    pub value: f64,
    pub max: Option<f64>,
    pub crit: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Chip {
    pub id: String,
    /// Nombre del driver (k10temp, amdgpu, nvme…).
    pub driver: String,
    /// Nombre legible ("CPU", "GPU", "NVMe CT500P3PSSD8"…).
    pub label: String,
    /// "cpu" | "gpu" | "disk" | "network" | "board" | "other"
    pub category: String,
    pub readings: Vec<Reading>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub name: String,
    pub capacity: f32,
    pub status: String,
    /// W, positivo descargando o cargando según `status`.
    pub power: Option<f64>,
    pub energy_now: Option<f64>,
    pub energy_full: Option<f64>,
    pub energy_design: Option<f64>,
    pub cycles: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SensorSnapshot {
    pub chips: Vec<Chip>,
    pub batteries: Vec<Battery>,
    pub ac_online: Option<bool>,
}

struct ChipStatic {
    dir: PathBuf,
    id: String,
    driver: String,
    label: String,
    category: String,
    sensors: Vec<SensorDef>,
}

/// (sensor, tipo, etiqueta, escala, max, crit)
type SensorDef = (String, String, String, f64, Option<f64>, Option<f64>);

pub struct SensorCollector {
    chips: Vec<ChipStatic>,
    last_scan: std::time::Instant,
}

fn classify(driver: &str, dev: &Path) -> (&'static str, String) {
    // según el kernel, el hwmon de un NVMe cuelga del controlador (…/nvme/nvme0) o del dispositivo PCI
    let model = |p: &Path| {
        read_trim(p.join("model"))
            .or_else(|| read_trim(p.join("device/model")))
            .or_else(|| fs::read_dir(p.join("nvme")).ok()?.flatten().find_map(|e| read_trim(e.path().join("model"))))
    };
    match driver {
        "k10temp" | "zenpower" | "coretemp" | "cpu_thermal" => ("cpu", "CPU".into()),
        "amdgpu" | "radeon" | "nouveau" | "i915" | "xe" => ("gpu", "GPU".into()),
        "nvme" => ("disk", model(dev).map(|m| format!("NVMe {m}")).unwrap_or_else(|| "NVMe".into())),
        "drivetemp" => ("disk", model(dev).map(|m| format!("Disco {m}")).unwrap_or_else(|| "Disco".into())),
        d if d.starts_with("mt79") || d.starts_with("iwlwifi") || d.contains("phy") => ("network", "Wi-Fi".into()),
        d if d.starts_with("nct") || d.starts_with("it8") || d.starts_with("asus") || d.contains("ec") => ("board", "Placa base".into()),
        "acpitz" => ("board", "ACPI".into()),
        _ => ("other", driver.into()),
    }
}

fn scan() -> Vec<ChipStatic> {
    let Ok(rd) = fs::read_dir("/sys/class/hwmon") else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    dirs.sort();
    let mut seen: std::collections::HashMap<String, u32> = Default::default();
    dirs.into_iter()
        .filter_map(|dir| {
            let driver = read_trim(dir.join("name"))?;
            let dev = fs::canonicalize(dir.join("device")).unwrap_or_default();
            let (category, mut label) = classify(&driver, &dev);
            // varios chips iguales (dos NVMe del mismo modelo): se numeran
            let n = seen.entry(label.clone()).or_default();
            *n += 1;
            if *n > 1 {
                label = format!("{label} #{n}");
            }
            let hw = dir.file_name()?.to_string_lossy().into_owned();
            let mut files: Vec<String> = fs::read_dir(&dir).ok()?.flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
            files.sort_by_key(|f| {
                // orden natural: temp2 antes que temp10
                let digits: String = f.chars().filter(|c| c.is_ascii_digit()).collect();
                (f.chars().take_while(|c| c.is_alphabetic()).collect::<String>(), digits.parse::<u32>().unwrap_or(0))
            });
            let sensors = files
                .iter()
                .filter_map(|f| {
                    let (sensor, kind, scale) = if let Some(s) = f.strip_suffix("_input") {
                        let kind = s.trim_end_matches(|c: char| c.is_ascii_digit());
                        let scale = match kind {
                            "temp" => 1000.0,
                            "in" | "curr" => 1000.0,
                            "power" => 1e6,
                            "fan" => 1.0,
                            _ => return None,
                        };
                        (s.to_string(), kind.to_string(), scale)
                    } else if let Some(s) = f.strip_suffix("_average").filter(|s| s.starts_with("power")) {
                        // amdgpu publica power1_average en lugar de _input
                        if files.iter().any(|x| x == &format!("{s}_input")) {
                            return None;
                        }
                        (format!("{s}_average"), "power".to_string(), 1e6)
                    } else if f.starts_with("pwm") && f[3..].chars().all(|c| c.is_ascii_digit()) && f.len() > 3 {
                        (f.clone(), "pwm".to_string(), 255.0 / 100.0)
                    } else {
                        return None;
                    };
                    let base = sensor.trim_end_matches("_average").to_string();
                    let label = read_trim(dir.join(format!("{base}_label"))).unwrap_or_else(|| base.to_string());
                    let lim = |suffix: &str| {
                        read_trim(dir.join(format!("{base}_{suffix}")))
                            .and_then(|v| v.parse::<f64>().ok())
                            .map(|v| v / scale)
                            // algunos NVMe informan 65261 °C como "sin límite"
                            .filter(|v| kind != "temp" || (*v > 0.0 && *v < 200.0))
                    };
                    let (max, crit) = (lim("max"), lim("crit"));
                    Some((sensor, kind, label, scale, max, crit))
                })
                .collect::<Vec<_>>();
            (!sensors.is_empty()).then(|| ChipStatic { id: format!("{hw}:{driver}"), dir, driver, label, category: category.into(), sensors })
        })
        .collect()
}

fn batteries() -> (Vec<Battery>, Option<bool>) {
    let Ok(rd) = fs::read_dir("/sys/class/power_supply") else { return (Vec::new(), None) };
    let mut bats = Vec::new();
    let mut ac = None;
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        match read_trim(p.join("type")).as_deref() {
            Some("Mains") => ac = read_u64(p.join("online")).map(|v| v == 1),
            Some("Battery") if read_trim(p.join("scope")).as_deref() != Some("Device") => {
                // energy_* en µWh; algunas baterías solo dan charge_* (µAh) y voltaje
                let volt = read_u64(p.join("voltage_now")).map(|v| v as f64 / 1e6);
                let energy = |k: &str| {
                    read_u64(p.join(format!("energy_{k}")))
                        .map(|v| v as f64 / 1e6)
                        .or_else(|| Some(read_u64(p.join(format!("charge_{k}")))? as f64 / 1e6 * volt?))
                };
                let power = read_u64(p.join("power_now"))
                    .map(|v| v as f64 / 1e6)
                    .or_else(|| Some(read_u64(p.join("current_now"))? as f64 / 1e6 * volt?));
                bats.push(Battery {
                    capacity: read_u64(p.join("capacity")).unwrap_or(0) as f32,
                    status: read_trim(p.join("status")).unwrap_or_default(),
                    power,
                    energy_now: energy("now"),
                    energy_full: energy("full"),
                    energy_design: energy("full_design"),
                    cycles: read_u64(p.join("cycle_count")).map(|v| v as u32).filter(|v| *v > 0),
                    name,
                });
            }
            _ => {}
        }
    }
    (bats, ac)
}

impl Default for SensorCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl SensorCollector {
    pub fn new() -> Self {
        SensorCollector { chips: scan(), last_scan: std::time::Instant::now() }
    }

    pub fn sample(&mut self) -> SensorSnapshot {
        // se reescanea de vez en cuando por si se carga un módulo (nct6775, drivetemp…)
        if self.last_scan.elapsed().as_secs() >= 15 {
            self.chips = scan();
            self.last_scan = std::time::Instant::now();
        }
        let chips = self
            .chips
            .iter()
            .map(|c| Chip {
                id: c.id.clone(),
                driver: c.driver.clone(),
                label: c.label.clone(),
                category: c.category.clone(),
                readings: c
                    .sensors
                    .iter()
                    .filter_map(|(sensor, kind, label, scale, max, crit)| {
                        let raw = read_trim(c.dir.join(if kind == "pwm" || sensor.ends_with("_average") { sensor.clone() } else { format!("{sensor}_input") }))?;
                        Some(Reading {
                            id: format!("{}/{}", c.id, sensor),
                            label: label.clone(),
                            kind: kind.clone(),
                            value: raw.parse::<f64>().ok()? / scale,
                            max: *max,
                            crit: *crit,
                        })
                    })
                    .collect(),
            })
            .collect();
        let (batteries, ac_online) = batteries();
        SensorSnapshot { chips, batteries, ac_online }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn samples_sensors() {
        let s = super::SensorCollector::new().sample();
        for c in &s.chips {
            eprintln!("[{}] {} ({})", c.category, c.label, c.driver);
            for r in &c.readings {
                eprintln!("    {:<12} {:<6} {:>9.2}  max {:?} crit {:?}", r.label, r.kind, r.value, r.max, r.crit);
            }
        }
        eprintln!("baterías: {}  AC: {:?}", s.batteries.len(), s.ac_online);
    }
}
