mod collectors;
mod config;
pub mod helper;
mod launchers;
mod sampler;
pub mod sessions;
mod tray;
mod web;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State, WindowEvent};

use config::ConfigStore;

struct AppState {
    interval_ms: Arc<AtomicU64>,
    paused: Arc<AtomicBool>,
    collect: Arc<sampler::Collect>,
    cfg: Arc<ConfigStore>,
}

/// Argumento con el que el sistema arranca NeonTop al iniciar sesión.
const AUTOSTART_ARG: &str = "--autostart";

/// Trae al frente la ventana (desde la bandeja o minimizada).
fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[tauri::command]
fn get_system_info() -> collectors::system::SystemInfo {
    collectors::system::info()
}

#[tauri::command]
fn set_interval(state: State<AppState>, ms: u64) {
    state.interval_ms.store(ms.clamp(100, 60_000), Ordering::Relaxed);
}

#[tauri::command]
fn set_paused(app: AppHandle, state: State<AppState>, paused: bool) {
    state.paused.store(paused, Ordering::Relaxed);
    tray::set_paused_label(&app, paused);
}

#[tauri::command]
fn set_close_to_tray(state: State<tray::CloseToTray>, enabled: bool) {
    state.0.store(enabled, Ordering::Relaxed);
}

/// Guarda un fichero de texto en la ruta elegida por el usuario en el diálogo de guardado.
#[tauri::command]
fn write_text_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, contents).map_err(|e| format!("No se pudo guardar {path}: {e}"))
}

/// Activa o desactiva una recogida cara ("procs" | "gpu_procs").
#[tauri::command]
fn set_collect(state: State<AppState>, kind: String, enabled: bool) -> Result<(), String> {
    state.collect.set(&kind, "desktop", enabled, false)
}

/// Si la acción falla por permisos y el usuario activó las acciones de administrador,
/// se repite como root con pkexec (el sistema pide la contraseña).
async fn with_elevation(direct: Result<(), String>, elevate: bool, args: Vec<String>) -> Result<(), String> {
    match direct {
        Err(e) if elevate && (e.contains("no permitida") || e.contains("Permiso denegado")) => {
            tauri::async_runtime::spawn_blocking(move || helper::elevate(&args.iter().map(String::as_str).collect::<Vec<_>>()))
                .await
                .map_err(|e| e.to_string())?
        }
        other => other,
    }
}

#[tauri::command]
async fn send_signal(pid: u32, signal: String, elevate: Option<bool>) -> Result<(), String> {
    let direct = collectors::processes::send_signal(pid, &signal);
    with_elevation(direct, elevate.unwrap_or(false), vec!["signal".into(), pid.to_string(), signal]).await
}

