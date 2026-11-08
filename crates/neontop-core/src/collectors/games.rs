//! Juegos: tiempos de fotograma medidos por la capa NeonTop FPS (packaging/layer).
//!
//! Cada juego lanzado con `neontop-fps` escribe en /dev/shm/neontop-fps-<pid> un anillo con el
//! instante de cada fotograma presentado (ver neontop_fps.h: el formato debe coincidir). Aquí se
//! mapea en memoria (solo lectura) y en cada muestra se envían los tiempos de fotograma nuevos;
//! las estadísticas (1 % bajo, tirones, estabilidad…) se calculan en la interfaz.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

const MAGIC: u64 = 0x3153_5046_5450_4e4e;
const HEADER: usize = 112;
/// Sin fotogramas en este tiempo el juego se considera en pausa / minimizado.
const IDLE_NS: u64 = 3_000_000_000;
/// Un proceso que ya no existe y lleva este tiempo sin fotogramas se da por terminado y su
/// segmento se borra. (Se exige también la inactividad por si el pid no fuera visible.)
const STALE_NS: u64 = 3_000_000_000;

pub const LAYER_MANIFEST: &str = "/usr/share/vulkan/implicit_layer.d/neontop_fps.x86_64.json";

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameSnapshot {
    pub pid: u32,
    /// Nombre del juego (de Steam si se conoce; si no, el ejecutable).
    pub title: String,
    pub exe: String,
    /// "vulkan" | "opengl" | "egl"
    pub api: String,
    pub bits: u32,
    pub steam_app_id: Option<u32>,
    /// Fotogramas totales desde que empezó a medir.
    pub frames: u64,
    /// Tiempos de fotograma nuevos desde la muestra anterior (ms), en orden.
    pub frametimes: Vec<f32>,
    /// FPS del último segundo.
    pub fps: f32,
    /// Presentando fotogramas ahora (no en pausa / minimizado).
    pub active: bool,
    /// Segundos desde el primer fotograma.
    pub duration: f64,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GamesSnapshot {
    pub games: Vec<GameSnapshot>,
    /// La capa está instalada en el sistema.
    pub layer_installed: bool,
}

struct Mapped {
    ptr: *const u8,
    len: usize,
}
// El mapeo es de solo lectura y solo lo usa el hilo de muestreo.
unsafe impl Send for Mapped {}

impl Mapped {
    fn open(path: &Path) -> Option<Mapped> {
        use std::os::unix::io::AsRawFd;
        let f = fs::File::open(path).ok()?;
        let len = f.metadata().ok()?.len() as usize;
        if len < HEADER + 8 {
            return None;
        }
        // SAFETY: mapeo de solo lectura de un fichero de /dev/shm; se comprueba el tamaño antes de leer.
        let ptr = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ, libc::MAP_SHARED, f.as_raw_fd(), 0) };
        if ptr == libc::MAP_FAILED {
            return None;
        }
        Some(Mapped { ptr: ptr as *const u8, len })
    }
    fn u32_at(&self, off: usize) -> u32 {
        // SAFETY: off + 4 <= HEADER <= len (comprobado al abrir)
        unsafe { std::ptr::read_volatile(self.ptr.add(off) as *const u32) }
    }
    fn u64_at(&self, off: usize) -> u64 {
        if off + 8 > self.len {
            return 0;
        }
        // SAFETY: dentro del mapeo; el escritor usa accesos alineados de 8 bytes
        unsafe { std::ptr::read_volatile(self.ptr.add(off) as *const u64) }
    }
    fn count(&self) -> u64 {
        // SAFETY: igual que u64_at; lectura con orden "acquire" para ver los ts ya escritos
        unsafe { (*(self.ptr.add(104) as *const std::sync::atomic::AtomicU64)).load(std::sync::atomic::Ordering::Acquire) }
    }
}

impl Drop for Mapped {
    fn drop(&mut self) {
        // SAFETY: ptr/len vienen de un mmap correcto
        unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
    }
}

struct Game {
    map: Mapped,
    path: PathBuf,
    pid: u32,
    exe: String,
    title: String,
    api: String,
    bits: u32,
    app_id: Option<u32>,
    cap: u64,
    start_ns: u64,
    /// Fotogramas ya enviados y el instante del último.
    seen: u64,
    last_ts: u64,
}

