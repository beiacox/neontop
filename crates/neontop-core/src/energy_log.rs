//! Histórico de energía: lo gastado cada día (total, por fuente y por hora), pico y mínimo de la
//! sesión, y estimación del consumo de cada proceso. Se acumula siempre que NeonTop esté midiendo,
//! aunque la pestaña Energía no esté abierta, y se guarda en ~/.local/share/com.neontop.app/energy.json.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use crate::collectors::energy::EnergySource;
use crate::collectors::gpu::GpuSnapshot;
use crate::collectors::processes::ProcInfo;
use crate::format;
use crate::sampler::Snapshot;
use crate::theme::Slot;

/// Días de histórico que se conservan.
const KEEP_DAYS: usize = 400;
/// Cada cuánto se guarda el histórico (ms).
const SAVE_MS: u64 = 30_000;

/// Id del «resto del puesto» (SAI − componentes medidos) en el histórico diario.
pub const REST_ID: &str = "ups:rest";
pub const REST_LABEL: &str = "Resto del puesto";

/// Energía de un día (Wh), en total, por fuente y por hora local.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct DayEnergy {
    pub wh: f64,
    pub src: BTreeMap<String, f64>,
    pub hours: Vec<f64>,
}

impl Default for DayEnergy {
    fn default() -> Self {
        DayEnergy { wh: 0.0, src: BTreeMap::new(), hours: vec![0.0; 24] }
    }
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
#[serde(default)]
struct Saved {
    days: BTreeMap<String, DayEnergy>,
    labels: HashMap<String, String>,
}

/// Nombre corto de cada tipo de fuente.
pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "cpu" => "CPU",
        "cpu-core" => "Núcleos",
        "uncore" => "Uncore",
        "dram" => "RAM",
        "platform" => "Plataforma",
        "gpu" => "GPU",
        "igpu" => "iGPU",
        "sensor" => "Sensor",
        "psu" => "Fuente",
        "battery" => "Batería",
        "charge" => "Carga",
        "ups" => "SAI",
        _ => "Otro",
    }
}

pub fn kind_color(kind: &str) -> Slot {
    match kind {
        "cpu" => Slot::Cyan,
        "cpu-core" => Slot::Teal,
        "uncore" | "igpu" => Slot::Violet,
        "dram" | "battery" | "charge" => Slot::Lime,
        "platform" | "psu" | "ups" => Slot::Amber,
        "gpu" => Slot::Magenta,
        "sensor" => Slot::Orange,
        _ => Slot::Muted,
    }
}

/// Potencia estimada de un proceso (o grupo de procesos con el mismo nombre).
#[derive(Clone, Debug, PartialEq)]
pub struct ProcPower {
    pub name: String,
    pub count: u32,
    pub cpu: f32,
    pub cpu_w: Option<f64>,
    pub gpu_w: Option<f64>,
    pub total_w: f64,
}

/// Potencia de la CPU (paquetes RAPL o sensores de la CPU), si se conoce.
pub fn cpu_watts(s: &Snapshot) -> Option<f64> {
    let pkg: Vec<f64> = s.energy.sources.iter().filter(|x| x.kind == "cpu").filter_map(|x| x.watts).collect();
    (!pkg.is_empty()).then(|| pkg.iter().sum())
}

