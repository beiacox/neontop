//! Interfaz web: sirve la misma interfaz que la ventana (los ficheros van incrustados en el
//! binario) para abrir NeonTop desde el teléfono u otro equipo de la red local.
//!
//! - GET  /api/ping          → {"neontop":true,"auth":bool}; sin clave (para detectar el modo web).
//! - GET  /api/poll?since=N  → espera (≤ 25 s) a una muestra más nueva que N y la devuelve junto
//!                             con la última lista de procesos. Consulta larga en vez de SSE: con
//!                             tiny_http los flujos troceados se retienen en un búfer.
//! - POST /api/invoke/<cmd>  → subconjunto de los comandos de Tauri.
//! - resto                   → ficheros de la interfaz (index.html como respaldo).
//!
//! Todo /api salvo ping exige la clave (cabecera `Authorization: Bearer …`). Es HTTP sin cifrar:
//! pensado para la red de casa.

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::config::ConfigStore;
use crate::sampler::Collect;

/// Sin consultas en este tiempo se deja de serializar para la web.
const IDLE: Duration = Duration::from_secs(30);
const POLL_WAIT: Duration = Duration::from_secs(25);

#[derive(Default)]
struct Latest {
    seq: u64,
    metrics: Option<Arc<str>>,
    pseq: u64,
    procs: Option<Arc<str>>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebStatus {
    pub running: bool,
    pub error: Option<String>,
    /// Direcciones de la red local por las que se puede abrir.
    pub urls: Vec<String>,
    /// Clientes que han consultado en los últimos 30 s.
    pub clients: usize,
}

pub struct Web {
    latest: Mutex<Latest>,
    cv: Condvar,
    /// Último acceso de cada cliente (id elegido por el navegador).
    clients: Mutex<HashMap<String, Instant>>,
    server: Mutex<Option<Arc<Server>>>,
    status: Mutex<WebStatus>,
}

/// Lo que los comandos web necesitan del resto de la app.
#[derive(Clone)]
pub struct Ctx {
    pub app: AppHandle,
    pub cfg: Arc<ConfigStore>,
    pub interval_ms: Arc<AtomicU64>,
    pub paused: Arc<AtomicBool>,
    pub collect: Arc<Collect>,
}

impl Web {
    pub fn new() -> Self {
        Web { latest: Default::default(), cv: Condvar::new(), clients: Default::default(), server: Mutex::new(None), status: Default::default() }
    }

    /// Hay algún cliente web conectado (merece la pena serializar las muestras).
    pub fn active(&self) -> bool {
        let mut c = self.clients.lock().unwrap();
        c.retain(|_, t| t.elapsed() < IDLE);
        !c.is_empty()
    }

    pub fn publish_metrics<T: Serialize>(&self, snap: &T) {
        let Ok(s) = serde_json::to_string(snap) else { return };
        let mut l = self.latest.lock().unwrap();
        l.seq += 1;
        l.metrics = Some(s.into());
        self.cv.notify_all();
    }

    pub fn publish_procs<T: Serialize>(&self, procs: &T) {
        let Ok(s) = serde_json::to_string(procs) else { return };
        let mut l = self.latest.lock().unwrap();
        l.pseq += 1;
        l.procs = Some(s.into());
    }

    pub fn status(&self) -> WebStatus {
        let mut s = self.status.lock().unwrap().clone();
        s.clients = {
            let mut c = self.clients.lock().unwrap();
            c.retain(|_, t| t.elapsed() < IDLE);
            c.len()
        };
        s
    }

    pub fn stop(&self) {
        if let Some(srv) = self.server.lock().unwrap().take() {
            srv.unblock();
        }
        *self.status.lock().unwrap() = WebStatus::default();
    }

