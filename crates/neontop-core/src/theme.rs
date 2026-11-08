//! Temas. Un tema tiene dos partes:
//!  - `colors`: la paleta. Los nombres de los "huecos" vienen del tema Neón original
//!    (cyan = primario, magenta = secundario…); cada tema los rellena con los suyos.
//!  - `style`: el carácter visual (tipografías, esquinas, fondo, brillo, líneas CRT…).
//!
//! Los colores son `0xRRGGBB`; la interfaz los convierte a su tipo de color.

#[path = "theme_data.rs"]
mod data;
pub use data::THEMES;

/// Pseudo-tema que sigue el modo claro/oscuro del escritorio.
pub const SYSTEM_THEME: &str = "sistema";
pub const DEFAULT_THEME: &str = "neon";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub bg: u32,
    pub panel: u32,
    /// Relleno neutro (p. ej. memoria libre).
    pub dim: u32,
    pub cyan: u32,
    pub magenta: u32,
    pub lime: u32,
    pub amber: u32,
    pub violet: u32,
    pub red: u32,
    pub blue: u32,
    pub teal: u32,
    pub orange: u32,
    pub text: u32,
    pub muted: u32,
}

/// Nombre de un color de la paleta (para guardarlo en datos: "lime", "amber"…).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Cyan,
    Magenta,
    Lime,
    Amber,
    Violet,
    Red,
    Blue,
    Teal,
    Orange,
    Text,
    Muted,
    Dim,
}

impl Palette {
    pub fn slot(&self, s: Slot) -> u32 {
        match s {
            Slot::Cyan => self.cyan,
            Slot::Magenta => self.magenta,
            Slot::Lime => self.lime,
            Slot::Amber => self.amber,
            Slot::Violet => self.violet,
            Slot::Red => self.red,
            Slot::Blue => self.blue,
            Slot::Teal => self.teal,
            Slot::Orange => self.orange,
            Slot::Text => self.text,
            Slot::Muted => self.muted,
            Slot::Dim => self.dim,
        }
    }
}

/// Forma de los paneles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corners {
    /// Esquina cortada en diagonal (HUD).
    Cut,
    Round,
    Square,
}

/// Fondo de la ventana.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    Grid,
    Plain,
    Aurora,
    Dots,
    Paper,
    Lines,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Clasico,
    Neon,
    Terminal,
    Editor,
    Tematico,
}

pub const CATEGORIES: [(Category, &str); 5] = [
    (Category::Clasico, "Clásicos"),
    (Category::Neon, "Neón"),
    (Category::Terminal, "Terminal y retro"),
    (Category::Editor, "De editores de código"),
    (Category::Tematico, "Temáticos"),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// Fuente de títulos y cifras grandes.
    pub display: &'static str,
    /// Fuente general de la interfaz.
    pub body: &'static str,
    /// Fuente de ejes, tablas y valores numéricos.
    pub mono: &'static str,
    pub corners: Corners,
    /// Radio de los paneles (px) cuando `corners` es `Round`.
    pub radius: f32,
    pub backdrop: Backdrop,
    /// El tema usa brillo neón (el usuario puede apagarlo en Ajustes).
    pub glow: bool,
    /// El tema usa líneas de escaneo CRT.
    pub scanlines: bool,
    /// Títulos en mayúsculas espaciadas (estilo HUD) o normales.
    pub upper_titles: bool,
    /// Opacidad del fondo de los paneles.
    pub panel_alpha: f32,
    /// Intensidad del borde de los paneles (fracción del color de acento).
    pub border: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    /// Tema claro (fondo luminoso).
    pub light: bool,
    pub colors: Palette,
    /// 6 paradas del mapa de calor, de 0 % a 100 %.
    pub heat: [u32; 6],
    pub style: Style,
}

impl Theme {
    /// Radio efectivo de los paneles.
    pub fn radius(&self) -> f32 {
        if self.style.corners == Corners::Round { self.style.radius } else { 0.0 }
    }

    /// Color de la rejilla de las gráficas: del texto en temas claros, del primario en oscuros.
    pub fn grid(&self) -> (u32, f32) {
        if self.light { (self.colors.text, 0.08) } else { (self.colors.cyan, 0.07) }
    }

    /// Color del mapa de calor para un porcentaje 0–100 (interpolado entre las 6 paradas).
    pub fn heat_color(&self, pct: f32) -> u32 {
        const POS: [f32; 6] = [0.0, 0.15, 0.4, 0.65, 0.85, 1.0];
        let t = (pct / 100.0).clamp(0.0, 1.0);
        let mut k = 0;
        while k < 4 && t > POS[k + 1] {
            k += 1;
        }
        let f = (t - POS[k]) / (POS[k + 1] - POS[k]);
        mix(self.heat[k], self.heat[k + 1], f)
    }

