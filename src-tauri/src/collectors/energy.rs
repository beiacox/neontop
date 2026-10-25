//! Energía: todo lo que el sistema deja medir en vatios, con la energía acumulada de cada fuente.
//!
//! Fuentes:
//! - RAPL (/sys/class/powercap/intel-rapl:*): contadores de energía de CPU (paquete, núcleos,
//!   uncore/iGPU), DRAM y plataforma (psys). Existen en Intel y AMD Zen, pero desde el kernel
//!   5.10 solo root puede leerlos (CVE-2020-8694); `rapl_access` los abre con permiso del usuario.
//! - GPU: potencia que ya da cada backend (amdgpu, NVML, i915/xe).
//! - hwmon: cualquier otro sensor de potencia (zenpower, fuentes de alimentación, placa…).
//! - Baterías: descargando, su potencia es el consumo total del equipo.

use serde::Serialize;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Instant;

use super::gpu::GpuSnapshot;
use super::sensors::SensorSnapshot;
use super::{read_trim, read_u64};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EnergySource {
    /// Identificador estable ("rapl:intel-rapl:0", "gpu:0000:03:00.0", "bat:BAT0"…).
    pub id: String,
    pub label: String,
    /// "cpu" | "cpu-core" | "uncore" | "dram" | "platform" | "gpu" | "igpu" | "sensor" | "psu" | "battery" | "charge"
    pub kind: String,
    /// Potencia actual (W); None si aún no hay dos lecturas del contador.
    pub watts: Option<f64>,
    /// Energía acumulada desde que arrancó NeonTop (J).
    pub joules: f64,
    /// Límite de potencia configurado (W).
    pub limit: Option<f64>,
    /// "counter": contador de energía del hardware (exacto) | "sensor": potencia instantánea integrada.
    pub method: String,
    /// Cuenta para el total medido (las subzonas y los sensores redundantes no suman).
    pub in_total: bool,
    /// Mide el consumo del equipo entero (plataforma, fuente, batería descargando).
    pub system: bool,
    /// Fuente que la contiene (p. ej. los núcleos dentro del paquete).
    pub parent: Option<String>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnergySnapshot {
    pub sources: Vec<EnergySource>,
    /// Consumo total medido (W): fuente de sistema si la hay; si no, suma de las que cuentan.
    pub total_w: f64,
    /// Energía total acumulada desde el arranque (J).
    pub total_j: f64,
    /// Cómo se obtiene el total: "ups" | "platform" | "psu" | "battery" | "sum" | "none".
    pub total_method: String,
    /// Estado de RAPL: "ok" | "denied" (existe pero sin permiso) | "absent".
    pub rapl: String,
    /// Existe la regla udev que abre RAPL de forma permanente.
    pub rapl_rule: bool,
    /// SAI conectado (apcupsd), con su estado completo.
    pub ups: Option<super::ups::UpsSnapshot>,
}

pub const RAPL_RULE_PATH: &str = "/etc/udev/rules.d/60-neontop-rapl.rules";

struct RaplZone {
    dir: PathBuf,
    id: String,
    label: String,
    kind: &'static str,
    parent: Option<String>,
    max_range: u64,
    limit: Option<f64>,
    prev: Option<(u64, Instant)>,
    joules: f64,
    watts: Option<f64>,
}

fn scan_rapl() -> (Vec<RaplZone>, &'static str) {
    let Ok(rd) = fs::read_dir("/sys/class/powercap") else { return (Vec::new(), "absent") };
    let mut dirs: Vec<(String, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().into_string().ok()?;
            // intel-rapl-mmio duplica el paquete en Intel; intel-rapl (sin ':') es el tipo de control
            n.starts_with("intel-rapl:").then(|| (n, e.path()))
        })
        .collect();
    dirs.sort();
    let packages = dirs.iter().filter(|(_, d)| read_trim(d.join("name")).is_some_and(|n| n.starts_with("package"))).count();
    let mut status = if dirs.is_empty() { "absent" } else { "denied" };
    let zones = dirs
        .into_iter()
        .filter_map(|(n, dir)| {
            match fs::read_to_string(dir.join("energy_uj")) {
                Ok(_) => status = "ok",
                Err(e) if e.kind() == ErrorKind::PermissionDenied => return None,
                Err(_) => return None,
            }
            let name = read_trim(dir.join("name"))?;
            let pkg_no = |s: &str| s.rsplit('-').next().and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
            let (label, kind) = match name.as_str() {
                s if s.starts_with("package") => (if packages > 1 { format!("CPU paquete {}", pkg_no(s)) } else { "CPU (paquete)".into() }, "cpu"),
                "core" => ("Núcleos de CPU".into(), "cpu-core"),
                "uncore" => ("Uncore / iGPU".into(), "uncore"),
                "dram" => ("Memoria RAM".into(), "dram"),
                "psys" => ("Plataforma (SoC completo)".into(), "platform"),
                other => (other.to_string(), "cpu-core"),
            };
            // intel-rapl:0:1 cuelga de intel-rapl:0
            let parent = n.rsplit_once(':').filter(|(p, _)| p.contains(':')).map(|(p, _)| format!("rapl:{p}"));
            // límite a largo plazo (PL1), si el hardware lo expone
            let limit = (0..4).find_map(|i| {
                (read_trim(dir.join(format!("constraint_{i}_name")))? == "long_term")
                    .then(|| read_u64(dir.join(format!("constraint_{i}_power_limit_uw"))))
                    .flatten()
                    .map(|uw| uw as f64 / 1e6)
                    .filter(|w| *w > 0.0)
            });
            Some(RaplZone {
                max_range: read_u64(dir.join("max_energy_range_uj")).unwrap_or(u64::MAX),
                id: format!("rapl:{n}"),
                dir,
                label,
                kind,
                parent,
                limit,
                prev: None,
                joules: 0.0,
                watts: None,
            })
        })
        .collect();
    (zones, status)
}

