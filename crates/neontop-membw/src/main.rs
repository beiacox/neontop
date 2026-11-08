//! neontop-membw: tráfico de la RAM en tiempo real a partir de los contadores "uncore" del
//! procesador (los que cuentan lo que pasa por el controlador de memoria).
//!
//! - AMD Zen 2 / Zen 3: PMU `amd_df` (Data Fabric), eventos DramChannelDataController0-7:
//!   cada cuenta es una transferencia de 64 bytes entre el controlador y la DRAM (lectura +
//!   escritura juntas). Requiere el módulo `amd_uncore`.
//! - Intel: PMU `uncore_imc*`, eventos `data_reads` / `data_writes` (o `data_read` /
//!   `data_write`) con su escala en sysfs.
//!
//! Leer contadores de todo el sistema exige CAP_PERFMON (o root). Este programa es lo único
//! que recibe ese permiso (`setcap cap_perfmon=ep`); NeonTop lo lanza y lee su salida:
//!
//!   primera línea:  `ok <fuente> <lectura/escritura separadas: 0|1>`  o  `err <código> <mensaje>`
//!   después, cada 250 ms:  `<ns monotónicos> <bytes leídos|-1> <bytes escritos|-1> <bytes totales>`
//!   (acumulados desde el arranque).
//!
//! Solo lee contadores; no acepta argumentos ni escribe nada fuera de su salida estándar.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const PMU_DIR: &str = "/sys/bus/event_source/devices";
const READ_FORMAT_TOTAL_TIME_ENABLED: u64 = 1;
const READ_FORMAT_TOTAL_TIME_RUNNING: u64 = 2;
const PERF_FLAG_FD_CLOEXEC: libc::c_ulong = 8;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Read,
    Write,
    Both,
}

struct Counter {
    fd: i32,
    kind: Kind,
    /// Bytes por cada unidad del contador.
    bytes_per_count: f64,
}

fn read(p: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// Lista de CPU de sysfs ("0", "0,8", "0-3").
fn cpu_list(s: &str) -> Vec<i32> {
    s.split(',')
        .flat_map(|part| match part.split_once('-') {
            Some((a, b)) => match (a.parse::<i32>(), b.parse::<i32>()) {
                (Ok(a), Ok(b)) => (a..=b).collect(),
                _ => vec![],
            },
            None => part.parse().ok().into_iter().collect(),
        })
        .collect()
}

/// Coloca `value` en los bits que describe un fichero de formato ("config:0-7,32-35").
fn place(format: &str, mut value: u64) -> Option<(usize, u64)> {
    let (field, ranges) = format.split_once(':')?;
    let idx = match field {
        "config" => 0,
        "config1" => 1,
        "config2" => 2,
        _ => return None,
    };
    let mut out = 0u64;
    for r in ranges.split(',') {
        let (lo, hi) = match r.split_once('-') {
            Some((a, b)) => (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?),
            None => {
                let b = r.parse::<u32>().ok()?;
                (b, b)
            }
        };
        let width = hi - lo + 1;
        let mask = if width >= 64 { u64::MAX } else { (1u64 << width) - 1 };
        out |= (value & mask) << lo;
        value = if width >= 64 { 0 } else { value >> width };
    }
    Some((idx, out))
}

/// Convierte "event=0x07,umask=0x38" en config/config1/config2 según los formatos del PMU.
fn encode(pmu: &Path, terms: &str) -> Option<[u64; 3]> {
    let mut cfg = [0u64; 3];
    for t in terms.split(',').filter(|t| !t.is_empty()) {
        let (name, val) = t.split_once('=').unwrap_or((t, "1"));
        let v = if let Some(h) = val.strip_prefix("0x") { u64::from_str_radix(h, 16).ok()? } else { val.parse().ok()? };
        let fmt = read(pmu.join("format").join(name))?;
        let (i, bits) = place(&fmt, v)?;
        cfg[i] |= bits;
    }
    Some(cfg)
}

fn perf_open(ty: u32, cfg: [u64; 3], cpu: i32) -> Result<i32, i32> {
    // perf_event_attr (tamaño de la versión 7, 128 bytes); solo se rellenan los campos usados
    let mut attr = [0u8; 128];
    attr[0..4].copy_from_slice(&ty.to_ne_bytes());
    attr[4..8].copy_from_slice(&128u32.to_ne_bytes());
    attr[8..16].copy_from_slice(&cfg[0].to_ne_bytes());
    attr[32..40].copy_from_slice(&(READ_FORMAT_TOTAL_TIME_ENABLED | READ_FORMAT_TOTAL_TIME_RUNNING).to_ne_bytes());
    // config1 y config2 van en los desplazamientos 56 y 64
    attr[56..64].copy_from_slice(&cfg[1].to_ne_bytes());
    attr[64..72].copy_from_slice(&cfg[2].to_ne_bytes());
    // SAFETY: attr es un perf_event_attr válido de 128 bytes; pid = -1 (todo el sistema) en `cpu`.
    let fd = unsafe { libc::syscall(libc::SYS_perf_event_open, attr.as_ptr(), -1 as libc::pid_t, cpu, -1, PERF_FLAG_FD_CLOEXEC) };
    if fd < 0 {
        Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
    } else {
        Ok(fd as i32)
    }
}

/// Valor del contador escalado por el tiempo que estuvo realmente activo (multiplexado).
fn read_counter(fd: i32) -> Option<f64> {
    let mut buf = [0u64; 3];
    // SAFETY: lectura de 24 bytes (valor, tiempo activo, tiempo contando) en un búfer de 24 bytes.
    let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, 24) };
    if n != 24 {
        return None;
    }
    let (value, enabled, running) = (buf[0] as f64, buf[1] as f64, buf[2] as f64);
    Some(if running > 0.0 && running < enabled { value * enabled / running } else { value })
}