    /// (Re)arranca el servidor según la configuración.
    pub fn apply(self: &Arc<Self>, ctx: Ctx) {
        self.stop();
        let cfg = ctx.cfg.get();
        if !cfg.web_enabled {
            return;
        }
        match Server::http(("0.0.0.0", cfg.web_port)) {
            Ok(srv) => {
                let srv = Arc::new(srv);
                *self.server.lock().unwrap() = Some(srv.clone());
                *self.status.lock().unwrap() = WebStatus { running: true, error: None, urls: urls(cfg.web_port), clients: 0 };
                let web = self.clone();
                thread::spawn(move || {
                    for req in srv.incoming_requests() {
                        let (web, ctx) = (web.clone(), ctx.clone());
                        // un hilo por petición: las consultas largas esperan hasta 25 s
                        thread::spawn(move || web.handle(req, &ctx));
                    }
                });
            }
            Err(e) => {
                *self.status.lock().unwrap() = WebStatus { running: false, error: Some(format!("No se pudo abrir el puerto {}: {e}", cfg.web_port)), urls: vec![], clients: 0 };
            }
        }
    }

    fn handle(&self, mut req: Request, ctx: &Ctx) {
        let url = req.url().to_string();
        let (path, query) = url.split_once('?').unwrap_or((&url, ""));
        let q: HashMap<&str, &str> = query.split('&').filter_map(|kv| kv.split_once('=')).collect();
        let path = path.to_string();

        if !path.starts_with("/api/") {
            return self.serve_file(req, &path, &ctx.app);
        }
        let cfg = ctx.cfg.get();
        let authed = req
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .and_then(|h| h.value.as_str().strip_prefix("Bearer "))
            .is_some_and(|t| eq_const(t.as_bytes(), cfg.web_token.as_bytes()));

        if path == "/api/ping" {
            return respond_json(req, 200, json!({ "neontop": true, "auth": authed, "actions": cfg.web_actions }).to_string());
        }
        if !authed {
            return respond_json(req, 401, json!("Clave de acceso incorrecta").to_string());
        }
        let client = q.get("client").map(|c| c.chars().filter(|c| c.is_ascii_alphanumeric()).take(32).collect::<String>()).unwrap_or_default();
        if !client.is_empty() {
            self.clients.lock().unwrap().insert(client.clone(), Instant::now());
            ctx.collect.touch(&format!("web:{client}"));
        }

        if path == "/api/poll" && *req.method() == Method::Get {
            let since: u64 = q.get("since").and_then(|v| v.parse().ok()).unwrap_or(0);
            let psince: u64 = q.get("psince").and_then(|v| v.parse().ok()).unwrap_or(0);
            let deadline = Instant::now() + POLL_WAIT;
            let mut l = self.latest.lock().unwrap();
            while l.seq <= since || l.metrics.is_none() {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                l = self.cv.wait_timeout(l, left).unwrap().0;
            }
            let metrics = if l.seq > since { l.metrics.clone() } else { None };
            let procs = if l.pseq > psince { l.procs.clone() } else { None };
            let (seq, pseq) = (l.seq, l.pseq);
            drop(l);
            let body = format!(
                r#"{{"seq":{seq},"pseq":{pseq},"paused":{},"intervalMs":{},"metrics":{},"procs":{}}}"#,
                ctx.paused.load(Ordering::Relaxed),
                ctx.interval_ms.load(Ordering::Relaxed),
                metrics.as_deref().unwrap_or("null"),
                procs.as_deref().unwrap_or("null"),
            );
            return respond_json(req, 200, body);
        }

        if let (Some(cmd), Method::Post) = (path.strip_prefix("/api/invoke/"), req.method().clone()) {
            let mut body = String::new();
            let _ = req.as_reader().take(64 * 1024).read_to_string(&mut body);
            let args: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            return match invoke(cmd, &args, &client, cfg.web_actions, ctx) {
                Ok(v) => respond_json(req, 200, v.to_string()),
                Err(e) => respond_json(req, 400, json!(e).to_string()),
            };
        }
        respond_json(req, 404, json!("No encontrado").to_string());
    }