fn now_ns() -> u64 {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime solo escribe en `t`
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}

pub struct GamesCollector {
    games: HashMap<PathBuf, Game>,
    steam_names: HashMap<u32, Option<String>>,
    last_scan: Option<Instant>,
}

impl GamesCollector {
    pub fn new() -> Self {
        GamesCollector { games: HashMap::new(), steam_names: HashMap::new(), last_scan: None }
    }

    fn scan(&mut self) {
        let Ok(rd) = fs::read_dir("/dev/shm") else { return };
        for e in rd.flatten() {
            let name = e.file_name();
            let Some(pid) = name.to_str().and_then(|n| n.strip_prefix("neontop-fps-")).and_then(|p| p.parse::<u32>().ok()) else { continue };
            let path = e.path();
            if self.games.contains_key(&path) {
                continue;
            }
            let Some(map) = Mapped::open(&path) else { continue };
            if map.u64_at(0) != MAGIC || map.u32_at(8) != 1 {
                continue;
            }
            let cap = map.u32_at(20) as u64;
            if cap == 0 || HEADER + cap as usize * 8 > map.len {
                continue;
            }
            let api = match map.u32_at(16) {
                1 => "vulkan",
                2 => "opengl",
                3 => "egl",
                _ => "?",
            };
            let app_id = Some(map.u32_at(24)).filter(|v| *v != 0);
            // SAFETY: 32..96 está dentro de la cabecera
            let raw = unsafe { std::slice::from_raw_parts(map.ptr.add(32), 64) };
            let exe = String::from_utf8_lossy(&raw[..raw.iter().position(|b| *b == 0).unwrap_or(64)]).into_owned();
            let title = app_id.and_then(|id| self.steam_name(id)).or_else(|| java_game_name(pid, &exe)).unwrap_or_else(|| pretty_exe(&exe));
            let count = map.count();
            let last_ts = if count > 0 { map.u64_at(HEADER + ((count - 1) % cap) as usize * 8) } else { 0 };
            self.games.insert(
                path.clone(),
                Game { pid, exe, title, api: api.into(), bits: map.u32_at(28), app_id, cap, start_ns: map.u64_at(96), seen: count, last_ts, map, path },
            );
        }
    }

    fn steam_name(&mut self, id: u32) -> Option<String> {
        self.steam_names.entry(id).or_insert_with(|| steam_app_name(id)).clone()
    }

    pub fn sample(&mut self) -> GamesSnapshot {
        // buscar juegos nuevos cuesta un readdir de /dev/shm: cada segundo basta
        if self.last_scan.is_none_or(|t| t.elapsed().as_millis() >= 1000) {
            self.scan();
            self.last_scan = Some(Instant::now());
        }
        let now = now_ns();
        let mut out = Vec::new();
        let mut dead = Vec::new();
        for g in self.games.values_mut() {
            let count = g.map.count();
            let mut frametimes = Vec::new();
            if count > g.seen {
                // si se han perdido fotogramas (anillo desbordado), se empieza por los disponibles
                let from = g.seen.max(count.saturating_sub(g.cap));
                let mut prev = if from == g.seen { g.last_ts } else { 0 };
                for i in from..count {
                    let ts = g.map.u64_at(HEADER + (i % g.cap) as usize * 8);
                    if prev != 0 && ts > prev {
                        frametimes.push(((ts - prev) as f64 / 1e6) as f32);
                    }
                    prev = ts;
                }
                g.seen = count;
                g.last_ts = prev;
            }
            // FPS del último segundo, contando hacia atrás en el anillo
            let mut n = 0u64;
            let mut i = count;
            while i > 0 && n < g.cap {
                let ts = g.map.u64_at(HEADER + ((i - 1) % g.cap) as usize * 8);
                if now.saturating_sub(ts) > 1_000_000_000 {
                    break;
                }
                n += 1;
                i -= 1;
            }
            let idle = now.saturating_sub(g.last_ts);
            let alive = Path::new(&format!("/proc/{}", g.pid)).exists();
            if idle > STALE_NS && !alive {
                dead.push(g.path.clone());
                continue;
            }
            out.push(GameSnapshot {
                pid: g.pid,
                title: g.title.clone(),
                exe: g.exe.clone(),
                api: g.api.clone(),
                bits: g.bits,
                steam_app_id: g.app_id,
                frames: count,
                frametimes,
                fps: n as f32,
                active: idle < IDLE_NS && alive,
                duration: now.saturating_sub(g.start_ns) as f64 / 1e9,
            });
        }
        for p in dead {
            // el juego terminó sin limpiar (cierre forzado): se borra su segmento
            let _ = fs::remove_file(&p);
            self.games.remove(&p);
        }
        out.sort_by(|a, b| b.active.cmp(&a.active).then(b.fps.total_cmp(&a.fps)));
        GamesSnapshot { games: out, layer_installed: Path::new(LAYER_MANIFEST).exists() }
    }
}

