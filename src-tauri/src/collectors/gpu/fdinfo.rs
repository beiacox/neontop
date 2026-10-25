//! Uso de GPU por proceso desde /proc/<pid>/fdinfo (interfaz DRM estándar, ver
//! Documentation/gpu/drm-usage-stats.rst). Funciona con amdgpu, i915, xe, msm, panfrost…
//! Solo se ven los procesos cuyos descriptores podemos leer (los del propio usuario).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::Instant;

use super::{GpuEngine, GpuProc};

#[derive(Default, Clone)]
pub struct DeviceUsage {
    /// Uso total de cada motor (% sumado entre clientes, máx. 100).
    pub engines: Vec<GpuEngine>,
    pub processes: Vec<GpuProc>,
}

#[derive(Default, Clone)]
pub struct Usage {
    /// Por ranura PCI (drm-pdev).
    pub devices: HashMap<String, DeviceUsage>,
}

#[derive(Clone, Copy)]
enum Counter {
    /// Tiempo ocupado acumulado (amdgpu, i915).
    Ns(u64),
    /// Ciclos ocupados y totales (xe).
    Cycles(u64, u64),
}

#[derive(Default)]
struct Client {
    pdev: String,
    id: u64,
    engines: Vec<(String, Counter, u32)>,
    vram: u64,
    gtt: u64,
}

pub struct FdInfo {
    prev: HashMap<(String, u64, String), Counter>,
    prev_at: Instant,
    names: HashMap<u32, String>,
    /// Descriptores DRM conocidos por proceso. Recorrer todos los fd del sistema (readlink
    /// de miles de enlaces) cuesta ~20 ms, así que solo se hace cada FULL_SCAN; entre medias
    /// se releen únicamente los fdinfo ya conocidos (~1 ms).
    known: Vec<(u32, String)>,
    last_full: Option<Instant>,
}

const FULL_SCAN: std::time::Duration = std::time::Duration::from_secs(5);

/// "53748 KiB", "4 MiB", "0" → bytes
fn parse_size(v: &str) -> u64 {
    let mut it = v.split_whitespace();
    let n: u64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    match it.next() {
        Some("KiB") => n << 10,
        Some("MiB") => n << 20,
        Some("GiB") => n << 30,
        _ => n,
    }
}

fn parse_client(text: &str) -> Option<Client> {
    let mut c = Client::default();
    let mut caps: HashMap<String, u32> = HashMap::new();
    let mut cycles: HashMap<String, (u64, u64)> = HashMap::new();
    // memoria: preferimos "resident" (lo realmente en VRAM) sobre "total"/"memory"
    let (mut vram_res, mut vram_any, mut gtt_res, mut gtt_any) = (None::<u64>, 0u64, None::<u64>, 0u64);

    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        match k {
            "drm-pdev" => c.pdev = v.to_string(),
            "drm-client-id" => c.id = v.parse().ok()?,
            _ => {
                if let Some(e) = k.strip_prefix("drm-engine-capacity-") {
                    caps.insert(e.into(), v.parse().unwrap_or(1));
                } else if let Some(e) = k.strip_prefix("drm-engine-") {
                    let ns = v.trim_end_matches("ns").trim().parse().unwrap_or(0);
                    c.engines.push((e.into(), Counter::Ns(ns), 1));
                } else if let Some(e) = k.strip_prefix("drm-total-cycles-") {
                    cycles.entry(e.into()).or_default().1 = v.parse().unwrap_or(0);
                } else if let Some(e) = k.strip_prefix("drm-cycles-") {
                    cycles.entry(e.into()).or_default().0 = v.parse().unwrap_or(0);
                } else if let Some(region) = k.strip_prefix("drm-resident-") {
                    let b = parse_size(v);
                    if region.starts_with("vram") || region.starts_with("local") {
                        *vram_res.get_or_insert(0) += b;
                    } else if region == "gtt" || region.starts_with("system") {
                        *gtt_res.get_or_insert(0) += b;
                    }
                } else if let Some(region) = k.strip_prefix("drm-memory-").or_else(|| k.strip_prefix("drm-total-")) {
                    let b = parse_size(v);
                    if region.starts_with("vram") || region.starts_with("local") {
                        vram_any += b;
                    } else if region == "gtt" || region.starts_with("system") {
                        gtt_any += b;
                    }
                }
            }
        }
    }
    if c.pdev.is_empty() {
        return None;
    }
    for (e, (cy, total)) in cycles {
        c.engines.push((e, Counter::Cycles(cy, total), 1));
    }
    for (e, _, cap) in &mut c.engines {
        *cap = caps.get(e.as_str()).copied().unwrap_or(1).max(1);
    }
    c.vram = vram_res.unwrap_or(vram_any);
    c.gtt = gtt_res.unwrap_or(gtt_any);
    Some(c)
}

impl FdInfo {
    pub fn new() -> Self {
        FdInfo {
            prev: HashMap::new(),
            prev_at: Instant::now(),
            names: HashMap::new(),
            known: Vec::new(),
            last_full: None,
        }
    }

