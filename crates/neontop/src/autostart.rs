//! Arranque con la sesión: un .desktop en ~/.config/autostart (estándar freedesktop).

use std::path::PathBuf;

/// Argumento con el que se arranca al iniciar sesión.
pub const ARG: &str = "--autostart";

fn path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("autostart/neontop.desktop"))
}

pub fn enabled() -> bool {
    path().is_some_and(|p| p.exists())
}

pub fn set(on: bool) -> Result<(), String> {
    let p = path().ok_or("sin carpeta de configuración")?;
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = if exe.starts_with("/usr") { "neontop".into() } else { exe.display().to_string() };
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&p, format!("[Desktop Entry]\nType=Application\nName=NeonTop\nExec={exe} {ARG}\nIcon=neontop\nX-GNOME-Autostart-enabled=true\n")).map_err(|e| e.to_string())
    } else {
        match std::fs::remove_file(&p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}
