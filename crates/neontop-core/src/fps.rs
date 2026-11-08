//! Estadísticas de juegos medidos con la capa NeonTop FPS: FPS medio, 1 % y 0,1 % bajo,
//! tirones, estabilidad, línea de tiempo de la partida y grabación de benchmarks.
//!
//! Se procesan todas las muestras (también con la pestaña cerrada o la ventana oculta) para que
//! las estadísticas de la sesión sean completas. Al cerrarse el juego la partida se guarda en el
//! historial (`sessions::Sessions`) con el mismo formato que la versión web.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::collectors::games::GameSnapshot;
use crate::collectors::gpu::GpuSnapshot;
use crate::history::History;
use crate::sampler::Snapshot;
use crate::sessions::Sessions;
use crate::theme::Slot;

/// Resolución del histograma de tiempos de fotograma (ms) y tope (lo que pase va al último cubo).
const BIN_MS: f32 = 0.05;
const MAX_MS: f32 = 250.0;
const NBINS: usize = (MAX_MS / BIN_MS) as usize + 1;
/// Fotogramas recientes que se guardan por juego (gráfica por fotograma y mediana móvil).
const RECENT: usize = 6000;
/// Ventana del 1 % bajo "en vivo" (s).
pub const LIVE_WINDOW: f64 = 5.0;

/// Estadísticas acumuladas de tiempos de fotograma. Los percentiles salen de un histograma de
/// 0,05 ms: una sesión de horas ocupa lo mismo que una de segundos y el 1 % / 0,1 % bajo son
/// exactos a 0,05 ms.
#[derive(Clone)]
pub struct FrameStats {
    bins: Vec<u32>,
    pub count: u64,
    /// Suma de tiempos (ms) = duración cubierta.
    pub sum: f64,
    pub min: f32,
    pub max: f32,
    /// Tirones: fotogramas de más del doble de la mediana reciente (y ≥ 4 ms más).
    pub stutters: u32,
    /// Parones: fotogramas de más de 50 ms y 3 veces la mediana.
    pub hitches: u32,
    /// Suma de |ft[i] − ft[i−1]|: irregularidad del ritmo de fotogramas.
    jitter: f64,
    last: f32,
}

impl Default for FrameStats {
    fn default() -> Self {
        FrameStats { bins: vec![0; NBINS], count: 0, sum: 0.0, min: f32::INFINITY, max: 0.0, stutters: 0, hitches: 0, jitter: 0.0, last: 0.0 }
    }
}

impl FrameStats {
    pub fn add(&mut self, ft: f32, median: f32) {
        let ft = ft.max(0.0);
        self.bins[((ft / BIN_MS) as usize).min(NBINS - 1)] += 1;
        self.count += 1;
        self.sum += ft as f64;
        self.min = self.min.min(ft);
        self.max = self.max.max(ft);
        if median > 0.0 {
            if ft > median * 2.0 && ft - median >= 4.0 {
                self.stutters += 1;
            }
            if ft > 50.0 && ft > median * 3.0 {
                self.hitches += 1;
            }
        }
        if self.last > 0.0 {
            self.jitter += (ft - self.last).abs() as f64;
        }
        self.last = ft;
    }

    /// Tiempo de fotograma (ms) del percentil `p` (0–100): p = 99 → base del 1 % bajo.
    pub fn percentile(&self, p: f64) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let target = ((p / 100.0) * self.count as f64).ceil().max(1.0) as u64;
        let mut acc = 0u64;
        for (i, &b) in self.bins.iter().enumerate() {
            acc += b as u64;
            if acc >= target {
                return (i as f32 + 0.5) * BIN_MS;
            }
        }
        MAX_MS
    }

    pub fn avg_fps(&self) -> f32 {
        if self.sum > 0.0 { (self.count as f64 * 1000.0 / self.sum) as f32 } else { 0.0 }
    }

    /// 1 % bajo: FPS equivalente al percentil 99 del tiempo de fotograma (como MangoHud / CapFrameX).
    pub fn low1(&self) -> f32 {
        let p = self.percentile(99.0);
        if p > 0.0 { 1000.0 / p } else { 0.0 }
    }

    pub fn low01(&self) -> f32 {
        let p = self.percentile(99.9);
        if p > 0.0 { 1000.0 / p } else { 0.0 }
    }

    /// Variación media entre fotogramas consecutivos (ms).
    pub fn jitter_ms(&self) -> f32 {
        if self.count > 1 { (self.jitter / (self.count - 1) as f64) as f32 } else { 0.0 }
    }

    pub fn seconds(&self) -> f64 {
        self.sum / 1000.0
    }

    /// Estabilidad 0–100: 70 % constancia (1 % bajo frente a la media) + 30 % suavidad del ritmo
    /// (variación entre fotogramas relativa al tiempo medio), menos una penalización por parones.
    pub fn stability(&self) -> f32 {
        if self.count < 30 {
            return 0.0;
        }
        let avg_ft = (self.sum / self.count as f64) as f32;
        let consistency = (self.low1() / self.avg_fps()).min(1.0);
        let smooth = (1.0 - self.jitter_ms() / avg_ft).max(0.0);
        let hitch_penalty = ((self.hitches as f64 / (self.seconds() / 60.0).max(1.0)) * 0.03).min(0.3) as f32;
        ((consistency * 0.7 + smooth * 0.3 - hitch_penalty) * 100.0).clamp(0.0, 100.0)
    }

    /// Histograma agrupado en cubos de `step` ms hasta `up_to` ms (para dibujar).
    pub fn grouped(&self, step: f32, up_to: f32) -> Vec<u32> {
        let per = (step / BIN_MS).round().max(1.0) as usize;
        let n = (up_to / step).ceil() as usize;
        let mut out = vec![0u32; n];
        for (i, &b) in self.bins.iter().enumerate() {
            out[(i / per).min(n - 1)] += b;
        }
        out
    }
}