/// Juegos que corren dentro de Java (el ejecutable es "java"): Minecraft se reconoce por su línea de
/// comandos ("net.minecraft…", "--version 1.21.4"); si no, se usa el .jar o la clase principal.
fn java_game_name(pid: u32, exe: &str) -> Option<String> {
    if !exe.starts_with("java") {
        return None;
    }
    let raw = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Vec<String> = raw.split(|b| *b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    let after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    if args.iter().any(|a| a.contains("minecraft") || a.contains("Minecraft")) {
        // Prism/MultiMC pasan la versión; con cargadores de mods puede venir como "fabric-loader-…"
        let version = after("--version").filter(|v| !v.is_empty());
        return Some(match version {
            Some(v) => format!("Minecraft {v}"),
            None => "Minecraft".to_string(),
        });
    }
    let jar = after("-jar").map(|j| pretty_exe(j.rsplit('/').next().unwrap_or(&j)).trim_end_matches(".jar").to_string());
    jar.or_else(|| args.iter().skip(1).find(|a| !a.starts_with('-') && a.contains('.')).map(|c| c.rsplit('.').next().unwrap_or(c).to_string()))
}

/// "Cyberpunk2077.exe" → "Cyberpunk2077".
fn pretty_exe(exe: &str) -> String {
    exe.strip_suffix(".exe").or_else(|| exe.strip_suffix(".EXE")).unwrap_or(exe).to_string()
}

/// Nombre de un juego de Steam a partir de su appmanifest, buscando en todas las bibliotecas.
fn steam_app_name(id: u32) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let roots = [format!("{home}/.local/share/Steam"), format!("{home}/.steam/steam"), format!("{home}/.var/app/com.valvesoftware.Steam/.local/share/Steam")];
    let mut libs: Vec<String> = Vec::new();
    for r in &roots {
        libs.push(r.clone());
        if let Ok(vdf) = fs::read_to_string(format!("{r}/steamapps/libraryfolders.vdf")) {
            libs.extend(vdf.lines().filter_map(|l| vdf_value(l, "path")));
        }
    }
    libs.iter().find_map(|l| fs::read_to_string(format!("{l}/steamapps/appmanifest_{id}.acf")).ok()?.lines().find_map(|l| vdf_value(l, "name")))
}

/// Valor de una línea `"clave"  "valor"` de un fichero VDF de Steam.
fn vdf_value(line: &str, key: &str) -> Option<String> {
    let mut parts = line.trim().split('"').filter(|s| !s.trim().is_empty());
    (parts.next()? == key).then(|| parts.next().map(|v| v.replace("\\\\", "\\")))?
}

#[cfg(test)]
mod tests {
    #[test]
    fn vdf() {
        assert_eq!(super::vdf_value("\t\t\"name\"\t\t\"Hades II\"", "name").as_deref(), Some("Hades II"));
        assert_eq!(super::vdf_value("\"path\"  \"/mnt/juegos\"", "name"), None);
        assert_eq!(super::pretty_exe("Game.exe"), "Game");
    }

    #[test]
    fn samples_games() {
        let mut c = super::GamesCollector::new();
        let s = c.sample();
        eprintln!("capa instalada: {}", s.layer_installed);
        for g in &s.games {
            eprintln!("{} [{}] {} {} fps, {} fotogramas", g.title, g.pid, g.api, g.fps, g.frames);
        }
    }
}