    fn serve_file(&self, req: Request, path: &str, app: &AppHandle) {
        let clean = path.trim_start_matches('/');
        let resolver = app.asset_resolver();
        let asset = if clean.is_empty() { None } else { resolver.get(clean.to_string()) }.or_else(|| resolver.get("index.html".into()));
        match asset {
            Some(a) => {
                let mut resp = Response::from_data(a.bytes().to_vec()).with_header(header("Content-Type", a.mime_type()));
                // los ficheros de _app llevan hash en el nombre: se pueden cachear para siempre
                if clean.starts_with("_app/immutable/") {
                    resp = resp.with_header(header("Cache-Control", "public, max-age=31536000, immutable"));
                }
                let _ = req.respond(resp);
            }
            None => {
                let _ = req.respond(Response::from_string("NeonTop: interfaz no disponible en esta compilación").with_status_code(404));
            }
        }
    }
}

/// Comandos disponibles desde la web. Las acciones que cambian algo exigen `web_actions`;
/// las que piden contraseña de administrador o escriben ficheros no están disponibles.
fn invoke(cmd: &str, a: &Value, client: &str, actions: bool, ctx: &Ctx) -> Result<Value, String> {
    let need_actions = || if actions { Ok(()) } else { Err("Las acciones desde la web están desactivadas en los ajustes del equipo.".to_string()) };
    let u = |k: &str| a.get(k).and_then(Value::as_u64);
    let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    match cmd {
        "get_system_info" => Ok(json!(crate::collectors::system::info())),
        "set_collect" => {
            let on = a.get("enabled").and_then(Value::as_bool).unwrap_or(false);
            ctx.collect.set(&s("kind"), &format!("web:{client}"), on, true)?;
            Ok(Value::Null)
        }
        "set_interval" => {
            need_actions()?;
            let ms = u("ms").unwrap_or(1000).clamp(100, 60_000);
            ctx.interval_ms.store(ms, Ordering::Relaxed);
            let _ = ctx.app.emit("interval-changed", ms);
            Ok(Value::Null)
        }
        "set_paused" => {
            need_actions()?;
            let p = a.get("paused").and_then(Value::as_bool).unwrap_or(false);
            ctx.paused.store(p, Ordering::Relaxed);
            crate::tray::set_paused_label(&ctx.app, p);
            let _ = ctx.app.emit("paused-changed", p);
            Ok(Value::Null)
        }
        "send_signal" => {
            need_actions()?;
            crate::collectors::processes::send_signal(u("pid").unwrap_or(0) as u32, &s("signal")).map(|_| Value::Null)
        }
        "renice_process" => {
            need_actions()?;
            let nice = a.get("nice").and_then(Value::as_i64).unwrap_or(0) as i32;
            crate::collectors::processes::renice(u("pid").unwrap_or(0) as u32, nice).map(|_| Value::Null)
        }
        "bench_memory" => Ok(json!(crate::collectors::membench::run())),
        // historial de partidas: consultar desde el teléfono; borrar solo con acciones permitidas
        "sessions_list" => Ok(json!(ctx.app.state::<crate::sessions::Sessions>().list())),
        "session_load" => ctx.app.state::<crate::sessions::Sessions>().load(&s("id")),
        "session_delete" => {
            need_actions()?;
            ctx.app.state::<crate::sessions::Sessions>().delete(&s("id")).map(|_| Value::Null)
        }
        other => Err(format!("«{other}» no está disponible desde la web")),
    }
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("cabecera válida")
}

fn respond_json(req: Request, code: u16, body: String) {
    let resp = Response::from_string(body)
        .with_status_code(code)
        .with_header(header("Content-Type", "application/json; charset=utf-8"))
        .with_header(header("Cache-Control", "no-store"));
    let _ = req.respond(resp);
}

/// Comparación en tiempo constante (la clave no se filtra por lo que tarda en fallar).
fn eq_const(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// URLs de la red local (IPv4, sin loopback ni interfaces virtuales de contenedores).
fn urls(port: u16) -> Vec<String> {
    let mut v: Vec<String> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback() && !i.name.starts_with("docker") && !i.name.starts_with("br-") && !i.name.starts_with("veth") && !i.name.starts_with("virbr"))
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(ip) => Some(format!("http://{ip}:{port}")),
            _ => None,
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Acceso desde el propio equipo (para probar).
pub fn local_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub type SharedWeb = Arc<Web>;

/// Arranca el servidor al iniciar la app si está activado.
pub fn init(app: &AppHandle, ctx: Ctx) -> SharedWeb {
    let web = Arc::new(Web::new());
    web.apply(ctx);
    app.manage(web.clone());
    web
}
