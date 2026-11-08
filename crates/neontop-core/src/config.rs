//! Configuración que necesita el backend antes de que cargue la interfaz (servidor web,
//! arranque oculto). Se guarda en ~/.config/com.neontop.app/config.json.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;

pub const DEFAULT_PORT: u16 = 7340;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// Servidor web para abrir NeonTop desde el teléfono u otro equipo de la red.
    pub web_enabled: bool,
    pub web_port: u16,
    /// Clave de acceso de la interfaz web (va en el enlace / código QR).
    pub web_token: String,
    /// Desde la web se puede pausar, cambiar el muestreo y enviar señales a procesos.
    pub web_actions: bool,
    /// Al arrancar con el sistema, quedarse en la bandeja sin abrir la ventana.
    pub start_hidden: bool,
    /// Añadir `neontop-fps` a los lanzadores de juegos (Steam, Heroic, Lutris, Prism) para medir
    /// los FPS sin configurar nada a mano.
    pub fps_auto: bool,
    /// Se desactivó lo anterior: quitar `neontop-fps` cuando cada lanzador esté cerrado.
    pub fps_auto_cleanup: bool,
    /// Ajustes de la interfaz (tema, muestreo, ventana…).
    pub ui: UiSettings,
}

/// Ajustes de la ventana. Antes se guardaban en el almacenamiento del navegador.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UiSettings {
    /// Id del tema o "sistema" (sigue el modo claro/oscuro del escritorio).
    pub theme: String,
    pub interval_ms: u64,
    /// Ventana de las gráficas en segundos; `None` = ilimitada.
    pub window_sec: Option<u64>,
    pub scanlines: bool,
    pub glow: bool,
    /// Última pestaña abierta.
    pub tab: String,
    /// Cerrar la ventana la deja en la bandeja del sistema.
    pub close_to_tray: bool,
    /// Repetir como root (con contraseña, vía polkit) las acciones que lo necesiten.
    pub admin_actions: bool,
    /// Precio de la electricidad por kWh y símbolo de moneda.
    pub price: f64,
    pub currency: String,
}

impl Default for UiSettings {
    fn default() -> Self {
        UiSettings {
            theme: crate::theme::DEFAULT_THEME.into(),
            interval_ms: 1000,
            window_sec: Some(120),
            scanlines: true,
            glow: true,
            tab: "overview".into(),
            close_to_tray: false,
            admin_actions: false,
            price: 0.15,
            currency: "$".into(),
        }
    }
}

/// Muestreos que ofrece la interfaz (ms).
pub const INTERVALS: [u64; 5] = [250, 500, 1000, 2000, 5000];
/// Ventanas de las gráficas (s); `None` = ilimitada.
pub const WINDOWS: [Option<u64>; 7] = [Some(60), Some(120), Some(300), Some(900), Some(3600), Some(6 * 3600), None];

impl Default for Config {
    fn default() -> Self {
        Config { web_enabled: false, web_port: DEFAULT_PORT, web_token: new_token(), web_actions: false, start_hidden: true, fps_auto: true, fps_auto_cleanup: false, ui: UiSettings::default() }
    }
}

/// Clave aleatoria de 24 caracteres (sin caracteres ambiguos).
pub fn new_token() -> String {
    const ALPHA: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut buf = [0u8; 24];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).is_err() {
        // sin /dev/urandom (no debería pasar en Linux): hora + pid, peor pero no vacío
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) ^ std::process::id() as u128;
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (seed >> ((i * 5) % 120)) as u8;
        }
    }
    buf.iter().map(|b| ALPHA[*b as usize % ALPHA.len()] as char).collect()
}

pub struct ConfigStore {
    path: Option<PathBuf>,
    pub data: Mutex<Config>,
}

impl ConfigStore {
    pub fn load(dir: Option<PathBuf>) -> Self {
        let path = dir.map(|d| d.join("config.json"));
        let data = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Config>(&s).ok())
            .unwrap_or_default();
        let store = ConfigStore { path, data: Mutex::new(data) };
        // la primera vez se guarda para fijar la clave generada
        store.save();
        store
    }

    pub fn get(&self) -> Config {
        self.data.lock().unwrap().clone()
    }

    pub fn update(&self, f: impl FnOnce(&mut Config)) -> Config {
        let c = {
            let mut d = self.data.lock().unwrap();
            f(&mut d);
            d.clone()
        };
        self.save();
        c
    }

    fn save(&self) {
        let Some(p) = &self.path else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let json = serde_json::to_string_pretty(&*self.data.lock().unwrap()).unwrap_or_default();
        // la clave web da acceso a los datos del equipo: solo legible por el usuario
        use std::os::unix::fs::OpenOptionsExt;
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(p) {
            use std::io::Write;
            let _ = f.write_all(json.as_bytes());
        }
    }
}