#[tauri::command]
async fn renice_process(pid: u32, nice: i32, elevate: Option<bool>) -> Result<(), String> {
    let direct = collectors::processes::renice(pid, nice);
    with_elevation(direct, elevate.unwrap_or(false), vec!["renice".into(), pid.to_string(), nice.to_string()]).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AppConfig {
    #[serde(flatten)]
    config: config::Config,
    web: web::WebStatus,
    local_url: String,
}

fn app_config(state: &AppState, web: &web::Web) -> AppConfig {
    let config = state.cfg.get();
    AppConfig { local_url: web::local_url(config.web_port), config, web: web.status() }
}

fn web_ctx(app: &AppHandle, state: &AppState) -> web::Ctx {
    web::Ctx { app: app.clone(), cfg: state.cfg.clone(), interval_ms: state.interval_ms.clone(), paused: state.paused.clone(), collect: state.collect.clone() }
}

#[tauri::command]
fn get_app_config(state: State<AppState>, web: State<web::SharedWeb>) -> AppConfig {
    app_config(&state, &web)
}

/// Activa/desactiva el servidor web y cambia su puerto o permisos.
#[tauri::command]
fn set_web(app: AppHandle, state: State<AppState>, web: State<web::SharedWeb>, enabled: bool, port: u16, actions: bool) -> AppConfig {
    state.cfg.update(|c| {
        c.web_enabled = enabled;
        c.web_port = port.max(1024);
        c.web_actions = actions;
    });
    web.apply(web_ctx(&app, &state));
    app_config(&state, &web)
}

/// Genera una clave web nueva (los enlaces anteriores dejan de funcionar).
#[tauri::command]
fn web_new_token(state: State<AppState>, web: State<web::SharedWeb>) -> AppConfig {
    state.cfg.update(|c| c.web_token = config::new_token());
    app_config(&state, &web)
}

#[tauri::command]
fn set_start_hidden(state: State<AppState>, web: State<web::SharedWeb>, hidden: bool) -> AppConfig {
    state.cfg.update(|c| c.start_hidden = hidden);
    app_config(&state, &web)
}

/// Estado de la medición automática en los lanzadores de juegos.
#[tauri::command]
async fn fps_auto_status(state: State<'_, AppState>) -> Result<launchers::FpsAuto, String> {
    let cfg = state.cfg.clone();
    tauri::async_runtime::spawn_blocking(move || launchers::status(&cfg)).await.map_err(|e| e.to_string())
}

/// Activa o desactiva la medición automática. Al desactivarla se quita `neontop-fps` de los
/// lanzadores (los abiertos, cuando se cierren).
#[tauri::command]
async fn set_fps_auto(state: State<'_, AppState>, enabled: bool) -> Result<launchers::FpsAuto, String> {
    state.cfg.update(|c| {
        c.fps_auto = enabled;
        c.fps_auto_cleanup = !enabled;
    });
    let cfg = state.cfg.clone();
    tauri::async_runtime::spawn_blocking(move || launchers::apply(&cfg)).await.map_err(|e| e.to_string())
}

/// Cierra Steam, aplica los cambios y lo vuelve a abrir.
#[tauri::command]
async fn fps_auto_restart_steam(state: State<'_, AppState>) -> Result<launchers::FpsAuto, String> {
    let cfg = state.cfg.clone();
    tauri::async_runtime::spawn_blocking(move || launchers::restart_steam(&cfg)).await.map_err(|e| e.to_string())?
}

/// Test de RAM (~1–2 s): se ejecuta fuera del hilo principal para no congelar la UI.
#[tauri::command]
async fn bench_memory() -> Result<collectors::membench::MemBench, String> {
    tauri::async_runtime::spawn_blocking(collectors::membench::run).await.map_err(|e| e.to_string())
}

/// Da (o quita) permiso de lectura a los contadores RAPL mediante el ayudante con pkexec.
/// `persistent`: además instala la regla udev para que se mantenga tras reiniciar.
#[tauri::command]
async fn rapl_access(state: State<'_, AppState>, enable: bool, persistent: bool) -> Result<(), String> {
    let args: Vec<&'static str> = if enable { vec!["rapl-grant", if persistent { "1" } else { "0" }] } else { vec!["rapl-revoke"] };
    let res = tauri::async_runtime::spawn_blocking(move || helper::elevate(&args)).await.map_err(|e| e.to_string())?;
    state.collect.rescan_energy.store(true, Ordering::Relaxed);
    res
}

/// Activa o desactiva el tráfico de RAM en vivo: carga el módulo de contadores (AMD), lo deja
/// para cada arranque y da CAP_PERFMON al lector `neontop-membw` (pide contraseña con polkit).
#[tauri::command]
async fn membw_access(state: State<'_, AppState>, enable: bool) -> Result<(), String> {
    let res = tauri::async_runtime::spawn_blocking(move || helper::elevate(&[if enable { "membw-grant" } else { "membw-revoke" }]))
        .await
        .map_err(|e| e.to_string())?;
    state.collect.restart_membw.store(true, Ordering::Relaxed);
    res
}

#[tauri::command]
fn sessions_list(store: State<sessions::Sessions>) -> Vec<serde_json::Value> {
    store.list()
}

#[tauri::command]
fn session_save(store: State<sessions::Sessions>, summary: serde_json::Value, detail: Option<serde_json::Value>) -> Result<(), String> {
    store.save(summary, detail)
}

#[tauri::command]
fn session_load(store: State<sessions::Sessions>, id: String) -> Result<serde_json::Value, String> {
    store.load(&id)
}

#[tauri::command]
fn session_delete(store: State<sessions::Sessions>, id: String) -> Result<(), String> {
    store.delete(&id)
}

/// Pasa al disco los resúmenes que versiones anteriores guardaban en el navegador.
#[tauri::command]
fn sessions_import(store: State<sessions::Sessions>, items: Vec<serde_json::Value>) -> Result<usize, String> {
    store.import(items)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let interval_ms = Arc::new(AtomicU64::new(1000));
    let paused = Arc::new(AtomicBool::new(false));
    let collect = Arc::new(sampler::Collect::default());
    tauri::Builder::default()
        // una sola instancia: abrir NeonTop otra vez trae al frente la ventana existente
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_autostart::Builder::new().args([AUTOSTART_ARG]).build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(tray::CloseToTray(AtomicBool::new(false)))
        .setup(move |app| {
            let cfg = Arc::new(ConfigStore::load(app.path().app_config_dir().ok()));
            app.manage(sessions::Sessions::new(app.path().app_data_dir().ok()));
            // medición de FPS automática: vigila los lanzadores de juegos y aplica cuando están cerrados
            launchers::spawn(cfg.clone());
            let state = AppState { interval_ms: interval_ms.clone(), paused: paused.clone(), collect: collect.clone(), cfg: cfg.clone() };
            let web = web::init(app.handle(), web_ctx(app.handle(), &state));
            app.manage(state);
            // sin libappindicator (u otro fallo) la app sigue funcionando, solo que sin bandeja
            let tray_ok = match tray::create(app.handle(), paused.clone()) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("NeonTop: no se pudo crear el icono de bandeja: {e}");
                    false
                }
            };
            // al arrancar con el sistema puede quedarse en la bandeja (si la hay)
            let autostarted = std::env::args().any(|a| a == AUTOSTART_ARG);
            if !(autostarted && cfg.get().start_hidden && tray_ok) {
                show_main(app.handle());
            }
            sampler::spawn(app.handle().clone(), interval_ms, paused, collect, web);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let to_tray = app.state::<tray::CloseToTray>().0.load(Ordering::Relaxed);
                if to_tray && app.tray_by_id("main").is_some() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_system_info,
            set_interval,
            set_paused,
            set_close_to_tray,
            write_text_file,
            set_collect,
            send_signal,
            renice_process,
            bench_memory,
            rapl_access,
            membw_access,
            sessions_list,
            session_save,
            session_load,
            session_delete,
            sessions_import,
            get_app_config,
            set_web,
            web_new_token,
            set_start_hidden,
            fps_auto_status,
            set_fps_auto,
            fps_auto_restart_steam
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
