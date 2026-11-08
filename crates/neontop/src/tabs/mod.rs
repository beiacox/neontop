//! Una vista por pestaña. Cada una es una función `render` que lee el estado y devuelve el
//! contenido (la ventana pone la cabecera, la barra lateral y el desplazamiento).

pub mod cpu;
pub mod disks;
pub mod energy;
pub mod games;
pub mod gpu;
pub mod memory;
pub mod network;
pub mod overview;
pub mod processes;
pub mod sensors;

use neontop_core::format as fmt;

use crate::state::AppState;

/// Serie del historial recortada a la ventana visible.
pub fn hv(st: &AppState, key: &str) -> Vec<f32> {
    st.model.history.view(key, st.window())
}

/// Instantes de la ventana visible (segundos desde epoch).
pub fn ht(st: &AppState) -> Vec<f64> {
    st.model.history.view_t(st.window())
}

/// Suma de varias series del historial (p. ej. el tráfico de todas las interfaces).
pub fn hsum(st: &AppState, keys: &[String]) -> Vec<f32> {
    let mut out: Vec<f32> = Vec::new();
    for k in keys {
        let v = hv(st, k);
        if out.is_empty() {
            out = v;
        } else {
            out.iter_mut().zip(v).for_each(|(a, b)| *a += b);
        }
    }
    out
}

// formatos para los ejes de las gráficas
pub fn y_pct(v: f32) -> String {
    format!("{v:.0}%")
}
pub fn y_bytes(v: f32) -> String {
    fmt::bytes_digits(v as f64, 1).replace(".0 ", " ")
}
pub fn y_rate(v: f32) -> String {
    format!("{}/s", y_bytes(v))
}
pub fn y_deg(v: f32) -> String {
    format!("{v:.0}°")
}
pub fn y_watts(v: f32) -> String {
    format!("{v:.0} W")
}
pub fn y_mhz(v: f32) -> String {
    if v >= 1000.0 { format!("{:.1} GHz", v / 1000.0) } else { format!("{v:.0} MHz") }
}
pub fn y_num(v: f32) -> String {
    fmt::num(v as f64)
}
pub fn y_ms(v: f32) -> String {
    format!("{v:.0} ms")
}
pub fn y_gbs(v: f32) -> String {
    fmt::gbs(v as f64, 0)
}
pub fn y_fps(v: f32) -> String {
    format!("{v:.0}")
}
