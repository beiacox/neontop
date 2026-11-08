//! Carpetas de NeonTop (las mismas que usaba la versión con Tauri, para conservar los datos):
//! configuración en ~/.config/com.neontop.app y datos en ~/.local/share/com.neontop.app.

use std::path::{Path, PathBuf};

pub const APP_ID: &str = "com.neontop.app";

fn xdg(var: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(fallback)))
}

pub fn config_dir() -> Option<PathBuf> {
    xdg("XDG_CONFIG_HOME", ".config").map(|d| d.join(APP_ID))
}

pub fn data_dir() -> Option<PathBuf> {
    xdg("XDG_DATA_HOME", ".local/share").map(|d| d.join(APP_ID))
}

/// Directorio de ejecución del usuario (sockets): $XDG_RUNTIME_DIR o /tmp.
pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// Lee un JSON; si no existe o está corrupto, el valor por defecto.
pub fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Escribe un JSON de forma atómica (temporal + rename): un corte a mitad no lo deja roto.
pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, serde_json::to_vec(value).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_and_fallback() {
        let dir = std::env::temp_dir().join(format!("neontop-paths-{}", std::process::id()));
        let p = dir.join("x/y.json");
        assert_eq!(read_json::<Vec<u32>>(&p), Vec::<u32>::new());
        write_json(&p, &vec![1u32, 2, 3]).unwrap();
        assert_eq!(read_json::<Vec<u32>>(&p), vec![1, 2, 3]);
        std::fs::write(&p, "{roto").unwrap();
        assert_eq!(read_json::<Vec<u32>>(&p), Vec::<u32>::new());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dirs_end_with_app_id() {
        if std::env::var_os("HOME").is_some() {
            assert!(config_dir().unwrap().ends_with(APP_ID));
            assert!(data_dir().unwrap().ends_with(APP_ID));
        }
    }
}