fn cpu_is_zen2_or_zen3() -> bool {
    let info = read("/proc/cpuinfo").unwrap_or_default();
    let field = |k: &str| info.lines().find(|l| l.starts_with(k)).and_then(|l| l.split(':').nth(1)).map(|v| v.trim().to_string());
    if field("vendor_id").as_deref() != Some("AuthenticAMD") {
        return false;
    }
    let family: u32 = field("cpu family").and_then(|v| v.parse().ok()).unwrap_or(0);
    let model: u32 = field("model").and_then(|v| v.parse().ok()).unwrap_or(0);
    match family {
        0x17 => true, // Zen, Zen+, Zen 2
        // Zen 3 (Zen 4 usa otros eventos: modelos 0x10-0x1f, 0x60-0x7f, 0xa0-0xaf)
        0x19 => !matches!(model, 0x10..=0x1f | 0x60..=0x7f | 0xa0..=0xaf),
        _ => false,
    }
}

enum Setup {
    Ok(&'static str, bool, Vec<Counter>),
    Err(&'static str, String),
}

fn open_counters(pmu: &Path, specs: &[(String, Kind, f64)]) -> Result<Vec<Counter>, i32> {
    let ty: u32 = read(pmu.join("type")).and_then(|t| t.parse().ok()).ok_or(libc::ENOENT)?;
    let cpus = read(pmu.join("cpumask")).map(|c| cpu_list(&c)).filter(|c| !c.is_empty()).unwrap_or(vec![0]);
    let mut out = Vec::new();
    for (terms, kind, bytes) in specs {
        let cfg = encode(pmu, terms).ok_or(libc::EINVAL)?;
        for &cpu in &cpus {
            out.push(Counter { fd: perf_open(ty, cfg, cpu)?, kind: *kind, bytes_per_count: *bytes });
        }
    }
    Ok(out)
}

fn err_code(errno: i32) -> (&'static str, String) {
    match errno {
        libc::EACCES | libc::EPERM => ("noperm", "sin permiso para leer los contadores (falta cap_perfmon)".into()),
        libc::ENOENT | libc::ENODEV => ("nopmu", "el controlador de memoria no expone contadores".into()),
        e => ("error", format!("perf_event_open: {}", std::io::Error::from_raw_os_error(e))),
    }
}

fn setup() -> Setup {
    // AMD Zen 2 / Zen 3: Data Fabric
    let df = PathBuf::from(PMU_DIR).join("amd_df");
    if cpu_is_zen2_or_zen3() {
        if !df.exists() {
            return Setup::Err("nomodule", "falta el módulo amd_uncore".into());
        }
        // DramChannelDataController0..7: event 0x07 + 0x40·n, umask 0x38, 64 bytes por cuenta
        let specs: Vec<(String, Kind, f64)> = (0..8u64).map(|n| (format!("event={:#x},umask=0x38", 0x07 + 0x40 * n), Kind::Both, 64.0)).collect();
        return match open_counters(&df, &specs) {
            Ok(c) => Setup::Ok("amd_df", false, c),
            Err(e) => {
                let (code, msg) = err_code(e);
                Setup::Err(code, msg)
            }
        };
    }

    // Intel: controladores de memoria con eventos con nombre
    let mut specs_by_pmu = Vec::new();
    if let Ok(rd) = fs::read_dir(PMU_DIR) {
        let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("uncore_imc"))).collect();
        dirs.sort();
        for pmu in dirs {
            let mut specs = Vec::new();
            for (names, kind) in [(["data_reads", "data_read"], Kind::Read), (["data_writes", "data_write"], Kind::Write)] {
                if let Some(name) = names.iter().find(|n| pmu.join("events").join(n).exists()) {
                    let ev = pmu.join("events").join(name);
                    let terms = read(&ev).unwrap_or_default();
                    let scale: f64 = read(ev.with_extension("scale")).and_then(|s| s.parse().ok()).unwrap_or(1.0);
                    let unit = read(ev.with_extension("unit")).unwrap_or_default();
                    let mult = match unit.as_str() {
                        "MiB" => 1048576.0,
                        "KiB" => 1024.0,
                        "GiB" => 1073741824.0,
                        _ => 1.0,
                    };
                    specs.push((terms, kind, scale * mult));
                }
            }
            if !specs.is_empty() {
                specs_by_pmu.push((pmu, specs));
            }
        }
    }
    if specs_by_pmu.is_empty() {
        return Setup::Err("unsupported", "este procesador no expone contadores de tráfico de memoria compatibles".into());
    }
    let mut all = Vec::new();
    for (pmu, specs) in &specs_by_pmu {
        match open_counters(pmu, specs) {
            Ok(c) => all.extend(c),
            Err(e) => {
                let (code, msg) = err_code(e);
                return Setup::Err(code, msg);
            }
        }
    }
    Setup::Ok("intel_imc", true, all)
}

