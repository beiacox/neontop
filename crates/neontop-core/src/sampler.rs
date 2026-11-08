use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::collectors::{
    cpu::{CpuCollector, CpuSnapshot},
    disks::{DiskCollector, DiskIoSnapshot},
    energy::{EnergyCollector, EnergySnapshot},
    games::{GamesCollector, GamesSnapshot},
    membw::{MemBw, MemBwSnapshot},
    sensors::{SensorCollector, SensorSnapshot},
    gpu::{GpuCollector, GpuSnapshot},
    memory::{self, MemSnapshot},
    network::{NetCollector, NetSnapshot},
    pressure::{self, PressureSnapshot},
    processes::{ProcCollector, ProcSnapshot},
    system,
};

/// Recogidas caras que solo se hacen mientras alguna vista las muestra.
#[derive(Default)]
pub struct Collect {
    /// Lista completa de procesos (/proc/*).
    pub procs: AtomicBool,
    /// Uso de GPU por proceso (fdinfo / NVML).
    pub gpu_procs: AtomicBool,
    /// Pide volver a buscar las zonas RAPL (tras cambiar sus permisos).
    pub rescan_energy: AtomicBool,
    /// Pide relanzar el lector de tráfico de RAM (tras cambiar sus permisos).
    pub restart_membw: AtomicBool,
    /// Quién pide cada recogida: (tipo, cliente) → último acceso. Los clientes web caducan si
    /// dejan de consultar (se cerró la pestaña del navegador); la ventana no caduca.
    wants: Mutex<HashMap<(String, String), (Instant, bool)>>,
}

const WEB_WANT_TTL: Duration = Duration::from_secs(30);

impl Collect {
    /// Registra (o retira) que `who` necesita la recogida `kind`.
    pub fn set(&self, kind: &str, who: &str, on: bool, expires: bool) -> Result<(), String> {
        if kind != "procs" && kind != "gpu_procs" {
            return Err(format!("recogida desconocida: {kind}"));
        }
        let mut w = self.wants.lock().unwrap();
        if on {
            w.insert((kind.to_string(), who.to_string()), (Instant::now(), expires));
        } else {
            w.remove(&(kind.to_string(), who.to_string()));
        }
        drop(w);
        self.refresh();
        Ok(())
    }

    /// Renueva las peticiones de un cliente web que sigue conectado.
    pub fn touch(&self, who: &str) {
        for ((_, w), (t, _)) in self.wants.lock().unwrap().iter_mut() {
            if w == who {
                *t = Instant::now();
            }
        }
    }

    /// Descarta peticiones caducadas y recalcula qué hay que recoger.
    pub fn refresh(&self) {
        let mut w = self.wants.lock().unwrap();
        w.retain(|_, (t, expires)| !*expires || t.elapsed() < WEB_WANT_TTL);
        let any = |k: &str| w.keys().any(|(kind, _)| kind == k);
        self.procs.store(any("procs"), Ordering::Relaxed);
        self.gpu_procs.store(any("gpu_procs"), Ordering::Relaxed);
    }
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// ms desde epoch
    pub ts: u64,
    pub uptime: f64,
    pub cpu: CpuSnapshot,
    pub mem: MemSnapshot,
    pub net: NetSnapshot,
    pub pressure: PressureSnapshot,
    pub gpus: Vec<GpuSnapshot>,
    pub disks: DiskIoSnapshot,
    pub sensors: SensorSnapshot,
    pub energy: EnergySnapshot,
    pub games: GamesSnapshot,
    /// Tráfico de la RAM en tiempo real (contadores del controlador de memoria).
    pub membw: MemBwSnapshot,
}

/// Mandos del hilo de muestreo, compartidos con la interfaz y el servidor web.
pub struct Control {
    /// Intervalo de muestreo en ms (se lee en cada vuelta: cambiarlo tiene efecto inmediato).
    pub interval_ms: AtomicU64,
    /// En pausa no se muestrea nada (coste ~0).
    pub paused: AtomicBool,
    pub collect: Collect,
}

