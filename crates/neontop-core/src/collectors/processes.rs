use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::time::Instant;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub cmd: String,
    pub user: String,
    pub uid: u32,
    /// R, S, D, Z, T, t, I, X…
    pub state: String,
    pub threads: u32,
    pub nice: i32,
    pub priority: i32,
    /// % de un núcleo (como `top`; puede pasar de 100).
    pub cpu: f32,
    /// RSS en bytes.
    pub mem: u64,
    pub virt: u64,
    /// Bytes/s de E/S real a disco; `None` si no hay permiso para leer /proc/pid/io.
    pub read_rate: Option<f64>,
    pub write_rate: Option<f64>,
    /// Segundos de CPU acumulados (user + system).
    pub cpu_time: f64,
    /// Segundos desde el arranque del sistema en que empezó el proceso.
    pub start: f64,
    /// Hilo del kernel (kthreadd y sus hijos).
    pub kernel: bool,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProcSnapshot {
    pub procs: Vec<ProcInfo>,
}

struct Prev {
    ticks: u64,
    start: u64,
    read: Option<u64>,
    write: Option<u64>,
    cmd: String,
    name: String,
}

pub struct ProcCollector {
    prev: HashMap<u32, Prev>,
    prev_at: Instant,
    users: HashMap<u32, String>,
    clk_tck: f64,
    page_size: u64,
}

fn load_users() -> HashMap<u32, String> {
    fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?;
            let uid = f.nth(1)?.parse().ok()?;
            Some((uid, name.to_string()))
        })
        .collect()
}

fn read_io(pid: u32) -> (Option<u64>, Option<u64>) {
    let Ok(t) = fs::read_to_string(format!("/proc/{pid}/io")) else { return (None, None) };
    let get = |k: &str| t.lines().find_map(|l| l.strip_prefix(k)?.trim().parse().ok());
    (get("read_bytes:"), get("write_bytes:"))
}

/// Nombre legible: `comm` va truncado a 15 caracteres, así que si argv[0] empieza igual se usa su basename.
fn pick_name(comm: &str, cmd: &str) -> String {
    if comm.len() >= 15 {
        if let Some(arg0) = cmd.split(' ').next() {
            let base = arg0.rsplit('/').next().unwrap_or(arg0);
            if base.starts_with(comm) {
                return base.to_string();
            }
        }
    }
    comm.to_string()
}

impl ProcCollector {
    pub fn new() -> Self {
        // SAFETY: sysconf solo consulta constantes del sistema.
        let (clk, page) = unsafe { (libc::sysconf(libc::_SC_CLK_TCK), libc::sysconf(libc::_SC_PAGESIZE)) };
        ProcCollector {
            prev: HashMap::new(),
            prev_at: Instant::now(),
            users: load_users(),
            clk_tck: if clk > 0 { clk as f64 } else { 100.0 },
            page_size: if page > 0 { page as u64 } else { 4096 },
        }
    }

