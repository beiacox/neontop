//! Icono en la bandeja del sistema: línea de estado con las métricas principales,
//! mostrar/ocultar la ventana, pausar el muestreo y salir.
//!
//! En Linux la bandeja usa StatusNotifierItem (libayatana-appindicator): no hay eventos de
//! clic sobre el icono ni tooltip, así que todo va en el menú.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::sampler::Snapshot;

/// Elementos del menú que se actualizan en vivo.
pub struct TrayState {
    status: MenuItem<Wry>,
    pause: MenuItem<Wry>,
}

/// Cerrar la ventana la oculta en la bandeja en lugar de salir.
pub struct CloseToTray(pub AtomicBool);

pub fn toggle_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
    }
}

pub fn set_paused_label(app: &AppHandle, paused: bool) {
    if let Some(t) = app.try_state::<TrayState>() {
        let _ = t.pause.set_text(if paused { "▶ Reanudar muestreo" } else { "⏸ Pausar muestreo" });
    }
}

pub fn create(app: &AppHandle, paused: Arc<AtomicBool>) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "NeonTop · iniciando…", false, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Mostrar / ocultar ventana", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "⏸ Pausar muestreo", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Salir de NeonTop", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&status, &PredefinedMenuItem::separator(app)?, &toggle, &pause, &PredefinedMenuItem::separator(app)?, &quit],
    )?;

    let mut builder = TrayIconBuilder::with_id("main").tooltip("NeonTop").menu(&menu).show_menu_on_left_click(true);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "toggle" => toggle_window(app),
            "pause" => {
                let now = !paused.load(Ordering::Relaxed);
                paused.store(now, Ordering::Relaxed);
                set_paused_label(app, now);
                // la interfaz refleja el cambio (botón de pausa, etiqueta "EN PAUSA")
                let _ = app.emit("paused-changed", now);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(TrayState { status, pause });
    Ok(())
}

/// Actualiza la línea de estado del menú con la última muestra.
pub fn update(app: &AppHandle, s: &Snapshot) {
    let Some(t) = app.try_state::<TrayState>() else { return };
    let m = &s.mem;
    let cache = (m.cached + m.s_reclaimable).saturating_sub(m.shmem);
    let used = m.total.saturating_sub(m.free + m.buffers + cache);
    let mut text = format!("CPU {:.0} % · RAM {:.0} %", s.cpu.total.usage, used as f64 / m.total.max(1) as f64 * 100.0);
    if let Some(busy) = s.gpus.first().and_then(|g| g.busy) {
        text.push_str(&format!(" · GPU {busy:.0} %"));
    }
    if let Some(temp) = s.cpu.temp {
        text.push_str(&format!(" · {temp:.0} °C"));
    }
    let _ = t.status.set_text(text);
}