    /// Lista (pid, fd) de todos los descriptores abiertos sobre /dev/dri.
    fn full_scan() -> Vec<(u32, String)> {
        let mut out = Vec::new();
        let Ok(rd) = fs::read_dir("/proc") else { return out };
        for entry in rd.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            // sin permiso (procesos de otros usuarios) read_dir falla y se salta
            let Ok(fds) = fs::read_dir(format!("/proc/{pid}/fd")) else { continue };
            for fd in fds.flatten() {
                if fs::read_link(fd.path()).is_ok_and(|t| t.as_os_str().to_string_lossy().starts_with("/dev/dri/")) {
                    out.push((pid, fd.file_name().to_string_lossy().into_owned()));
                }
            }
        }
        out
    }

    pub fn sample(&mut self) -> Usage {
        let now = Instant::now();
        let dt_ns = now.duration_since(self.prev_at).as_nanos().max(1) as f64;
        self.prev_at = now;

        let mut next_prev = HashMap::new();
        let mut seen_clients: HashSet<(String, u64)> = HashSet::new();
        // (pdev, pid) → proceso acumulado
        let mut procs: HashMap<(String, u32), GpuProc> = HashMap::new();
        let mut engines: HashMap<String, HashMap<String, f32>> = HashMap::new();
        let mut alive = HashSet::new();

        if self.last_full.is_none_or(|t| t.elapsed() >= FULL_SCAN) {
            self.known = Self::full_scan();
            self.last_full = Some(now);
        }
        let mut still_open = Vec::with_capacity(self.known.len());
        for (pid, fd) in std::mem::take(&mut self.known) {
            // el descriptor puede haberse cerrado (o el pid reutilizado): se descarta
            let Ok(text) = fs::read_to_string(format!("/proc/{pid}/fdinfo/{fd}")) else {
                continue;
            };
            if !text.contains("drm-client-id") {
                continue;
            }
            still_open.push((pid, fd));
            let Some(client) = parse_client(&text) else { continue };
            // varios descriptores (incluso de procesos distintos) pueden ser el mismo cliente
            if !seen_clients.insert((client.pdev.clone(), client.id)) {
                continue;
            }
            alive.insert(pid);
            let name = self
                .names
                .entry(pid)
                .or_insert_with(|| {
                    fs::read_to_string(format!("/proc/{pid}/comm"))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default()
                })
                .clone();
            let p = procs.entry((client.pdev.clone(), pid)).or_insert_with(|| GpuProc {
                pid,
                name,
                ..Default::default()
            });
            p.vram += client.vram;
            p.gtt += client.gtt;

            for (eng, counter, cap) in &client.engines {
                let key = (client.pdev.clone(), client.id, eng.clone());
                let busy = match (self.prev.get(&key), counter) {
                    (Some(Counter::Ns(a)), Counter::Ns(b)) => b.saturating_sub(*a) as f64 / dt_ns * 100.0,
                    (Some(Counter::Cycles(a, ta)), Counter::Cycles(b, tb)) if tb > ta => b.saturating_sub(*a) as f64 / (tb - ta) as f64 * 100.0,
                    _ => 0.0,
                } as f32
                    / *cap as f32;
                next_prev.insert(key, *counter);
                let busy = busy.clamp(0.0, 100.0);
                match p.engines.iter_mut().find(|e| &e.name == eng) {
                    Some(e) => e.busy += busy,
                    None => p.engines.push(GpuEngine { name: eng.clone(), busy }),
                }
                *engines.entry(client.pdev.clone()).or_default().entry(eng.clone()).or_default() += busy;
            }
        }
        self.known = still_open;
        self.prev = next_prev;
        self.names.retain(|pid, _| alive.contains(pid));

        let mut usage = Usage::default();
        for ((pdev, _), p) in procs {
            usage.devices.entry(pdev).or_default().processes.push(p);
        }
        for (pdev, engs) in engines {
            let mut list: Vec<GpuEngine> = engs.into_iter().map(|(name, busy)| GpuEngine { name, busy: busy.min(100.0) }).collect();
            list.sort_by(|a, b| a.name.cmp(&b.name));
            usage.devices.entry(pdev).or_default().engines = list;
        }
        usage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_cost() {
        let mut f = FdInfo::new();
        f.sample();
        let t = Instant::now();
        let u = f.sample();
        eprintln!(
            "fdinfo: {:?} · {} dispositivos · {} procesos",
            t.elapsed(),
            u.devices.len(),
            u.devices.values().map(|d| d.processes.len()).sum::<usize>()
        );
    }

    #[test]
    fn parses_amdgpu_and_xe() {
        let amd = "drm-driver:\tamdgpu\ndrm-client-id:\t574\ndrm-pdev:\t0000:2d:00.0\ndrm-resident-vram:\t53748 KiB\ndrm-purgeable-vram:\t4 MiB\ndrm-memory-vram:\t53748 KiB\ndrm-resident-gtt:\t14348 KiB\ndrm-engine-gfx:\t167661951 ns\n";
        let c = parse_client(amd).unwrap();
        assert_eq!(c.vram, 53748 * 1024);
        assert_eq!(c.gtt, 14348 * 1024);
        assert!(matches!(c.engines[0], (ref n, Counter::Ns(167661951), 1) if n == "gfx"));

        let xe =
            "drm-driver:\txe\ndrm-client-id:\t9\ndrm-pdev:\t0000:00:02.0\ndrm-cycles-rcs:\t100\ndrm-total-cycles-rcs:\t1000\ndrm-engine-capacity-vcs:\t2\n";
        let c = parse_client(xe).unwrap();
        assert!(matches!(c.engines[0], (ref n, Counter::Cycles(100, 1000), 1) if n == "rcs"));
    }
}