impl Default for Control {
    fn default() -> Self {
        Control { interval_ms: AtomicU64::new(1000), paused: AtomicBool::new(false), collect: Collect::default() }
    }
}

impl Control {
    pub fn set_interval(&self, ms: u64) {
        self.interval_ms.store(ms.clamp(100, 60_000), Ordering::Relaxed);
    }
}

/// Quien recibe las muestras (la ventana, el servidor web, la bandeja…).
pub trait Sink: Send + 'static {
    fn metrics(&mut self, snap: &Arc<Snapshot>);
    /// Lista de procesos: solo llega si alguna vista la pidió (`Collect::set("procs", …)`).
    fn procs(&mut self, _list: &Arc<ProcSnapshot>) {}
}

/// Hilo de muestreo: recoge todas las métricas y se las pasa a `sink`.
///
/// Los procesos van aparte y solo se recogen si alguna vista los muestra, como mucho cada
/// 500 ms: recorrer /proc entero es lo más caro.
pub fn spawn(ctl: Arc<Control>, mut sink: impl Sink) -> thread::JoinHandle<()> {
    const PROC_MIN: Duration = Duration::from_millis(500);
    thread::Builder::new()
        .name("neontop-sampler".into())
        .spawn(move || {
            let mut cpu = CpuCollector::new();
            let mut net = NetCollector::new();
            let mut procs = ProcCollector::new();
            let mut gpu = GpuCollector::new();
            let mut disks = DiskCollector::new();
            let mut sensors = SensorCollector::new();
            let mut energy = EnergyCollector::new();
            let mut games = GamesCollector::new();
            let mut membw = MemBw::new();
            let ups = crate::collectors::ups::UpsCollector::new();
            let mut last_procs: Option<Instant> = None;
            thread::sleep(Duration::from_millis(200));

            loop {
                if ctl.paused.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
                let started = Instant::now();
                let collect = &ctl.collect;
                collect.refresh();
                if collect.rescan_energy.swap(false, Ordering::Relaxed) {
                    energy.rescan();
                }
                if collect.restart_membw.swap(false, Ordering::Relaxed) {
                    membw.restart();
                }
                let gpus = gpu.sample(collect.gpu_procs.load(Ordering::Relaxed));
                let sensor_snap = sensors.sample();
                // un hueco mayor que ~2 intervalos es una pausa o una suspensión: no se acumula energía
                let max_gap = ctl.interval_ms.load(Ordering::Relaxed) as f64 / 1000.0 * 2.0 + 1.0;
                let energy_snap = energy.sample(&gpus, &sensor_snap, ups.sample(), max_gap);
                let snap = Arc::new(Snapshot {
                    ts: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
                    uptime: system::uptime(),
                    cpu: cpu.sample(),
                    mem: memory::sample(),
                    net: net.sample(),
                    pressure: pressure::sample(),
                    gpus,
                    disks: disks.sample(),
                    sensors: sensor_snap,
                    energy: energy_snap,
                    games: games.sample(),
                    membw: membw.sample(),
                });
                sink.metrics(&snap);

                if collect.procs.load(Ordering::Relaxed) && last_procs.is_none_or(|t| t.elapsed() >= PROC_MIN) {
                    last_procs = Some(Instant::now());
                    sink.procs(&Arc::new(procs.sample()));
                }

                // dormir en trozos cortos para reaccionar rápido a cambios de intervalo o pausa
                while !ctl.paused.load(Ordering::Relaxed) {
                    let target = Duration::from_millis(ctl.interval_ms.load(Ordering::Relaxed));
                    let elapsed = started.elapsed();
                    if elapsed >= target {
                        break;
                    }
                    thread::sleep((target - elapsed).min(Duration::from_millis(50)));
                }
            }
        })
        .expect("hilo de muestreo")
}
