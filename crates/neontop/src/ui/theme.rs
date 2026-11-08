//! Puente entre los temas de `neontop_core::theme` y GPUI: colores en `Hsla`, fuentes y medidas.
//! El tema activo es un `Global` (`Th::get(cx)`), así cualquier componente lo lee sin pasarlo.

use std::borrow::Cow;

use gpui::{App, Global, Hsla, Rgba, SharedString};
use neontop_core::theme::{self as core_theme, Backdrop, Corners, Slot, Theme};

/// `0xRRGGBB` → color de GPUI.
pub fn hex(c: u32) -> Hsla {
    Rgba { r: ((c >> 16) & 0xff) as f32 / 255.0, g: ((c >> 8) & 0xff) as f32 / 255.0, b: (c & 0xff) as f32 / 255.0, a: 1.0 }.into()
}

/// `0xRRGGBB` con opacidad.
pub fn hexa(c: u32, a: f32) -> Hsla {
    let mut h = hex(c);
    h.a = a.clamp(0.0, 1.0);
    h
}

/// El mismo color con otra opacidad.
pub fn fade(c: Hsla, a: f32) -> Hsla {
    Hsla { a: a.clamp(0.0, 1.0), ..c }
}

/// Tema activo con sus colores ya convertidos.
#[derive(Clone)]
pub struct Th {
    pub theme: &'static Theme,
    pub bg: Hsla,
    pub panel: Hsla,
    pub dim: Hsla,
    pub cyan: Hsla,
    pub magenta: Hsla,
    pub lime: Hsla,
    pub amber: Hsla,
    pub violet: Hsla,
    pub red: Hsla,
    pub blue: Hsla,
    pub teal: Hsla,
    pub orange: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub grid: Hsla,
    pub display: SharedString,
    pub body: SharedString,
    pub mono: SharedString,
    pub radius: f32,
    /// Efectos activos (los usa el tema y el usuario no los ha apagado).
    pub glow: bool,
    pub scanlines: bool,
}

impl Global for Th {}

impl Th {
    pub fn new(theme: &'static Theme, user_glow: bool, user_scanlines: bool) -> Self {
        let c = &theme.colors;
        let (g, ga) = theme.grid();
        Th {
            theme,
            bg: hex(c.bg),
            panel: hexa(c.panel, theme.style.panel_alpha),
            dim: hex(c.dim),
            cyan: hex(c.cyan),
            magenta: hex(c.magenta),
            lime: hex(c.lime),
            amber: hex(c.amber),
            violet: hex(c.violet),
            red: hex(c.red),
            blue: hex(c.blue),
            teal: hex(c.teal),
            orange: hex(c.orange),
            text: hex(c.text),
            muted: hex(c.muted),
            grid: hexa(g, ga),
            display: theme.style.display.into(),
            body: theme.style.body.into(),
            mono: theme.style.mono.into(),
            radius: theme.radius(),
            glow: theme.style.glow && user_glow,
            scanlines: theme.style.scanlines && user_scanlines,
        }
    }

    pub fn get(cx: &App) -> &Th {
        cx.global::<Th>()
    }

    pub fn slot(&self, s: Slot) -> Hsla {
        hex(self.theme.colors.slot(s))
    }

    pub fn light(&self) -> bool {
        self.theme.light
    }

    pub fn corners(&self) -> Corners {
        self.theme.style.corners
    }

    pub fn backdrop(&self) -> Backdrop {
        self.theme.style.backdrop
    }

    /// Borde de un panel con su color de acento.
    pub fn border(&self, accent: Hsla) -> Hsla {
        fade(accent, 0.15 + self.theme.style.border * 0.6)
    }

    pub fn upper(&self) -> bool {
        self.theme.style.upper_titles
    }

    /// Título con el estilo del tema (mayúsculas en los temas HUD).
    pub fn title(&self, s: &str) -> String {
        if self.upper() { s.to_uppercase() } else { s.to_string() }
    }

    pub fn load_color(&self, pct: f32) -> Hsla {
        hex(self.theme.load_color(pct))
    }

    pub fn temp_color(&self, c: f32) -> Hsla {
        hex(self.theme.temp_color(c))
    }

    pub fn heat(&self, pct: f32) -> Hsla {
        hex(self.theme.heat_color(pct))
    }

    /// Paleta para series sin color propio (núcleos, interfaces…).
    pub fn series(&self, i: usize) -> Hsla {
        let p = [self.cyan, self.magenta, self.lime, self.amber, self.violet, self.teal, self.orange, self.blue, self.red];
        p[i % p.len()]
    }
}

/// Instala el tema `id` (o "sistema") como global.
pub fn apply(cx: &mut App, id: &str, glow: bool, scanlines: bool) {
    let dark = if id == core_theme::SYSTEM_THEME { core_theme::system_prefers_dark() } else { true };
    cx.set_global(Th::new(core_theme::resolve(id, dark), glow, scanlines));
}

/// Fuentes incluidas en el binario (no hace falta tenerlas instaladas).
pub fn load_fonts(cx: &App) {
    macro_rules! f {
        ($($n:literal),*) => { vec![$(Cow::Borrowed(&include_bytes!(concat!("../../../../assets/fonts/", $n, ".ttf"))[..])),*] };
    }
    let fonts = f!(
        "orbitron-400", "orbitron-700", "jetbrains-mono-400", "jetbrains-mono-700", "inter-400", "inter-700", "space-grotesk-400", "space-grotesk-700",
        "vt323-400", "lora-400", "lora-700", "ibm-plex-mono-400", "ibm-plex-mono-700"
    );
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("NeonTop: no se pudieron cargar las fuentes: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        let c = hex(0xff0000);
        assert!((c.h - 0.0).abs() < 1e-3 && (c.s - 1.0).abs() < 1e-3 && (c.l - 0.5).abs() < 1e-3);
        assert_eq!(hexa(0x00ff00, 0.3).a, 0.3);
        assert_eq!(fade(c, 2.0).a, 1.0);
        for t in core_theme::THEMES {
            let th = Th::new(t, true, true);
            assert!(th.panel.a > 0.0);
            assert_eq!(th.glow, t.style.glow);
            assert!(!Th::new(t, false, false).glow);
        }
    }
}