/// Energía integrada de una fuente con potencia instantánea.
#[derive(Default)]
struct Integrator {
    joules: f64,
}

pub struct EnergyCollector {
    rapl: Vec<RaplZone>,
    rapl_status: &'static str,
    last_scan: Instant,
    last: Option<Instant>,
    integ: std::collections::HashMap<String, Integrator>,
    total_j: f64,
}

impl EnergyCollector {
    pub fn new() -> Self {
        let (rapl, rapl_status) = scan_rapl();
        EnergyCollector { rapl, rapl_status, last_scan: Instant::now(), last: None, integ: Default::default(), total_j: 0.0 }
    }

    /// Vuelve a buscar las zonas RAPL (p. ej. justo después de dar permiso).
    pub fn rescan(&mut self) {
        let old: std::collections::HashMap<String, f64> = self.rapl.iter().map(|z| (z.id.clone(), z.joules)).collect();
        let (mut rapl, status) = scan_rapl();
        for z in &mut rapl {
            z.joules = old.get(&z.id).copied().unwrap_or(0.0);
        }
        self.rapl = rapl;
        self.rapl_status = status;
        self.last_scan = Instant::now();
    }

    /// `max_gap_s`: por encima de este hueco entre muestras (pausa, suspensión) no se acumula
    /// energía, para no inventar consumo de un periodo que no se ha medido.
    pub fn sample(&mut self, gpus: &[GpuSnapshot], sensors: &SensorSnapshot, ups: Option<super::ups::UpsSnapshot>, max_gap_s: f64) -> EnergySnapshot {
        // sin permiso se reintenta cada 10 s, así el cambio se nota sin reiniciar
        if self.rapl_status != "ok" && self.last_scan.elapsed().as_secs() >= 10 {
            self.rescan();
        }
        let now = Instant::now();
        let dt = self.last.map(|t| now.duration_since(t).as_secs_f64());
        self.last = Some(now);
        let accumulate = dt.is_some_and(|d| d <= max_gap_s);

        let mut sources = Vec::new();
        let mut lost_access = false;

        for z in &mut self.rapl {
            let Some(e) = read_u64(z.dir.join("energy_uj")) else {
                lost_access = true;
                continue;
            };
            if let Some((pe, pt)) = z.prev {
                // el contador da la vuelta en max_energy_range_uj
                let delta = if e >= pe { e - pe } else { e + z.max_range.saturating_sub(pe) };
                let secs = now.duration_since(pt).as_secs_f64();
                if secs > 0.0 {
                    let j = delta as f64 / 1e6;
                    z.watts = Some(j / secs);
                    if accumulate {
                        z.joules += j;
                    }
                }
            }
            z.prev = Some((e, now));
            sources.push(EnergySource {
                id: z.id.clone(),
                label: z.label.clone(),
                kind: z.kind.into(),
                watts: z.watts,
                joules: z.joules,
                limit: z.limit,
                method: "counter".into(),
                // los núcleos y el uncore están dentro del paquete; la DRAM (Intel) va aparte
                in_total: matches!(z.kind, "cpu" | "dram"),
                system: z.kind == "platform",
                parent: z.parent.clone(),
            });
        }
        if lost_access {
            self.rescan();
        }
        let rapl_cpu = sources.iter().any(|s| s.kind == "cpu");

        let mut push_sensor = |sources: &mut Vec<EnergySource>, id: String, label: String, kind: &str, w: Option<f64>, limit: Option<f64>, in_total: bool, system: bool, parent: Option<String>| {
            let it = self.integ.entry(id.clone()).or_default();
            if let (Some(w), Some(d), true) = (w, dt, accumulate) {
                it.joules += w * d;
            }
            sources.push(EnergySource { id, label, kind: kind.into(), watts: w, joules: it.joules, limit, method: "sensor".into(), in_total, system, parent });
        };

        let pkg = sources.iter().find(|s| s.kind == "cpu").map(|s| s.id.clone());
        for g in gpus {
            if g.power.is_none() {
                continue;
            }
            // una iGPU consume dentro del paquete de la CPU: si RAPL ya lo mide, no se suma dos veces
            let kind = if g.integrated { "igpu" } else { "gpu" };
            let in_total = !(g.integrated && rapl_cpu);
            let parent = if g.integrated { pkg.clone() } else { None };
            push_sensor(&mut sources, format!("gpu:{}", g.pci_slot), g.name.clone(), kind, g.power.map(|w| w as f64), g.power_limit.map(|w| w as f64), in_total, false, parent);
        }

        for c in &sensors.chips {
            // la potencia de las GPU ya viene de su backend
            if c.category == "gpu" {
                continue;
            }
            for r in c.readings.iter().filter(|r| r.kind == "power") {
                let psu = c.driver.contains("psu");
                let (kind, in_total) = match c.category.as_str() {
                    // zenpower/SVI2: redundante con RAPL si está disponible
                    "cpu" => ("cpu", !rapl_cpu),
                    _ if psu => ("psu", false),
                    _ => ("sensor", false),
                };
                push_sensor(&mut sources, format!("hwmon:{}", r.id), format!("{} · {}", c.label, r.label), kind, Some(r.value), r.max, in_total, psu, None);
            }
        }

        for b in &sensors.batteries {
            let discharging = b.status == "Discharging";
            let (kind, label) = if discharging { ("battery", format!("Batería {} (descarga)", b.name)) } else { ("charge", format!("Batería {} (carga)", b.name)) };
            push_sensor(&mut sources, format!("bat:{}:{kind}", b.name), label, kind, b.power, None, false, discharging, None);
        }

        // el SAI mide todo lo que tiene enchufado (PC, monitores…): es el total del puesto
        if let Some(u) = &ups {
            let label = format!("SAI · {}", u.model);
            push_sensor(&mut sources, "ups:apcupsd".into(), label, "ups", u.power_w, u.nominal_w, false, true, None);
        }

        // Total: SAI > plataforma > fuente de alimentación > batería > suma de componentes
        let pick = |k: &str| sources.iter().find(|s| s.system && s.kind == k && s.watts.is_some());
        let (total_w, total_method) = if let Some(s) = pick("ups") {
            (s.watts.unwrap_or(0.0), "ups")
        } else if let Some(s) = pick("platform") {
            (s.watts.unwrap_or(0.0), "platform")
        } else if let Some(s) = pick("psu") {
            (s.watts.unwrap_or(0.0), "psu")
        } else if let Some(s) = pick("battery") {
            (s.watts.unwrap_or(0.0), "battery")
        } else {
            let parts: Vec<f64> = sources.iter().filter(|s| s.in_total).filter_map(|s| s.watts).collect();
            (parts.iter().sum(), if parts.is_empty() { "none" } else { "sum" })
        };
        if let (Some(d), true) = (dt, accumulate) {
            self.total_j += total_w * d;
        }

        EnergySnapshot {
            sources,
            total_w,
            total_j: self.total_j,
            total_method: total_method.into(),
            rapl: self.rapl_status.into(),
            rapl_rule: std::path::Path::new(RAPL_RULE_PATH).exists(),
            ups,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn samples_energy() {
        let mut gpu = super::super::gpu::GpuCollector::new();
        let mut sens = super::super::sensors::SensorCollector::new();
        let mut e = super::EnergyCollector::new();
        for _ in 0..3 {
            let s = e.sample(&gpu.sample(false), &sens.sample(), None, 5.0);
            eprintln!("RAPL {}  regla {}  total {:.1} W ({}) {:.2} J", s.rapl, s.rapl_rule, s.total_w, s.total_method, s.total_j);
            for x in &s.sources {
                eprintln!("  {:<28} {:<9} {:>7.2} W  {:>8.2} J  total={} sys={} parent={:?}", x.label, x.kind, x.watts.unwrap_or(-1.0), x.joules, x.in_total, x.system, x.parent);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}