    /// Color según nivel de carga: primario → lima → ámbar → secundario → rojo.
    pub fn load_color(&self, pct: f32) -> u32 {
        let c = &self.colors;
        match pct {
            p if p < 30.0 => c.cyan,
            p if p < 55.0 => c.lime,
            p if p < 75.0 => c.amber,
            p if p < 90.0 => c.magenta,
            _ => c.red,
        }
    }

    /// Color según temperatura (°C).
    pub fn temp_color(&self, celsius: f32) -> u32 {
        let c = &self.colors;
        match celsius {
            t if t < 50.0 => c.cyan,
            t if t < 65.0 => c.lime,
            t if t < 78.0 => c.amber,
            t if t < 88.0 => c.magenta,
            _ => c.red,
        }
    }
}

/// Mezcla lineal de dos colores `0xRRGGBB` (`f` = 0 → `a`, 1 → `b`).
pub fn mix(a: u32, b: u32, f: f32) -> u32 {
    let f = f.clamp(0.0, 1.0);
    let ch = |c: u32, s: u32| ((c >> s) & 0xff) as f32;
    let m = |s: u32| (ch(a, s) + (ch(b, s) - ch(a, s)) * f).round() as u32;
    (m(16) << 16) | (m(8) << 8) | m(0)
}

/// Tema por id (con "sistema" resuelto según el escritorio); si no existe, Neón.
pub fn resolve(id: &str, prefers_dark: bool) -> &'static Theme {
    let id = if id == SYSTEM_THEME { if prefers_dark { "oscuro" } else { "claro" } } else { id };
    find(id).unwrap_or_else(|| find(DEFAULT_THEME).expect("el tema Neón existe"))
}

pub fn find(id: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|t| t.id == id)
}

/// ¿El escritorio usa modo oscuro? (freedesktop: org.freedesktop.appearance color-scheme;
/// si no se puede saber, se mira el tema GTK y por defecto oscuro).
pub fn system_prefers_dark() -> bool {
    if let Ok(out) = std::process::Command::new("gdbus")
        .args(["call", "--session", "--dest", "org.freedesktop.portal.Desktop", "--object-path", "/org/freedesktop/portal/desktop", "--method", "org.freedesktop.portal.Settings.ReadOne", "org.freedesktop.appearance", "color-scheme"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        // 1 = prefiere oscuro, 2 = prefiere claro, 0 = sin preferencia
        if s.contains("uint32 1") {
            return true;
        }
        if s.contains("uint32 2") {
            return false;
        }
    }
    std::env::var("GTK_THEME").map(|t| !t.to_lowercase().contains("light")).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_is_consistent() {
        assert_eq!(THEMES.len(), 24);
        let mut ids: Vec<_> = THEMES.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), THEMES.len(), "ids únicos");
        for (cat, _) in CATEGORIES {
            assert!(THEMES.iter().any(|t| t.category == cat), "cada categoría tiene temas");
        }
        let fonts = ["Orbitron", "JetBrains Mono", "Inter", "Space Grotesk Light", "VT323", "Lora", "IBM Plex Mono"];
        for t in THEMES {
            for f in [t.style.display, t.style.body, t.style.mono] {
                assert!(fonts.contains(&f), "{}: fuente {f} incluida", t.id);
            }
            assert!((0.0..=1.0).contains(&t.style.border));
            assert!((0.0..=1.0).contains(&t.style.panel_alpha));
        }
    }

    #[test]
    fn resolve_falls_back() {
        assert_eq!(resolve("sistema", true).id, "oscuro");
        assert_eq!(resolve("sistema", false).id, "claro");
        assert_eq!(resolve("no-existe", true).id, "neon");
        assert_eq!(resolve("dracula", true).name, "Dracula");
    }

    #[test]
    fn heat_scale_endpoints() {
        let t = resolve("neon", true);
        assert_eq!(t.heat_color(0.0), t.heat[0]);
        assert_eq!(t.heat_color(100.0), t.heat[5]);
        assert_eq!(t.heat_color(15.0), t.heat[1]);
        assert_eq!(t.heat_color(-5.0), t.heat[0]);
        assert_eq!(mix(0x000000, 0xffffff, 0.5), 0x808080);
    }

    #[test]
    fn load_and_temp_colors() {
        let t = resolve("neon", true);
        assert_eq!(t.load_color(10.0), t.colors.cyan);
        assert_eq!(t.load_color(95.0), t.colors.red);
        assert_eq!(t.temp_color(40.0), t.colors.cyan);
        assert_eq!(t.temp_color(70.0), t.colors.amber);
        assert_eq!(t.radius(), 0.0);
        assert!(resolve("mocha", true).radius() > 0.0);
    }
}