/// Reparte la potencia medida entre procesos: la de la CPU según el tiempo de CPU de cada uno,
/// la de cada GPU según el uso de sus motores. Es una estimación (incluye el consumo en reposo
/// repartido proporcionalmente), pero ordena bien quién está gastando.
pub fn estimate_procs(s: &Snapshot, list: &[ProcInfo], gpus: &[GpuSnapshot]) -> Vec<ProcPower> {
    let cpu_w = cpu_watts(s);
    let cpu_sum: f32 = list.iter().map(|p| p.cpu).sum();
    let mut rows: Vec<ProcPower> = Vec::new();
    let mut idx: HashMap<String, usize> = HashMap::new();
    let mut row = |rows: &mut Vec<ProcPower>, name: &str| -> usize {
        *idx.entry(name.to_string()).or_insert_with(|| {
            rows.push(ProcPower { name: name.to_string(), count: 0, cpu: 0.0, cpu_w: cpu_w.map(|_| 0.0), gpu_w: None, total_w: 0.0 });
            rows.len() - 1
        })
    };
    for p in list.iter().filter(|p| p.cpu > 0.0) {
        let i = row(&mut rows, &p.name);
        let r = &mut rows[i];
        r.count += 1;
        r.cpu += p.cpu;
        if let (Some(w), true) = (cpu_w, cpu_sum > 0.0) {
            *r.cpu_w.get_or_insert(0.0) += w * (p.cpu / cpu_sum) as f64;
        }
    }
    let busy = |e: &[crate::collectors::gpu::GpuEngine]| e.iter().map(|x| x.busy).sum::<f32>();
    for g in gpus {
        let (Some(power), true) = (g.power, g.processes_sampled) else { continue };
        let busy_sum: f32 = g.processes.iter().map(|p| busy(&p.engines)).sum();
        if busy_sum <= 0.0 {
            continue;
        }
        for p in &g.processes {
            let b = busy(&p.engines);
            if b <= 0.0 {
                continue;
            }
            let i = row(&mut rows, &p.name);
            let r = &mut rows[i];
            r.count = r.count.max(1);
            *r.gpu_w.get_or_insert(0.0) += (power * b / busy_sum) as f64;
        }
    }
    for r in &mut rows {
        r.total_w = r.cpu_w.unwrap_or(0.0) + r.gpu_w.unwrap_or(0.0);
    }
    rows.sort_by(|a, b| b.total_w.total_cmp(&a.total_w).then(b.cpu.total_cmp(&a.cpu)));
    rows
}

/// Fuentes de primer nivel (sin padre): primero las que suman al total, luego por potencia.
pub fn top_sources(list: &[EnergySource]) -> Vec<&EnergySource> {
    let mut v: Vec<&EnergySource> = list.iter().filter(|s| s.parent.is_none()).collect();
    v.sort_by(|a, b| b.in_total.cmp(&a.in_total).then(b.watts.unwrap_or(0.0).total_cmp(&a.watts.unwrap_or(0.0))));
    v
}

pub struct EnergyLog {
    path: Option<PathBuf>,
    pub days: BTreeMap<String, DayEnergy>,
    /// Nombre legible de cada fuente, para los días en que ya no esté presente.
    pub labels: HashMap<String, String>,
    /// Inicio de la sesión (ms).
    pub start_ts: u64,
    pub peak_w: f64,
    pub min_w: f64,
    /// Energía estimada acumulada por proceso (J), mientras se mide la lista de procesos.
    pub proc_j: HashMap<String, f64>,
    proc_last: Option<u64>,
    prev_total: Option<f64>,
    prev_src: HashMap<String, f64>,
    last_save: u64,
}

impl EnergyLog {
    /// `path = None`: solo en memoria (pruebas).
    pub fn load(path: Option<PathBuf>) -> Self {
        let saved: Saved = path.as_deref().map(crate::paths::read_json).unwrap_or_default();
        EnergyLog {
            path,
            days: saved.days,
            labels: saved.labels,
            start_ts: 0,
            peak_w: 0.0,
            min_w: f64::INFINITY,
            proc_j: HashMap::new(),
            proc_last: None,
            prev_total: None,
            prev_src: HashMap::new(),
            last_save: 0,
        }
    }