/// Calificación de una puntuación de estabilidad (texto y color).
pub fn stability_label(score: f32, frames: u64) -> (&'static str, Slot) {
    if frames < 30 {
        ("sin datos", Slot::Muted)
    } else if score >= 90.0 {
        ("Excelente", Slot::Lime)
    } else if score >= 75.0 {
        ("Buena", Slot::Cyan)
    } else if score >= 55.0 {
        ("Irregular", Slot::Amber)
    } else {
        ("Mala", Slot::Red)
    }
}

/// Recursos del sistema durante una grabación o partida (sumas para la media y máximos).
#[derive(Clone, Copy, Default, Debug)]
pub struct ResAcc {
    pub n: u32,
    pub cpu: f64,
    pub cpu_max_core: f64,
    pub gpu: f64,
    pub gpu_power: f64,
    pub cpu_temp_max: f32,
    pub gpu_temp_max: f32,
}

/// GPU principal: la primera dedicada (o la única).
pub fn main_gpu(gpus: &[GpuSnapshot]) -> Option<&GpuSnapshot> {
    gpus.iter().find(|g| !g.integrated).or(gpus.first())
}

impl ResAcc {
    fn sample(&mut self, s: &Snapshot) {
        let g = main_gpu(&s.gpus);
        self.n += 1;
        self.cpu += s.cpu.total.usage as f64;
        self.cpu_max_core += s.cpu.cores.iter().map(|c| c.usage).fold(0.0, f32::max) as f64;
        self.gpu += g.and_then(|g| g.busy).unwrap_or(0.0) as f64;
        self.gpu_power += g.and_then(|g| g.power).unwrap_or(0.0) as f64;
        self.cpu_temp_max = self.cpu_temp_max.max(s.cpu.temp.unwrap_or(0.0));
        self.gpu_temp_max = self.gpu_temp_max.max(g.and_then(|g| g.temp_hotspot.or(g.temp_edge)).unwrap_or(0.0));
    }
    fn mean(sum: f64, n: u32) -> f32 {
        (sum / n.max(1) as f64) as f32
    }
}

/// Resumen de una partida o benchmark en el historial (index.json). Mismo formato que la
/// versión web, para conservar el historial al actualizar.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Benchmark {
    pub id: String,
    /// "bench": grabación manual; "session": partida completa, guardada al cerrar el juego.
    pub kind: Option<String>,
    pub title: String,
    pub exe: String,
    pub api: String,
    /// ms desde epoch
    pub date: u64,
    pub seconds: f64,
    pub frames: u64,
    pub avg: f32,
    pub low1: f32,
    pub low01: f32,
    pub min_fps: f32,
    pub max_fps: f32,
    pub stutters: u32,
    pub hitches: u32,
    pub stability: f32,
    pub jitter_ms: f32,
    pub cpu: f32,
    pub gpu: f32,
    pub gpu_power: f32,
    pub cpu_temp_max: f32,
    pub gpu_temp_max: f32,
}