fn now_ns() -> u64 {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime solo escribe en `t`
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}

fn main() {
    // si NeonTop termina, este proceso también
    // SAFETY: prctl con PR_SET_PDEATHSIG solo configura la señal de este proceso.
    unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) };
    let mut out = std::io::stdout().lock();
    let counters = match setup() {
        Setup::Ok(source, split, c) => {
            let _ = writeln!(out, "ok {source} {}", split as u8);
            c
        }
        Setup::Err(code, msg) => {
            let _ = writeln!(out, "err {code} {msg}");
            std::process::exit(2);
        }
    };
    let _ = out.flush();
    loop {
        let (mut rd, mut wr, mut both) = (0.0f64, 0.0f64, 0.0f64);
        for c in &counters {
            let bytes = read_counter(c.fd).unwrap_or(0.0) * c.bytes_per_count;
            match c.kind {
                Kind::Read => rd += bytes,
                Kind::Write => wr += bytes,
                Kind::Both => both += bytes,
            }
        }
        let split = counters.iter().any(|c| c.kind != Kind::Both);
        let line = if split { format!("{} {rd:.0} {wr:.0} {:.0}", now_ns(), rd + wr) } else { format!("{} -1 -1 {both:.0}", now_ns()) };
        // si NeonTop cerró la tubería, se termina
        if writeln!(out, "{line}").and_then(|_| out.flush()).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn places_bits() {
        // evento 0x107 en un formato partido (Zen 3): bits 0-7 y 32-35
        assert_eq!(super::place("config:0-7,32-35", 0x107), Some((0, 0x07 | (1 << 32))));
        assert_eq!(super::place("config:8-15", 0x38), Some((0, 0x38 << 8)));
        assert_eq!(super::cpu_list("0-2,8"), vec![0, 1, 2, 8]);
    }
}