    /// Acumula en el día y la hora actuales la energía gastada desde la muestra anterior.
    pub fn ingest(&mut self, s: &Snapshot) {
        let e = &s.energy;
        if self.start_ts == 0 {
            self.start_ts = s.ts;
            self.last_save = s.ts;
        }
        if e.total_method != "none" {
            self.peak_w = self.peak_w.max(e.total_w);
            if e.total_w > 0.0 {
                self.min_w = self.min_w.min(e.total_w);
            }
        }
        let key = format::day_key(s.ts);
        let hour = format::local_hour(s.ts);
        let day = self.days.entry(key).or_default();
        day.hours.resize(24, 0.0);
        if let Some(prev) = self.prev_total {
            let d = e.total_j - prev;
            if d > 0.0 {
                day.wh += d / 3600.0;
                day.hours[hour] += d / 3600.0;
            }
        }
        self.prev_total = Some(e.total_j);
        let mut parts_wh = 0.0;
        let mut ups_wh: Option<f64> = None;
        for x in &e.sources {
            self.labels.insert(x.id.clone(), x.label.clone());
            let prev = self.prev_src.get(&x.id).copied();
            let d = match prev {
                Some(p) if x.joules > p => (x.joules - p) / 3600.0,
                _ => 0.0,
            };
            // por día solo se guardan las fuentes que suman al total (sus barras se apilan)
            if x.in_total && d > 0.0 {
                *day.src.entry(x.id.clone()).or_default() += d;
                parts_wh += d;
            }
            if x.kind == "ups" && prev.is_some() {
                ups_wh = Some(d);
            }
            self.prev_src.insert(x.id.clone(), x.joules);
        }
        // con SAI, lo que mide de más que los componentes es el resto del puesto (monitores, placa…)
        if let Some(u) = ups_wh.filter(|&u| u > parts_wh) {
            *day.src.entry(REST_ID.into()).or_default() += u - parts_wh;
            self.labels.insert(REST_ID.into(), REST_LABEL.into());
        }
        if s.ts.saturating_sub(self.last_save) > SAVE_MS {
            self.last_save = s.ts;
            self.save();
        }
    }

    /// Suma la energía estimada de cada proceso desde la última lista (`now` en ms).
    pub fn accumulate_procs(&mut self, rows: &[ProcPower], now: u64) {
        let dt = self.proc_last.map(|t| now.saturating_sub(t) as f64 / 1000.0).unwrap_or(0.0);
        self.proc_last = Some(now);
        // un hueco largo (pestaña cerrada, pausa) no se cuenta
        if dt <= 0.0 || dt > 5.0 {
            return;
        }
        for r in rows.iter().filter(|r| r.total_w > 0.0) {
            *self.proc_j.entry(r.name.clone()).or_default() += r.total_w * dt;
        }
    }

    /// Energía (Wh) del mes del instante `ms`.
    pub fn month_wh(&self, ms: u64) -> f64 {
        let prefix = &format::day_key(ms)[..7];
        self.days.iter().filter(|(k, _)| k.starts_with(prefix)).map(|(_, v)| v.wh).sum()
    }

    pub fn today(&self, ms: u64) -> Option<&DayEnergy> {
        self.days.get(&format::day_key(ms))
    }

    pub fn clear(&mut self) {
        self.days.clear();
        self.proc_j.clear();
        self.save();
    }

    pub fn save(&mut self) {
        while self.days.len() > KEEP_DAYS {
            let first = self.days.keys().next().cloned().unwrap();
            self.days.remove(&first);
        }
        if let Some(p) = &self.path {
            let saved = Saved { days: self.days.clone(), labels: self.labels.clone() };
            if let Err(e) = crate::paths::write_json(p, &saved) {
                eprintln!("NeonTop: no se pudo guardar el histórico de energía: {e}");
            }
        }
    }
}

/// Coste de una energía (Wh) a `price` por kWh.
pub fn cost(wh: f64, price: f64) -> f64 {
    wh / 1000.0 * price
}