impl Benchmark {
    pub fn is_bench(&self) -> bool {
        self.kind.as_deref() == Some("bench")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Histogram {
    pub step: f32,
    pub counts: Vec<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Percentiles {
    pub p50: f32,
    pub p90: f32,
    pub p95: f32,
    pub p99: f32,
    pub p999: f32,
    pub max: f32,
    pub median: f32,
}

/// Series de una partida guardada, para volver a dibujarla.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionDetail {
    pub v: u32,
    /// Inicio (ms desde epoch); `t` son segundos desde ahí.
    pub t0: f64,
    pub t: Vec<f64>,
    pub fps: Vec<f32>,
    pub low1: Vec<f32>,
    /// Peor tiempo de fotograma de cada punto (ms): los tirones se ven como picos.
    pub ft_max: Vec<f32>,
    pub cpu: Vec<f32>,
    pub gpu: Vec<f32>,
    pub gpu_power: Vec<f32>,
    pub gpu_temp: Vec<f32>,
    pub cpu_temp: Vec<f32>,
    pub hist: Histogram,
    pub pct: Percentiles,
    /// Cada fotograma (solo benchmarks), para exportar a CSV.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frametimes: Option<Vec<f32>>,
}

/// Valores de un punto de la línea de tiempo.
#[derive(Clone, Copy, Default, Debug)]
pub struct TlPoint {
    pub fps: f32,
    pub low1: f32,
    pub ft_max: f32,
    pub cpu: f32,
    pub gpu: f32,
    pub gpu_power: f32,
    pub gpu_temp: f32,
    pub cpu_temp: f32,
}

/// Series en las que al agrupar se conserva el máximo (tirones y temperaturas), por posición.
const TL_MAX: [bool; 8] = [false, false, true, false, false, false, true, true];
/// Puntos en memoria por partida; al llegar se agrupan de dos en dos.
const TL_CAP: usize = 20_000;
/// Puntos que se guardan en disco por registro.
const TL_SAVE: usize = 1500;

/// Evolución de una partida muestra a muestra.
#[derive(Default, Clone)]
pub struct Timeline {
    pub t: Vec<f64>,
    /// fps, low1, ftMax, cpu, gpu, gpuPower, gpuTemp, cpuTemp
    pub s: [Vec<f32>; 8],
}

impl Timeline {
    pub fn push(&mut self, t: f64, p: TlPoint) {
        self.t.push(t);
        for (i, v) in [p.fps, p.low1, p.ft_max, p.cpu, p.gpu, p.gpu_power, p.gpu_temp, p.cpu_temp].into_iter().enumerate() {
            self.s[i].push(v);
        }
        if self.t.len() >= TL_CAP {
            self.compact();
        }
    }

    pub fn len(&self) -> usize {
        self.t.len()
    }

    pub fn is_empty(&self) -> bool {
        self.t.is_empty()
    }

    fn compact(&mut self) {
        fn pair<T: Copy>(v: &[T], f: impl Fn(T, T) -> T) -> Vec<T> {
            let mut out: Vec<T> = v.chunks_exact(2).map(|c| f(c[0], c[1])).collect();
            if v.len() % 2 == 1 {
                out.push(v[v.len() - 1]);
            }
            out
        }
        self.t = pair(&self.t, |a, b| (a + b) / 2.0);
        for (i, s) in self.s.iter_mut().enumerate() {
            *s = if TL_MAX[i] { pair(s, f32::max) } else { pair(s, |a, b| (a + b) / 2.0) };
        }
    }

    /// Reduce a ≤ `max` puntos desde `from` (media por cubo; máximo en tirones y temperaturas).
    pub fn export(&self, from: usize, max: usize) -> SessionDetail {
        let n = self.t.len().saturating_sub(from);
        let k = n.div_ceil(max.max(1)).max(1);
        let t0 = if n > 0 { self.t[from] } else { crate::format::now_ms() as f64 / 1000.0 };
        let r2 = |x: f64| (x * 100.0).round() / 100.0;
        let bucket = |arr: &[f32], use_max: bool| -> Vec<f32> {
            arr[from.min(arr.len())..]
                .chunks(k)
                .map(|c| {
                    let v = if use_max { c.iter().copied().fold(f32::MIN, f32::max) } else { c.iter().sum::<f32>() / c.len() as f32 };
                    r2(v as f64) as f32
                })
                .collect()
        };
        let t: Vec<f64> = self.t[from.min(self.t.len())..]
            .chunks(k)
            .map(|c| ((c.iter().sum::<f64>() / c.len() as f64 - t0) * 10.0).round() / 10.0)
            .collect();
        let s: Vec<Vec<f32>> = (0..8).map(|i| bucket(&self.s[i], TL_MAX[i])).collect();
        SessionDetail {
            v: 1,
            t0: t0 * 1000.0,
            t,
            fps: s[0].clone(),
            low1: s[1].clone(),
            ft_max: s[2].clone(),
            cpu: s[3].clone(),
            gpu: s[4].clone(),
            gpu_power: s[5].clone(),
            gpu_temp: s[6].clone(),
            cpu_temp: s[7].clone(),
            ..Default::default()
        }
    }
}

fn detail_of(tl: &Timeline, from: usize, st: &FrameStats, frametimes: Option<&[f32]>) -> SessionDetail {
    SessionDetail {
        hist: Histogram { step: 0.25, counts: st.grouped(0.25, 100.0) },
        pct: Percentiles {
            p50: st.percentile(50.0),
            p90: st.percentile(90.0),
            p95: st.percentile(95.0),
            p99: st.percentile(99.0),
            p999: st.percentile(99.9),
            max: st.max,
            median: st.percentile(50.0),
        },
        frametimes: frametimes.map(|f| f[..f.len().min(300_000)].to_vec()),
        ..tl.export(from, TL_SAVE)
    }
}

/// Grabación de benchmark en curso; `from` = primer punto de la línea de tiempo.
pub struct Recording {
    pub stats: FrameStats,
    pub frames: Vec<f32>,
    pub res: ResAcc,
    /// Inicio y fin (s, reloj de las muestras); `until = None` hasta que se detenga a mano.
    pub start: f64,
    pub until: Option<f64>,
    pub min_fps: f32,
    pub max_fps: f32,
    pub from: usize,
}

pub struct GameTrack {
    pub pid: u32,
    pub info: GameSnapshot,
    /// Últimos tiempos de fotograma (anillo) y su instante (s).
    recent: Vec<f32>,
    recent_at: Vec<f64>,
    n: usize,
    pub session: FrameStats,
    /// FPS por segundo mínimo/máximo de la sesión.
    pub min_fps: f32,
    pub max_fps: f32,
    /// Recursos del sistema durante la partida (mientras el juego está activo).
    pub res: ResAcc,
    /// Instante (ms) en que el juego se cerró; `None` mientras sigue abierto.
    pub ended: Option<u64>,
    pub timeline: Timeline,
    /// Id del registro guardado al cerrar el juego.
    pub saved_id: Option<String>,
    pub rec: Option<Recording>,
}

impl GameTrack {
    pub fn new(info: GameSnapshot) -> Self {
        GameTrack {
            pid: info.pid,
            info,
            recent: vec![0.0; RECENT],
            recent_at: vec![0.0; RECENT],
            n: 0,
            session: FrameStats::default(),
            min_fps: f32::INFINITY,
            max_fps: 0.0,
            res: ResAcc::default(),
            ended: None,
            timeline: Timeline::default(),
            saved_id: None,
            rec: None,
        }
    }

    /// Mediana de los últimos `k` fotogramas.
    pub fn median(&self, k: usize) -> f32 {
        let m = self.n.min(k).min(RECENT);
        if m == 0 {
            return 0.0;
        }
        let mut a: Vec<f32> = (0..m).map(|i| self.recent[(self.n - 1 - i) % RECENT]).collect();
        a.sort_by(f32::total_cmp);
        a[m / 2]
    }

    /// Últimos fotogramas en orden, con su tiempo relativo al último (s, ≤ 0).
    pub fn last_frames(&self, seconds: f64) -> (Vec<f64>, Vec<f32>) {
        let (mut x, mut y) = (Vec::new(), Vec::new());
        if self.n == 0 {
            return (x, y);
        }
        let end = self.recent_at[(self.n - 1) % RECENT];
        let m = self.n.min(RECENT);
        for i in self.n - m..self.n {
            let t = self.recent_at[i % RECENT] - end;
            if t >= -seconds {
                x.push(t);
                y.push(self.recent[i % RECENT]);
            }
        }
        (x, y)
    }

    /// 1 % bajo de los últimos `seconds` segundos.
    pub fn live_low(&self, seconds: f64) -> f32 {
        let (_, mut y) = self.last_frames(seconds);
        if y.len() < 20 {
            return 0.0;
        }
        y.sort_by(f32::total_cmp);
        let i = ((y.len() as f64 * 0.99).ceil() as usize).saturating_sub(1).min(y.len() - 1);
        1000.0 / y[i]
    }

    pub fn ingest(&mut self, g: &GameSnapshot, now: f64) {
        self.info = g.clone();
        let med = self.median(120);
        // los fotogramas de esta muestra se reparten hacia atrás desde `now` según su duración
        let mut t = now - g.frametimes.iter().map(|&f| f as f64).sum::<f64>() / 1000.0;
        for &ft in &g.frametimes {
            t += ft as f64 / 1000.0;
            let i = self.n % RECENT;
            self.recent[i] = ft;
            self.recent_at[i] = t;
            self.n += 1;
            self.session.add(ft, med);
            if let Some(r) = &mut self.rec {
                r.stats.add(ft, med);
                r.frames.push(ft);
            }
        }
        if g.active && g.fps > 0.0 {
            self.min_fps = self.min_fps.min(g.fps);
            self.max_fps = self.max_fps.max(g.fps);
            if let Some(r) = &mut self.rec {
                r.min_fps = r.min_fps.min(g.fps);
                r.max_fps = r.max_fps.max(g.fps);
            }
        }
    }

    pub fn reset_session(&mut self) {
        self.session = FrameStats::default();
        self.min_fps = f32::INFINITY;
        self.max_fps = 0.0;
        self.res = ResAcc::default();
        self.timeline = Timeline::default();
        // también la gráfica por fotograma y el 1 % bajo en vivo
        self.n = 0;
    }

    fn summary(&self, id: String, kind: &str, date: u64, st: &FrameStats, res: &ResAcc, min_fps: f32, max_fps: f32) -> Benchmark {
        Benchmark {
            id,
            kind: Some(kind.into()),
            title: self.info.title.clone(),
            exe: self.info.exe.clone(),
            api: self.info.api.clone(),
            date,
            seconds: st.seconds(),
            frames: st.count,
            avg: st.avg_fps(),
            low1: st.low1(),
            low01: st.low01(),
            min_fps: if min_fps.is_finite() { min_fps } else { 0.0 },
            max_fps,
            stutters: st.stutters,
            hitches: st.hitches,
            stability: st.stability(),
            jitter_ms: st.jitter_ms(),
            cpu: ResAcc::mean(res.cpu, res.n),
            gpu: ResAcc::mean(res.gpu, res.n),
            gpu_power: ResAcc::mean(res.gpu_power, res.n),
            cpu_temp_max: res.cpu_temp_max,
            gpu_temp_max: res.gpu_temp_max,
        }
    }
}

/// Identificador aleatorio de 8 caracteres para un registro.
pub fn new_id() -> String {
    crate::config::new_token()[..8].to_string()
}

/// Todos los juegos medidos y el historial de partidas.
#[derive(Default)]
pub struct Games {
    pub tracks: HashMap<u32, GameTrack>,
    /// pid del juego que se muestra.
    pub selected: Option<u32>,
    /// Historial (resúmenes), más reciente primero.
    pub benchmarks: Vec<Benchmark>,
    /// Última grabación terminada (se muestra su resumen).
    pub last_bench: Option<Benchmark>,
    /// Fotogramas de los benchmarks grabados en esta sesión (para exportar sin leer el disco).
    frames: HashMap<String, Vec<f32>>,
    details: HashMap<String, SessionDetail>,
    store: Option<Sessions>,
    /// La capa está instalada.
    pub layer_installed: bool,
}

impl Games {
    /// Con historial en disco (`None` en pruebas: solo en memoria).
    pub fn new(store: Option<Sessions>) -> Self {
        let benchmarks = store
            .as_ref()
            .map(|s| s.list().into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
            .unwrap_or_default();
        Games { store, benchmarks, ..Default::default() }
    }

    /// Juegos ordenados: activos, luego en pausa, luego terminados (más reciente primero).
    pub fn list(&self) -> Vec<&GameTrack> {
        let rank = |t: &GameTrack| if t.info.active { 2 } else if t.ended.is_some() { 0 } else { 1 };
        let mut v: Vec<&GameTrack> = self.tracks.values().collect();
        v.sort_by(|a, b| {
            rank(b).cmp(&rank(a)).then(b.ended.unwrap_or(0).cmp(&a.ended.unwrap_or(0))).then(b.info.fps.total_cmp(&a.info.fps)).then(a.pid.cmp(&b.pid))
        });
        v
    }

    pub fn current(&self) -> Option<&GameTrack> {
        self.selected.and_then(|p| self.tracks.get(&p)).or_else(|| self.list().into_iter().next())
    }

    pub fn current_pid(&self) -> Option<u32> {
        self.current().map(|t| t.pid)
    }

    /// Procesa una muestra; añade FPS y 1 % bajo de cada juego al historial general.
    pub fn ingest(&mut self, s: &Snapshot, h: &mut History) {
        self.layer_installed = s.games.layer_installed;
        let now = s.ts as f64 / 1000.0;
        for g in &s.games.games {
            // un pid que vuelve tras cerrarse es otro proceso: se empieza de cero
            if self.tracks.get(&g.pid).is_none_or(|t| t.ended.is_some()) {
                self.tracks.insert(g.pid, GameTrack::new(g.clone()));
            }
            let tr = self.tracks.get_mut(&g.pid).unwrap();
            tr.ingest(g, now);
            let low = tr.live_low(LIVE_WINDOW);
            h.push(&format!("game.{}.fps", g.pid), if g.active { g.fps } else { 0.0 });
            h.push(&format!("game.{}.low", g.pid), if g.active { low } else { 0.0 });
            if g.active {
                tr.res.sample(s);
                let gpu = main_gpu(&s.gpus);
                tr.timeline.push(
                    now,
                    TlPoint {
                        fps: g.fps,
                        low1: low,
                        ft_max: g.frametimes.iter().copied().fold(0.0, f32::max),
                        cpu: s.cpu.total.usage,
                        gpu: gpu.and_then(|g| g.busy).unwrap_or(0.0),
                        gpu_power: gpu.and_then(|g| g.power).unwrap_or(0.0),
                        gpu_temp: gpu.and_then(|g| g.temp_hotspot.or(g.temp_edge)).unwrap_or(0.0),
                        cpu_temp: s.cpu.temp.unwrap_or(0.0),
                    },
                );
            }
            if let Some(r) = &mut tr.rec {
                r.res.sample(s);
            }
        }
        // juegos que ya no están: se termina la partida; las grabaciones con tiempo se cierran solas
        let present: Vec<u32> = s.games.games.iter().map(|g| g.pid).collect();
        let pids: Vec<u32> = self.tracks.keys().copied().collect();
        for pid in pids {
            let tr = &self.tracks[&pid];
            if !present.contains(&pid) && tr.ended.is_none() {
                self.end(pid, s.ts);
            }
            if self.tracks[&pid].rec.as_ref().and_then(|r| r.until).is_some_and(|u| now >= u) {
                self.stop(pid, s.ts);
            }
        }
    }

    /// El juego se ha cerrado: se termina la grabación y se guarda la partida.
    fn end(&mut self, pid: u32, now_ms: u64) {
        if self.tracks[&pid].rec.is_some() {
            self.stop(pid, now_ms);
        }
        let tr = self.tracks.get_mut(&pid).unwrap();
        tr.ended = Some(now_ms);
        tr.info.active = false;
        tr.info.fps = 0.0;
        let st = &tr.session;
        // partidas de menos de 10 s (lanzadores, pantallas de carga) no se guardan
        if st.count >= 100 && st.seconds() >= 10.0 {
            let id = new_id();
            tr.saved_id = Some(id.clone());
            let sum = tr.summary(id, "session", now_ms, st, &tr.res, tr.min_fps, tr.max_fps);
            let detail = detail_of(&tr.timeline, 0, st, None);
            self.save(sum, detail);
        }
    }

    /// Quita de la vista un juego ya cerrado.
    pub fn dismiss(&mut self, pid: u32) {
        if self.tracks.get(&pid).is_some_and(|t| t.ended.is_some()) {
            self.tracks.remove(&pid);
        }
        if self.selected == Some(pid) {
            self.selected = None;
        }
    }

    /// Empieza a grabar un benchmark (`seconds = None`: hasta que se detenga a mano).
    pub fn start(&mut self, pid: u32, seconds: Option<f64>, now: f64) {
        if let Some(tr) = self.tracks.get_mut(&pid) {
            tr.rec = Some(Recording {
                stats: FrameStats::default(),
                frames: Vec::new(),
                res: ResAcc::default(),
                start: now,
                until: seconds.map(|s| now + s),
                min_fps: f32::INFINITY,
                max_fps: 0.0,
                from: tr.timeline.len(),
            });
        }
    }

    /// Termina la grabación; con menos de 30 fotogramas se descarta.
    pub fn stop(&mut self, pid: u32, now_ms: u64) -> Option<Benchmark> {
        let tr = self.tracks.get_mut(&pid)?;
        let rec = tr.rec.take()?;
        if rec.stats.count < 30 {
            return None;
        }
        let b = tr.summary(new_id(), "bench", now_ms, &rec.stats, &rec.res, rec.min_fps, rec.max_fps);
        let detail = detail_of(&tr.timeline, rec.from, &rec.stats, Some(&rec.frames));
        self.frames.insert(b.id.clone(), rec.frames);
        self.last_bench = Some(b.clone());
        self.save(b.clone(), detail);
        Some(b)
    }

    fn save(&mut self, sum: Benchmark, detail: SessionDetail) {
        self.benchmarks.insert(0, sum.clone());
        if let Some(st) = &self.store {
            let res = st.save(serde_json::to_value(&sum).unwrap_or_default(), Some(serde_json::to_value(&detail).unwrap_or_default()));
            if let Err(e) = res {
                eprintln!("NeonTop: no se pudo guardar la partida: {e}");
            }
        }
        self.details.insert(sum.id, detail);
    }

    /// Datos completos de un registro (se leen del disco la primera vez).
    pub fn detail(&mut self, id: &str) -> Option<&SessionDetail> {
        if !self.details.contains_key(id) {
            let d: SessionDetail = serde_json::from_value(self.store.as_ref()?.load(id).ok()?).ok()?;
            self.details.insert(id.to_string(), d);
        }
        self.details.get(id)
    }

    pub fn remove(&mut self, id: &str) {
        self.benchmarks.retain(|b| b.id != id);
        if self.last_bench.as_ref().is_some_and(|b| b.id == id) {
            self.last_bench = None;
        }
        self.details.remove(id);
        self.frames.remove(id);
        if let Some(st) = &self.store {
            let _ = st.delete(id);
        }
    }

    /// Grabación anterior del mismo juego (para comparar).
    pub fn previous(&self, b: &Benchmark) -> Option<&Benchmark> {
        self.benchmarks.iter().find(|x| x.exe == b.exe && x.date < b.date)
    }

    /// CSV con los tiempos de fotograma de una grabación.
    pub fn bench_csv(&mut self, id: &str) -> String {
        let frames = match self.frames.get(id) {
            Some(f) => f.clone(),
            None => self.detail(id).and_then(|d| d.frametimes.clone()).unwrap_or_default(),
        };
        let mut out = String::from("fotograma,tiempo_ms,fps\n");
        for (i, ft) in frames.iter().enumerate() {
            out.push_str(&format!("{},{ft:.3},{:.1}\n", i + 1, 1000.0 / ft));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats_of(frames: &[f32]) -> FrameStats {
        let mut s = FrameStats::default();
        for &f in frames {
            s.add(f, 16.6);
        }
        s
    }

    #[test]
    fn steady_60fps() {
        let s = stats_of(&[16.6; 600]);
        assert!((s.avg_fps() - 60.24).abs() < 0.1, "{}", s.avg_fps());
        assert!((s.percentile(99.0) - 16.625).abs() < 0.05);
        assert!((s.low1() - 60.2).abs() < 0.3);
        assert_eq!(s.stutters, 0);
        assert_eq!(s.hitches, 0);
        assert_eq!(s.jitter_ms(), 0.0);
        assert!(s.stability() > 99.0, "{}", s.stability());
        assert!((s.seconds() - 9.96).abs() < 0.01);
    }

    #[test]
    fn stutters_and_hitches() {
        let mut f = vec![16.6f32; 1000];
        f[100] = 40.0; // tirón (> 2× mediana y ≥ 4 ms)
        f[500] = 80.0; // parón (> 50 ms y 3× mediana) y también tirón
        let s = stats_of(&f);
        assert_eq!(s.stutters, 2);
        assert_eq!(s.hitches, 1);
        assert!(s.low01() < s.low1());
        assert!(s.stability() < 100.0);
        assert_eq!(s.max, 80.0);
    }

    #[test]
    fn percentiles_and_histogram() {
        let frames: Vec<f32> = (1..=100).map(|i| i as f32).collect();
        let s = stats_of(&frames);
        assert!((s.percentile(50.0) - 50.025).abs() < 0.05);
        assert!((s.percentile(99.0) - 99.025).abs() < 0.05);
        let g = s.grouped(10.0, 100.0);
        assert_eq!(g.len(), 10);
        assert_eq!(g.iter().sum::<u32>(), 100);
        // lo que pasa del tope cae en el último cubo
        let s = stats_of(&[1000.0]);
        assert_eq!(s.percentile(50.0), (NBINS as f32 - 0.5) * BIN_MS);
        assert_eq!(FrameStats::default().percentile(99.0), 0.0);
    }

    #[test]
    fn labels() {
        assert_eq!(stability_label(95.0, 1000).0, "Excelente");
        assert_eq!(stability_label(80.0, 1000).0, "Buena");
        assert_eq!(stability_label(60.0, 1000).0, "Irregular");
        assert_eq!(stability_label(10.0, 1000).0, "Mala");
        assert_eq!(stability_label(99.0, 5).0, "sin datos");
    }

    #[test]
    fn timeline_compacts_and_exports() {
        let mut tl = Timeline::default();
        for i in 0..TL_CAP {
            tl.push(i as f64, TlPoint { fps: 60.0, ft_max: (i % 7) as f32, ..Default::default() });
        }
        assert_eq!(tl.len(), TL_CAP / 2);
        // el máximo se conserva al agrupar
        assert!(tl.s[2].iter().any(|&v| v == 6.0));
        let d = tl.export(0, 100);
        assert!(d.t.len() <= 100);
        assert_eq!(d.t.len(), d.fps.len());
        // `t` en segundos desde el primer punto: el primer cubo es la media de sus instantes
        assert!(d.t[0] >= 0.0 && d.t.windows(2).all(|w| w[0] < w[1]));
        assert!(d.fps.iter().all(|&v| v == 60.0));
    }

    fn game(pid: u32, frames: Vec<f32>) -> GameSnapshot {
        GameSnapshot { pid, title: "Juego".into(), exe: "juego".into(), api: "vulkan".into(), bits: 64, fps: 60.0, active: true, frametimes: frames, ..Default::default() }
    }

    fn snap(ts: u64, games: Vec<GameSnapshot>) -> Snapshot {
        let mut s = Snapshot { ts, ..Default::default() };
        s.games.games = games;
        s
    }

    #[test]
    fn session_lifecycle() {
        let mut g = Games::new(None);
        let mut h = History::new();
        // 15 s de juego a 60 FPS
        for i in 0..15u64 {
            let s = snap(1_000_000 + i * 1000, vec![game(42, vec![16.6; 60])]);
            h.begin(s.ts as f64 / 1000.0);
            g.ingest(&s, &mut h);
            h.end();
        }
        assert_eq!(g.list().len(), 1);
        assert_eq!(g.current_pid(), Some(42));
        let tr = &g.tracks[&42];
        assert_eq!(tr.session.count, 900);
        assert!((tr.live_low(LIVE_WINDOW) - 60.2).abs() < 0.5);
        assert_eq!(h.get("game.42.fps").unwrap().len(), 15);
        // benchmark manual de 5 s
        g.start(42, None, 1_015.0);
        for i in 15..20u64 {
            let s = snap(1_000_000 + i * 1000, vec![game(42, vec![16.6; 60])]);
            g.ingest(&s, &mut h);
        }
        let b = g.stop(42, 1_020_000).expect("benchmark guardado");
        assert!(b.is_bench());
        assert_eq!(b.frames, 300);
        assert!(g.bench_csv(&b.id).lines().count() == 301);
        // el juego se cierra: la partida se guarda
        g.ingest(&snap(1_021_000, vec![]), &mut h);
        let tr = &g.tracks[&42];
        assert!(tr.ended.is_some());
        assert!(!tr.info.active);
        assert_eq!(g.benchmarks.len(), 2);
        let sess = &g.benchmarks[0];
        assert_eq!(sess.kind.as_deref(), Some("session"));
        assert!((sess.avg - 60.2).abs() < 0.3);
        assert_eq!(g.previous(&g.benchmarks[0].clone()).map(|x| x.id.clone()), Some(b.id.clone()));
        let id = sess.id.clone();
        assert!(g.detail(&id).is_some_and(|d| d.v == 1 && !d.fps.is_empty()));
        g.dismiss(42);
        assert!(g.tracks.is_empty());
        g.remove(&id);
        assert_eq!(g.benchmarks.len(), 1);
    }

    #[test]
    fn short_sessions_are_not_saved() {
        let mut g = Games::new(None);
        let mut h = History::new();
        g.ingest(&snap(1000, vec![game(7, vec![16.6; 30])]), &mut h);
        g.ingest(&snap(2000, vec![]), &mut h);
        assert!(g.benchmarks.is_empty());
        assert!(g.tracks[&7].ended.is_some());
        // el mismo pid vuelve: es otro proceso
        g.ingest(&snap(3000, vec![game(7, vec![16.6; 10])]), &mut h);
        assert!(g.tracks[&7].ended.is_none());
        assert_eq!(g.tracks[&7].session.count, 10);
    }

    #[test]
    fn timed_recording_stops_itself() {
        let mut g = Games::new(None);
        let mut h = History::new();
        g.ingest(&snap(0, vec![game(1, vec![16.6; 60])]), &mut h);
        g.start(1, Some(2.0), 0.0);
        for i in 1..=3u64 {
            g.ingest(&snap(i * 1000, vec![game(1, vec![16.6; 60])]), &mut h);
        }
        assert!(g.tracks[&1].rec.is_none());
        assert!(g.last_bench.is_some());
    }

    #[test]
    fn benchmark_json_matches_web_format() {
        let b = Benchmark { id: "abc".into(), kind: Some("bench".into()), min_fps: 1.0, gpu_temp_max: 2.0, ..Default::default() };
        let v = serde_json::to_value(&b).unwrap();
        assert!(v.get("minFps").is_some() && v.get("gpuTempMax").is_some() && v.get("jitterMs").is_some());
        // los registros antiguos sin algunos campos se siguen leyendo
        let old: Benchmark = serde_json::from_str(r#"{"id":"x","title":"T","exe":"t","api":"vulkan","date":1,"avg":60}"#).unwrap();
        assert_eq!(old.avg, 60.0);
        assert!(!old.is_bench());
    }
}
