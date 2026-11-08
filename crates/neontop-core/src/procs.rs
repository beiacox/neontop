//! Lista de procesos con un historial corto por proceso (mini-gráficas y detalle). El hilo de
//! muestreo solo la recoge mientras alguna vista la pide: recorrer /proc entero es caro.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::collectors::processes::{ProcInfo, ProcSnapshot};
use crate::theme::Slot;

/// Muestras por proceso para las mini-gráficas y el detalle.
pub const HIST: usize = 120;

#[derive(Default, Clone, Debug)]
pub struct ProcHistory {
    /// Instante de arranque del proceso: si cambia, el pid se ha reutilizado.
    pub start: f64,
    pub cpu: VecDeque<f32>,
    pub mem: VecDeque<f32>,
    pub io: VecDeque<f32>,
}

/// Descripción de los estados de /proc/pid/stat.
pub fn state(s: &str) -> (&'static str, Slot) {
    match s {
        "R" => ("Ejecutando", Slot::Lime),
        "S" => ("Durmiendo", Slot::Cyan),
        "D" => ("Espera de disco", Slot::Amber),
        "I" => ("Inactivo (kernel)", Slot::Muted),
        "Z" => ("Zombi", Slot::Red),
        "T" | "t" => (if s == "T" { "Detenido" } else { "Trazado" }, Slot::Violet),
        "X" => ("Muerto", Slot::Red),
        _ => ("Desconocido", Slot::Muted),
    }
}

/// Señales que se pueden enviar desde la interfaz: (nombre, descripción).
pub const SIGNALS: [(&str, &str); 6] = [
    ("TERM", "Terminar"),
    ("KILL", "Matar"),
    ("INT", "Interrumpir"),
    ("HUP", "Recargar (HUP)"),
    ("STOP", "Pausar"),
    ("CONT", "Reanudar"),
];

#[derive(Default)]
pub struct Procs {
    pub list: Arc<ProcSnapshot>,
    pub hist: HashMap<u32, ProcHistory>,
    /// Se incrementa con cada lista nueva.
    pub tick: u64,
}

impl Procs {
    pub fn ingest(&mut self, snap: Arc<ProcSnapshot>) {
        let mut alive = std::collections::HashSet::with_capacity(snap.procs.len());
        for p in &snap.procs {
            alive.insert(p.pid);
            let h = self.hist.entry(p.pid).or_default();
            // pid reutilizado por otro proceso: se empieza de cero
            if h.start != p.start {
                *h = ProcHistory { start: p.start, ..Default::default() };
            }
            h.cpu.push_back(p.cpu);
            h.mem.push_back(p.mem as f32);
            h.io.push_back((p.read_rate.unwrap_or(0.0) + p.write_rate.unwrap_or(0.0)) as f32);
            if h.cpu.len() > HIST {
                h.cpu.pop_front();
                h.mem.pop_front();
                h.io.pop_front();
            }
        }
        self.hist.retain(|pid, _| alive.contains(pid));
        self.list = snap;
        self.tick += 1;
    }

    pub fn get(&self, pid: u32) -> Option<&ProcInfo> {
        self.list.procs.iter().find(|p| p.pid == pid)
    }
}

/// Columna por la que se ordena la tabla de procesos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Pid,
    Name,
    User,
    Cpu,
    Mem,
    Io,
    Threads,
    Time,
    State,
}

/// Filtra (texto en nombre, comando, usuario o pid; procesos del kernel opcionales) y ordena.
pub fn filter_sort<'a>(list: &'a [ProcInfo], query: &str, show_kernel: bool, key: SortKey, desc: bool) -> Vec<&'a ProcInfo> {
    let q = query.trim().to_lowercase();
    let mut v: Vec<&ProcInfo> = list
        .iter()
        .filter(|p| show_kernel || !p.kernel)
        .filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q) || p.cmd.to_lowercase().contains(&q) || p.user.to_lowercase().contains(&q) || p.pid.to_string() == q)
        .collect();
    let io = |p: &ProcInfo| p.read_rate.unwrap_or(0.0) + p.write_rate.unwrap_or(0.0);
    v.sort_by(|a, b| {
        let o = match key {
            SortKey::Pid => a.pid.cmp(&b.pid),
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::User => a.user.cmp(&b.user),
            SortKey::Cpu => a.cpu.total_cmp(&b.cpu),
            SortKey::Mem => a.mem.cmp(&b.mem),
            SortKey::Io => io(a).total_cmp(&io(b)),
            SortKey::Threads => a.threads.cmp(&b.threads),
            SortKey::Time => a.cpu_time.total_cmp(&b.cpu_time),
            SortKey::State => a.state.cmp(&b.state),
        };
        let o = if desc { o.reverse() } else { o };
        o.then(a.pid.cmp(&b.pid))
    });
    v
}

