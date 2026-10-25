//! Acciones de administrador. NeonTop nunca se ejecuta como root: cuando una acción lo
//! necesita, se relanza el propio binario con `pkexec neontop --helper …`. El sistema pide la
//! contraseña en su ventana de polkit (NeonTop no la ve) y, con la política que instala el
//! paquete (com.neontop.app.policy), la recuerda unos minutos.
//!
//! El modo ayudante corre como root, así que solo acepta un conjunto cerrado de órdenes con
//! argumentos validados; no ejecuta nada que venga como texto libre.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::collectors::{energy::RAPL_RULE_PATH, processes};

/// Regla udev que abre RAPL en cada arranque (también se instala en /usr/share/neontop).
pub const RAPL_RULE: &str = include_str!("../../packaging/60-neontop-rapl.rules");

fn rapl_files() -> Vec<PathBuf> {
    fs::read_dir("/sys/class/powercap")
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("intel-rapl:"))
                .map(|e| e.path().join("energy_uj"))
                .filter(|p| p.exists())
                .collect()
        })
        .unwrap_or_default()
}

fn chmod_rapl(readable: bool) -> Result<(), String> {
    for f in rapl_files() {
        let mut perm = fs::metadata(&f).map_err(|e| e.to_string())?.permissions();
        let mode = if readable { perm.mode() | 0o444 } else { perm.mode() & !0o044 };
        perm.set_mode(mode);
        fs::set_permissions(&f, perm).map_err(|e| format!("{}: {e}", f.display()))?;
    }
    Ok(())
}

/// Programa que lee los contadores de tráfico de la RAM (packaging/membw). Es lo único que
/// recibe un permiso extra: CAP_PERFMON, solo lectura de contadores de rendimiento.
pub const MEMBW_PATH: &str = "/usr/lib/neontop/neontop-membw";
/// Marca de que el usuario activó el tráfico de RAM en vivo; carga el módulo en cada arranque
/// y el paquete la usa para volver a aplicar el permiso al actualizar.
pub const MEMBW_CONF: &str = "/etc/modules-load.d/neontop-membw.conf";

fn is_amd() -> bool {
    fs::read_to_string("/proc/cpuinfo").is_ok_and(|s| s.contains("AuthenticAMD"))
}

fn exec(cmd: &str, args: &[&str]) -> Result<(), String> {
    let st = std::process::Command::new(cmd).args(args).status().map_err(|e| format!("{cmd}: {e}"))?;
    if st.success() { Ok(()) } else { Err(format!("{cmd} {} falló ({st})", args.join(" "))) }
}

fn membw_grant() -> Result<(), String> {
    if !std::path::Path::new(MEMBW_PATH).exists() {
        return Err(format!("no está instalado {MEMBW_PATH}"));
    }
    // AMD: los contadores del Data Fabric los expone el módulo amd_uncore (Intel los trae cargados)
    let amd = is_amd();
    if amd {
        exec("/usr/bin/modprobe", &["amd_uncore"])?;
    }
    let conf = if amd { "# NeonTop: contadores de tráfico de la RAM\namd_uncore\n" } else { "# NeonTop: contadores de tráfico de la RAM (Intel: sin módulos extra)\n" };
    fs::write(MEMBW_CONF, conf).map_err(|e| e.to_string())?;
    exec("/usr/bin/setcap", &["cap_perfmon=ep", MEMBW_PATH])
}

fn membw_revoke() -> Result<(), String> {
    let _ = fs::remove_file(MEMBW_CONF);
    // sin permiso el programa ya no puede leer nada; el módulo se descarga al reiniciar
    let _ = exec("/usr/bin/setcap", &["-r", MEMBW_PATH]);
    Ok(())
}

/// Punto de entrada del modo ayudante (`neontop --helper <orden> …`). Devuelve el código de salida.
pub fn run(args: &[String]) -> i32 {
    let res = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["signal", pid, sig] => pid.parse::<u32>().map_err(|e| e.to_string()).and_then(|p| processes::send_signal(p, sig)),
        ["renice", pid, nice] => match (pid.parse::<u32>(), nice.parse::<i32>()) {
            (Ok(p), Ok(n)) if p > 0 => processes::renice(p, n),
            _ => Err("argumentos no válidos".into()),
        },
        ["rapl-grant", persist] => {
            let rule = if *persist == "1" { fs::write(RAPL_RULE_PATH, RAPL_RULE).map_err(|e| e.to_string()) } else { Ok(()) };
            rule.and_then(|_| chmod_rapl(true))
        }
        ["membw-grant"] => membw_grant(),
        ["membw-revoke"] => membw_revoke(),
        ["rapl-revoke"] => {
            let _ = fs::remove_file(RAPL_RULE_PATH);
            chmod_rapl(false)
        }
        _ => Err("orden desconocida".into()),
    };
    match res {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// Ejecuta una orden del ayudante como root mediante pkexec (bloquea hasta que termina).
pub fn elevate(args: &[&str]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let out = std::process::Command::new("pkexec")
        .arg(exe)
        .arg("--helper")
        .args(args)
        .output()
        .map_err(|e| format!("No se pudo ejecutar pkexec: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(()),
        Some(126) => Err("Autorización cancelada.".into()),
        Some(127) => Err("No autorizado (¿hay un agente de polkit en el escritorio?).".into()),
        _ => {
            let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if msg.is_empty() { format!("La acción falló ({})", out.status) } else { msg })
        }
    }
}
