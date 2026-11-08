//! Icono en la bandeja del sistema (StatusNotifierItem por D-Bus, sin GTK): línea de estado con
//! las métricas principales, mostrar la ventana, pausar el muestreo y salir. Un clic en el icono
//! abre la ventana.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender;
use ksni::blocking::TrayMethods;
use neontop_core::history::mem_segments;
use neontop_core::sampler::Snapshot;

use crate::state::Msg;

struct NeonTray {
    status: String,
    paused: bool,
    tx: UnboundedSender<Msg>,
}

impl ksni::Tray for NeonTray {
    fn id(&self) -> String {
        "neontop".into()
    }
    fn title(&self) -> String {
        "NeonTop".into()
    }
    fn icon_name(&self) -> String {
        "neontop".into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![
            ksni::Icon { width: 32, height: 32, data: include_bytes!("../../../assets/icons/tray-32.argb").to_vec() },
            ksni::Icon { width: 64, height: 64, data: include_bytes!("../../../assets/icons/tray-64.argb").to_vec() },
        ]
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "NeonTop".into(), description: self.status.clone(), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.unbounded_send(Msg::Show);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem { label: self.status.clone(), enabled: false, ..Default::default() }.into(),
            MenuItem::Separator,
            StandardItem {
                label: "Mostrar NeonTop".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.unbounded_send(Msg::Show);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: if self.paused { "▶ Reanudar muestreo".into() } else { "⏸ Pausar muestreo".into() },
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.unbounded_send(Msg::TogglePause);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Salir de NeonTop".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.unbounded_send(Msg::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

static HANDLE: OnceLock<ksni::blocking::Handle<NeonTray>> = OnceLock::new();
static AVAILABLE: AtomicBool = AtomicBool::new(false);
static LAST: Mutex<Option<Instant>> = Mutex::new(None);

/// Crea el icono. Devuelve `false` si el escritorio no tiene bandeja (StatusNotifierWatcher).
pub fn create(tx: UnboundedSender<Msg>) -> bool {
    let tray = NeonTray { status: "NeonTop · iniciando…".into(), paused: false, tx };
    match tray.spawn() {
        Ok(h) => {
            let _ = HANDLE.set(h);
            AVAILABLE.store(true, Ordering::Relaxed);
            true
        }
        Err(e) => {
            eprintln!("NeonTop: sin bandeja del sistema ({e})");
            false
        }
    }
}

/// Hay icono en la bandeja (cerrar la ventana puede dejar NeonTop ahí).
pub fn available() -> bool {
    AVAILABLE.load(Ordering::Relaxed)
}

pub fn set_paused(p: bool) {
    if let Some(h) = HANDLE.get() {
        h.update(|t| t.paused = p);
    }
}

/// Línea de estado con la última muestra (como mucho cada 2 s).
pub fn update(s: &Snapshot) {
    let Some(h) = HANDLE.get() else { return };
    {
        let mut last = LAST.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        *last = Some(Instant::now());
    }
    let text = status_line(s);
    h.update(|t| t.status = text);
}

pub fn status_line(s: &Snapshot) -> String {
    let used = mem_segments(&s.mem).used;
    let mut text = format!("CPU {:.0} % · RAM {:.0} %", s.cpu.total.usage, used as f64 / s.mem.total.max(1) as f64 * 100.0);
    if let Some(busy) = s.gpus.first().and_then(|g| g.busy) {
        text.push_str(&format!(" · GPU {busy:.0} %"));
    }
    if let Some(temp) = s.cpu.temp {
        text.push_str(&format!(" · {temp:.0} °C"));
    }
    text
}

#[cfg(test)]
mod tests {
    #[test]
    fn status() {
        let mut s = neontop_core::sampler::Snapshot::default();
        s.cpu.total.usage = 12.4;
        s.mem.total = 100;
        s.mem.free = 50;
        s.cpu.temp = Some(55.0);
        assert_eq!(super::status_line(&s), "CPU 12 % · RAM 50 % · 55 °C");
    }
}