/// Árbol de procesos: cada proceso con su profundidad, hijos debajo del padre (por pid).
pub fn tree<'a>(list: &[&'a ProcInfo]) -> Vec<(&'a ProcInfo, usize)> {
    let pids: std::collections::HashSet<u32> = list.iter().map(|p| p.pid).collect();
    let mut children: HashMap<u32, Vec<&'a ProcInfo>> = HashMap::new();
    let mut roots = Vec::new();
    for &p in list {
        if p.ppid != p.pid && pids.contains(&p.ppid) {
            children.entry(p.ppid).or_default().push(p);
        } else {
            roots.push(p);
        }
    }
    let mut out = Vec::with_capacity(list.len());
    fn walk<'a>(p: &'a ProcInfo, depth: usize, children: &HashMap<u32, Vec<&'a ProcInfo>>, out: &mut Vec<(&'a ProcInfo, usize)>) {
        out.push((p, depth));
        if let Some(cs) = children.get(&p.pid) {
            for c in cs {
                walk(c, depth + 1, children, out);
            }
        }
    }
    for r in roots {
        walk(r, 0, &children, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, name: &str, cpu: f32, start: f64) -> ProcInfo {
        ProcInfo { pid, ppid, name: name.into(), cmd: format!("/usr/bin/{name}"), user: "yo".into(), cpu, start, ..Default::default() }
    }

    #[test]
    fn history_tracks_and_resets() {
        let mut pr = Procs::default();
        for i in 0..(HIST + 10) {
            pr.ingest(Arc::new(ProcSnapshot { procs: vec![p(1, 0, "init", i as f32, 1.0), p(2, 1, "a", 1.0, 5.0)] }));
        }
        assert_eq!(pr.hist[&1].cpu.len(), HIST);
        assert_eq!(*pr.hist[&1].cpu.back().unwrap(), (HIST + 9) as f32);
        // pid 2 reutilizado (otro arranque) y pid 1 desaparece
        pr.ingest(Arc::new(ProcSnapshot { procs: vec![p(2, 0, "b", 3.0, 9.0)] }));
        assert!(!pr.hist.contains_key(&1));
        assert_eq!(pr.hist[&2].cpu.len(), 1);
        assert_eq!(pr.get(2).unwrap().name, "b");
        assert_eq!(pr.tick, (HIST + 11) as u64);
    }

    #[test]
    fn filter_and_sort() {
        let mut k = p(3, 2, "kworker", 0.0, 0.0);
        k.kernel = true;
        let list = vec![p(1, 0, "systemd", 0.5, 0.0), p(10, 1, "firefox", 20.0, 0.0), p(11, 10, "Web Content", 5.0, 0.0), k];
        let v = filter_sort(&list, "", false, SortKey::Cpu, true);
        assert_eq!(v.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![10, 11, 1]);
        assert_eq!(filter_sort(&list, "", true, SortKey::Pid, false).len(), 4);
        assert_eq!(filter_sort(&list, "FIRE", false, SortKey::Pid, false)[0].pid, 10);
        assert_eq!(filter_sort(&list, "11", false, SortKey::Pid, false)[0].pid, 11);
        let t = tree(&filter_sort(&list, "", false, SortKey::Pid, false));
        assert_eq!(t.iter().map(|(p, d)| (p.pid, *d)).collect::<Vec<_>>(), vec![(1, 0), (10, 1), (11, 2)]);
    }

    #[test]
    fn states() {
        assert_eq!(state("R").0, "Ejecutando");
        assert_eq!(state("t").0, "Trazado");
        assert_eq!(state("?").1, Slot::Muted);
    }
}
