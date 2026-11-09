//! Arranque con la sesión: un .desktop en ~/.config/autostart (estándar freedesktop).

use std::path::PathBuf;

/// Argumento con el que se arranca al iniciar sesión.
pub const ARG: &str = "--autostart";

fn dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("autostart"))
}

fn path() -> Option<PathBuf> {
    dir().map(|d| d.join("neontop.desktop"))
}

/// La versión Tauri (≤ 0.13) creaba `NeonTop.desktop`; si sigue ahí, NeonTop arranca dos veces.
/// Se quita y, si estaba activo, queda solo el nuevo.
pub fn migrate_legacy() {
    if let Some(d) = dir() {
        migrate_in(&d);
    }
}

fn migrate_in(dir: &std::path::Path) -> bool {
    let old = dir.join("NeonTop.desktop");
    let ours = std::fs::read_to_string(&old).is_ok_and(|s| s.lines().any(|l| l.starts_with("Exec=") && l.contains("neontop")));
    if !ours || std::fs::remove_file(&old).is_err() {
        return false;
    }
    let new = dir.join("neontop.desktop");
    if !new.exists() {
        let _ = std::fs::write(&new, desktop_entry("neontop"));
    }
    true
}

fn desktop_entry(exe: &str) -> String {
    format!("[Desktop Entry]\nType=Application\nName=NeonTop\nExec={exe} {ARG}\nIcon=neontop\nX-GNOME-Autostart-enabled=true\n")
}

pub fn enabled() -> bool {
    path().is_some_and(|p| p.exists())
}

pub fn set(on: bool) -> Result<(), String> {
    let p = path().ok_or(neontop_core::t!("sin carpeta de configuración"))?;
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = if exe.starts_with("/usr") { "neontop".into() } else { exe.display().to_string() };
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&p, desktop_entry(&exe)).map_err(|e| e.to_string())
    } else {
        match std::fs::remove_file(&p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_tauri_entry_is_replaced_once() {
        let d = std::env::temp_dir().join(format!("neontop-autostart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("NeonTop.desktop"), "[Desktop Entry]\nType=Application\nName=NeonTop\nExec=/usr/bin/neontop --autostart\n").unwrap();
        assert!(migrate_in(&d));
        assert!(!d.join("NeonTop.desktop").exists());
        assert!(std::fs::read_to_string(d.join("neontop.desktop")).unwrap().contains("Exec=neontop --autostart"));
        assert!(!migrate_in(&d), "la segunda vez no hay nada que hacer");
        // un NeonTop.desktop que no es nuestro no se toca
        std::fs::write(d.join("NeonTop.desktop"), "[Desktop Entry]\nExec=otra-cosa\n").unwrap();
        assert!(!migrate_in(&d));
        assert!(d.join("NeonTop.desktop").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