pub fn fmt_cost(wh: f64, price: f64, currency: &str) -> String {
    let c = cost(wh, price);
    if c < 1.0 { format!("{currency}{c:.3}") } else { format!("{currency}{c:.2}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collectors::energy::EnergySnapshot;

    fn src(id: &str, kind: &str, j: f64, w: f64, in_total: bool) -> EnergySource {
        EnergySource { id: id.into(), label: id.to_uppercase(), kind: kind.into(), watts: Some(w), joules: j, in_total, ..Default::default() }
    }

    fn snap(ts: u64, total_j: f64, sources: Vec<EnergySource>) -> Snapshot {
        Snapshot { ts, energy: EnergySnapshot { sources, total_w: 100.0, total_j, total_method: "rapl".into(), ..Default::default() }, ..Default::default() }
    }

    #[test]
    fn accumulates_per_day_hour_and_source() {
        let mut log = EnergyLog::load(None);
        let t0 = 1_700_000_000_000;
        log.ingest(&snap(t0, 0.0, vec![src("cpu", "cpu", 0.0, 50.0, true), src("ups", "ups", 0.0, 200.0, false)]));
        log.ingest(&snap(t0 + 1000, 3600.0, vec![src("cpu", "cpu", 1800.0, 50.0, true), src("ups", "ups", 7200.0, 200.0, false)]));
        let day = log.today(t0).unwrap();
        assert!((day.wh - 1.0).abs() < 1e-9);
        assert!((day.hours.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!((day.src["cpu"] - 0.5).abs() < 1e-9);
        // SAI 2 Wh − componentes 0,5 Wh = 1,5 Wh de resto del puesto
        assert!((day.src[REST_ID] - 1.5).abs() < 1e-9);
        assert_eq!(log.labels[REST_ID], REST_LABEL);
        assert_eq!(log.peak_w, 100.0);
        assert!((log.month_wh(t0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn counter_reset_is_ignored() {
        let mut log = EnergyLog::load(None);
        let t0 = 1_700_000_000_000;
        log.ingest(&snap(t0, 5000.0, vec![src("cpu", "cpu", 5000.0, 1.0, true)]));
        log.ingest(&snap(t0 + 1000, 10.0, vec![src("cpu", "cpu", 10.0, 1.0, true)]));
        assert_eq!(log.today(t0).unwrap().wh, 0.0);
    }

    #[test]
    fn persists_and_trims() {
        let p = std::env::temp_dir().join(format!("neontop-energy-{}.json", std::process::id()));
        let mut log = EnergyLog::load(Some(p.clone()));
        for d in 0..(KEEP_DAYS + 5) {
            log.days.insert(format!("2020-{:03}", d), DayEnergy { wh: 1.0, ..Default::default() });
        }
        log.save();
        let back = EnergyLog::load(Some(p.clone()));
        assert_eq!(back.days.len(), KEEP_DAYS);
        assert!(!back.days.contains_key("2020-000"), "se borran los más antiguos");
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn process_estimate() {
        let mut s = snap(0, 0.0, vec![src("pkg", "cpu", 0.0, 60.0, true)]);
        let mut g = GpuSnapshot { power: Some(100.0), processes_sampled: true, ..Default::default() };
        g.processes = vec![
            crate::collectors::gpu::GpuProc { pid: 1, name: "juego".into(), engines: vec![crate::collectors::gpu::GpuEngine { name: "gfx".into(), busy: 75.0 }], ..Default::default() },
            crate::collectors::gpu::GpuProc { pid: 2, name: "kwin".into(), engines: vec![crate::collectors::gpu::GpuEngine { name: "gfx".into(), busy: 25.0 }], ..Default::default() },
        ];
        s.gpus = vec![g];
        let procs = vec![
            ProcInfo { pid: 1, name: "juego".into(), cpu: 30.0, ..Default::default() },
            ProcInfo { pid: 3, name: "firefox".into(), cpu: 10.0, ..Default::default() },
        ];
        let rows = estimate_procs(&s, &procs, &s.gpus);
        assert_eq!(rows[0].name, "juego");
        assert!((rows[0].cpu_w.unwrap() - 45.0).abs() < 1e-6);
        assert!((rows[0].gpu_w.unwrap() - 75.0).abs() < 1e-6);
        assert!((rows[0].total_w - 120.0).abs() < 1e-6);
        assert!(rows.iter().any(|r| r.name == "kwin" && r.cpu_w == Some(0.0)));
        let mut log = EnergyLog::load(None);
        log.accumulate_procs(&rows, 1000);
        log.accumulate_procs(&rows, 3000);
        assert!((log.proc_j["juego"] - 240.0).abs() < 1e-6);
        log.accumulate_procs(&rows, 60_000); // hueco largo: no cuenta
        assert!((log.proc_j["juego"] - 240.0).abs() < 1e-6);
    }

    #[test]
    fn costs_and_kinds() {
        assert_eq!(fmt_cost(1000.0, 0.15, "$"), "$0.150");
        assert_eq!(fmt_cost(10_000.0, 0.15, "€"), "€1.50");
        assert_eq!(kind_label("dram"), "RAM");
        assert_eq!(kind_color("gpu"), Slot::Magenta);
        let list = vec![src("a", "cpu", 0.0, 10.0, false), src("b", "gpu", 0.0, 5.0, true)];
        assert_eq!(top_sources(&list)[0].id, "b");
    }
}