    pub fn sample(&mut self) -> ProcSnapshot {
        let now = Instant::now();
        let secs = now.duration_since(self.prev_at).as_secs_f64().max(1e-3);
        self.prev_at = now;

        let mut next: HashMap<u32, Prev> = HashMap::with_capacity(self.prev.len());
        let mut procs = Vec::with_capacity(self.prev.len());

        let Ok(dir) = fs::read_dir("/proc") else { return ProcSnapshot::default() };
        for entry in dir.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
            // el proceso puede terminar mientras lo leemos: cualquier fallo lo descarta
            let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
            let (Some(l), Some(r)) = (stat.find('('), stat.rfind(')')) else { continue };
            let comm = &stat[l + 1..r];
            let f: Vec<&str> = stat[r + 2..].split_whitespace().collect();
            if f.len() < 22 {
                continue;
            }
            let num = |i: usize| f[i].parse::<i64>().unwrap_or(0);
            let ticks = (num(11) + num(12)) as u64;
            let start_ticks = num(19) as u64;
            let ppid = num(1) as u32;

            let uid = entry.metadata().map(|m| m.uid()).unwrap_or(0);
            let (read, write) = read_io(pid);

            let old = self.prev.remove(&pid).filter(|p| p.start == start_ticks);
            let (cmd, name) = match &old {
                Some(p) => (p.cmd.clone(), p.name.clone()),
                None => {
                    let raw = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
                    let mut cmd = String::from_utf8_lossy(&raw).replace('\0', " ").trim().to_string();
                    if cmd.len() > 400 {
                        cmd.truncate(cmd.floor_char_boundary(400));
                    }
                    let name = pick_name(comm, &cmd);
                    if cmd.is_empty() {
                        cmd = format!("[{comm}]");
                    }
                    (cmd, name)
                }
            };

            let rate = |cur: Option<u64>, prev: Option<u64>| match (cur, prev) {
                (Some(c), Some(p)) => Some(c.saturating_sub(p) as f64 / secs),
                (Some(_), None) if old.is_some() => Some(0.0),
                _ => None,
            };
            let cpu = match &old {
                Some(p) => (ticks.saturating_sub(p.ticks) as f64 / self.clk_tck / secs * 100.0) as f32,
                None => 0.0,
            };

            procs.push(ProcInfo {
                pid,
                ppid,
                name: name.clone(),
                cmd: cmd.clone(),
                user: self.users.get(&uid).cloned().unwrap_or_else(|| uid.to_string()),
                uid,
                state: f[0].to_string(),
                threads: num(17) as u32,
                priority: num(15) as i32,
                nice: num(16) as i32,
                cpu,
                mem: num(21).max(0) as u64 * self.page_size,
                virt: num(20).max(0) as u64,
                read_rate: rate(read, old.as_ref().and_then(|p| p.read)),
                write_rate: rate(write, old.as_ref().and_then(|p| p.write)),
                cpu_time: ticks as f64 / self.clk_tck,
                start: start_ticks as f64 / self.clk_tck,
                kernel: pid == 2 || ppid == 2,
            });
            next.insert(pid, Prev { ticks, start: start_ticks, read, write, cmd, name });
        }

        self.prev = next;
        ProcSnapshot { procs }
    }
}

fn errno_msg() -> String {
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::EPERM) => "Operación no permitida (el proceso es de otro usuario o requiere root)".into(),
        Some(libc::EACCES) => "Permiso denegado (bajar el nice requiere privilegios)".into(),
        Some(libc::ESRCH) => "El proceso ya no existe".into(),
        _ => e.to_string(),
    }
}

pub fn send_signal(pid: u32, signal: &str) -> Result<(), String> {
    let sig = match signal {
        "TERM" => libc::SIGTERM,
        "KILL" => libc::SIGKILL,
        "INT" => libc::SIGINT,
        "HUP" => libc::SIGHUP,
        "STOP" => libc::SIGSTOP,
        "CONT" => libc::SIGCONT,
        other => return Err(format!("Señal desconocida: {other}")),
    };
    if pid <= 1 {
        return Err("No se permite enviar señales a init".into());
    }
    // SAFETY: kill() no toca memoria; el pid se ha validado arriba.
    if unsafe { libc::kill(pid as libc::pid_t, sig) } == 0 {
        Ok(())
    } else {
        Err(errno_msg())
    }
}

pub fn renice(pid: u32, nice: i32) -> Result<(), String> {
    let nice = nice.clamp(-20, 19);
    // SAFETY: setpriority() solo cambia la prioridad del proceso indicado.
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, nice) } == 0 {
        Ok(())
    } else {
        Err(errno_msg())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_processes() {
        let mut c = ProcCollector::new();
        c.sample();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let t = Instant::now();
        let s = c.sample();
        let took = t.elapsed();
        assert!(!s.procs.is_empty());
        assert!(s.procs.iter().any(|p| p.pid == std::process::id()));
        let mut top = s.procs.clone();
        top.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        eprintln!("{} procesos en {:?}", s.procs.len(), took);
        for p in top.iter().take(5) {
            eprintln!("{:>7} {:<20} {:<8} {:>6.1}% {:>8} KiB io={:?}", p.pid, p.name, p.user, p.cpu, p.mem / 1024, p.read_rate);
        }
    }
}
