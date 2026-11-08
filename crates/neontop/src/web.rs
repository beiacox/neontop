//! Interfaz web para abrir NeonTop desde el teléfono u otro equipo de la red local.
//!
//! Todo se genera en el servidor (HTML + gráficas SVG hechas en Rust, sin JavaScript): la página
//! se recarga sola cada pocos segundos. Rutas:
//! - GET  /?t=<clave>&tab=…   → panel (la clave se guarda en una cookie y se quita del enlace).
//! - POST /accion             → pausar, cambiar el muestreo o enviar señales (si se permite).
//! - GET  /api/snapshot       → última muestra en JSON (integraciones, scripts).
//!
//! Es HTTP sin cifrar, pensado para la red de casa; todo exige la clave de acceso.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Read;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender;
use neontop_core::collectors::processes::ProcSnapshot;
use neontop_core::config::ConfigStore;
use neontop_core::format as fmt;
use neontop_core::history::{mem_segments, History};
use neontop_core::sampler::{Control, Snapshot};
use neontop_core::theme;
use tiny_http::{Header, Method, Request, Response, Server};

use crate::state::Msg;

/// Muestras que guarda la web para sus gráficas (≈ 5 min a 1 s).
const WEB_POINTS: usize = 300;
/// Sin peticiones en este tiempo, se dejan de pedir los procesos.
const IDLE: Duration = Duration::from_secs(30);

#[derive(Clone, Default, Debug, PartialEq)]
pub struct WebStatus {
    pub running: bool,
    pub error: Option<String>,
    /// Direcciones de la red local por las que se puede abrir.
    pub urls: Vec<String>,
    /// Clientes que han consultado en los últimos 30 s.
    pub clients: usize,
}

#[derive(Default)]
struct Data {
    snap: Option<Arc<Snapshot>>,
    procs: Option<Arc<ProcSnapshot>>,
    history: History,
}

struct Inner {
    data: Mutex<Data>,
    server: Mutex<Option<Arc<Server>>>,
    status: Mutex<WebStatus>,
    clients: Mutex<HashMap<String, Instant>>,
    cfg: Arc<ConfigStore>,
    ctl: Arc<Control>,
    tx: UnboundedSender<Msg>,
    demo: bool,
}

/// Servidor web (se puede clonar; todas las copias son el mismo servidor).
#[derive(Clone)]
pub struct Web(Arc<Inner>);

/// Lado del hilo de muestreo: guarda las muestras para la web (solo si está en marcha).
#[derive(Clone)]
pub struct Publisher(Arc<Inner>);

impl Publisher {
    pub fn metrics(&self, s: &Arc<Snapshot>) {
        if !self.0.status.lock().unwrap().running {
            return;
        }
        let mut d = self.0.data.lock().unwrap();
        d.history.ingest(s);
        d.history.end();
        if d.history.len() > WEB_POINTS * 2 {
            // la web solo enseña los últimos minutos: se empieza de nuevo con lo reciente
            d.history.clear();
        }
        d.snap = Some(s.clone());
    }
    pub fn procs(&self, p: &Arc<ProcSnapshot>) {
        if self.0.status.lock().unwrap().running {
            self.0.data.lock().unwrap().procs = Some(p.clone());
        }
    }
}

impl Web {
    pub fn new(cfg: Arc<ConfigStore>, ctl: Arc<Control>, tx: UnboundedSender<Msg>, demo: bool) -> Self {
        Web(Arc::new(Inner {
            data: Default::default(),
            server: Mutex::new(None),
            status: Default::default(),
            clients: Default::default(),
            cfg,
            ctl,
            tx,
            demo,
        }))
    }

    pub fn publisher(&self) -> Publisher {
        Publisher(self.0.clone())
    }

    pub fn status(&self) -> WebStatus {
        let mut s = self.0.status.lock().unwrap().clone();
        let mut c = self.0.clients.lock().unwrap();
        c.retain(|_, t| t.elapsed() < IDLE);
        s.clients = c.len();
        s
    }

    pub fn stop(&self) {
        if let Some(srv) = self.0.server.lock().unwrap().take() {
            srv.unblock();
        }
        *self.0.status.lock().unwrap() = WebStatus::default();
        let _ = self.0.ctl.collect.set("procs", "web", false, true);
    }

    /// (Re)arranca el servidor según la configuración.
    pub fn apply(&self) {
        self.stop();
        let cfg = self.0.cfg.get();
        if !cfg.web_enabled {
            return;
        }
        match Server::http(("0.0.0.0", cfg.web_port)) {
            Ok(srv) => {
                let srv = Arc::new(srv);
                *self.0.server.lock().unwrap() = Some(srv.clone());
                *self.0.status.lock().unwrap() = WebStatus { running: true, error: None, urls: urls(cfg.web_port), clients: 0 };
                let inner = self.0.clone();
                thread::Builder::new()
                    .name("neontop-web".into())
                    .spawn(move || {
                        for req in srv.incoming_requests() {
                            let inner = inner.clone();
                            thread::spawn(move || handle(&inner, req));
                        }
                    })
                    .ok();
            }
            Err(e) => {
                *self.0.status.lock().unwrap() = WebStatus { running: false, error: Some(format!("No se pudo abrir el puerto {}: {e}", cfg.web_port)), urls: vec![], clients: 0 };
            }
        }
    }
}

/// Direcciones IPv4 de la red local (sin la de bucle).
pub fn urls(port: u16) -> Vec<String> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback() && i.ip().is_ipv4())
        .map(|i| format!("http://{}:{port}", i.ip()))
        .collect()
}

/// Comparación en tiempo constante (la clave no se filtra por el tiempo de respuesta).
pub fn eq_const(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn query(s: &str) -> HashMap<String, String> {
    s.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), url_decode(v))).collect()
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h << 4 | l);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Escapa texto para HTML.
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn handle(inner: &Inner, mut req: Request) {
    let url = req.url().to_string();
    let (path, qs) = url.split_once('?').unwrap_or((&url, ""));
    let q = query(qs);
    let cfg = inner.cfg.get();
    let token = cfg.web_token.as_bytes();
    let cookie_ok = req
        .headers()
        .iter()
        .filter(|h| h.field.equiv("Cookie"))
        .flat_map(|h| h.value.as_str().split(';').map(str::trim).map(String::from).collect::<Vec<_>>())
        .any(|c| c.strip_prefix("neontop=").is_some_and(|v| eq_const(v.as_bytes(), token)));
    let param_ok = q.get("t").is_some_and(|t| eq_const(t.as_bytes(), token));
    if !(cookie_ok || param_ok) {
        let _ = req.respond(Response::from_string(login_page()).with_status_code(401).with_header(header("Content-Type", "text/html; charset=utf-8")));
        return;
    }
    // con la clave en el enlace: se guarda en una cookie y se redirige sin ella
    if param_ok && !cookie_ok {
        let mut rest: Vec<String> = q.iter().filter(|(k, _)| k.as_str() != "t").map(|(k, v)| format!("{k}={v}")).collect();
        rest.sort();
        let to = if rest.is_empty() { path.to_string() } else { format!("{path}?{}", rest.join("&")) };
        let _ = req.respond(
            Response::empty(303)
                .with_header(header("Location", &to))
                .with_header(header("Set-Cookie", &format!("neontop={}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000", cfg.web_token))),
        );
        return;
    }
    let client = req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    inner.clients.lock().unwrap().insert(client, Instant::now());

    match (req.method().clone(), path) {
        (Method::Get, "/api/snapshot") => {
            let body = inner.data.lock().unwrap().snap.as_ref().and_then(|s| serde_json::to_string(&**s).ok()).unwrap_or_else(|| "null".into());
            let _ = req.respond(Response::from_string(body).with_header(header("Content-Type", "application/json")));
        }
        (Method::Post, "/accion") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let f = query(&body);
            if cfg.web_actions {
                action(inner, &f);
            }
            let back = f.get("tab").map(|t| format!("/?tab={}", esc(t))).unwrap_or_else(|| "/".into());
            let _ = req.respond(Response::empty(303).with_header(header("Location", &back)));
        }
        (Method::Get, "/") => {
            let tab = q.get("tab").map(String::as_str).unwrap_or("resumen");
            if tab == "procesos" {
                let _ = inner.ctl.collect.set("procs", "web", true, true);
                inner.ctl.collect.touch("web");
            }
            let html = page(inner, tab, &cfg.ui.theme, cfg.web_actions);
            let _ = req.respond(Response::from_string(html).with_header(header("Content-Type", "text/html; charset=utf-8")).with_header(header("Cache-Control", "no-store")));
        }
        _ => {
            let _ = req.respond(Response::from_string("No encontrado").with_status_code(404));
        }
    }
}

fn action(inner: &Inner, f: &HashMap<String, String>) {
    match f.get("a").map(String::as_str) {
        Some("pausa") => {
            let _ = inner.tx.unbounded_send(Msg::TogglePause);
        }
        Some("muestreo") => {
            if let Some(ms) = f.get("ms").and_then(|v| v.parse().ok()) {
                inner.ctl.set_interval(ms);
            }
        }
        Some("senal") if !inner.demo => {
            if let (Some(pid), Some(sig)) = (f.get("pid").and_then(|v| v.parse().ok()), f.get("sig"))
                && ["TERM", "KILL", "STOP", "CONT"].contains(&sig.as_str()) {
                    let _ = neontop_core::collectors::processes::send_signal(pid, sig);
                }
        }
        _ => {}
    }
}

fn login_page() -> String {
    "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'><title>NeonTop</title>\
     <body style='background:#07060d;color:#d8e1ff;font-family:monospace;padding:2em'><h2 style='color:#00f0ff'>NeonTop</h2>\
     <p>Abre el enlace (o escanea el código QR) que aparece en Ajustes → Acceso web del equipo.</p>\
     <form><input name=t placeholder='clave de acceso' style='padding:.5em;width:16em'> <button>Entrar</button></form>"
        .into()
}

fn css(t: &theme::Theme) -> String {
    let c = &t.colors;
    let h = |x: u32| format!("#{x:06x}");
    format!(
        ":root{{--bg:{};--panel:{};--text:{};--muted:{};--cyan:{};--magenta:{};--lime:{};--amber:{};--red:{};--violet:{}}}\
         *{{box-sizing:border-box}}body{{margin:0;background:var(--bg);color:var(--text);font:13px/1.35 'JetBrains Mono',ui-monospace,monospace}}\
         header{{display:flex;flex-wrap:wrap;gap:.5em;align-items:center;padding:.7em 1em;border-bottom:1px solid var(--muted)}}\
         h1{{font-size:1.2em;margin:0 1em 0 0;color:var(--cyan)}}nav a{{color:var(--muted);margin-right:.8em;text-decoration:none}}nav a.on{{color:var(--cyan)}}\
         main{{display:grid;grid-template-columns:repeat(auto-fill,minmax(300px,1fr));gap:.8em;padding:.8em}}\
         .p{{background:var(--panel);border:1px solid color-mix(in srgb,var(--cyan) 30%,transparent);padding:.7em}}\
         .p h2{{font-size:.85em;margin:0 0 .4em;color:var(--cyan);text-transform:uppercase;letter-spacing:.12em}}\
         .big{{font-size:1.9em;font-weight:bold}}.m{{color:var(--muted)}}svg{{width:100%;height:60px;display:block}}\
         table{{width:100%;border-collapse:collapse}}td,th{{padding:2px 4px;text-align:right}}td:first-child,th:first-child{{text-align:left}}\
         .bar{{height:6px;background:color-mix(in srgb,var(--cyan) 15%,transparent)}}.bar i{{display:block;height:100%}}\
         button,select{{background:transparent;color:var(--cyan);border:1px solid var(--cyan);padding:2px 8px;font:inherit}}form{{display:inline}}",
        h(c.bg),
        h(c.panel),
        h(c.text),
        h(c.muted),
        h(c.cyan),
        h(c.magenta),
        h(c.lime),
        h(c.amber),
        h(c.red),
        h(c.violet)
    )
}

/// Mini-gráfica SVG (línea + área) de una serie, con máximo fijo o automático.
pub fn svg_spark(ys: &[f32], color: &str, max: Option<f32>) -> String {
    if ys.len() < 2 {
        return "<svg></svg>".into();
    }
    let m = max.unwrap_or_else(|| ys.iter().copied().fold(0.0, f32::max)).max(1e-6);
    let n = ys.len() - 1;
    let mut pts = String::new();
    for (i, v) in ys.iter().enumerate() {
        let x = i as f32 / n as f32 * 300.0;
        let y = 58.0 - (v / m).clamp(0.0, 1.0) * 56.0;
        let _ = write!(pts, "{x:.1},{y:.1} ");
    }
    format!(
        "<svg viewBox='0 0 300 60' preserveAspectRatio='none'><polygon points='0,60 {pts}300,60' fill='{color}' fill-opacity='.18'/><polyline points='{pts}' fill='none' stroke='{color}' stroke-width='1.5' vector-effect='non-scaling-stroke'/></svg>"
    )
}

fn bar(frac: f64, color: &str) -> String {
    format!("<div class=bar><i style='width:{:.1}%;background:{color}'></i></div>", (frac * 100.0).clamp(0.0, 100.0))
}

fn page(inner: &Inner, tab: &str, theme_id: &str, actions: bool) -> String {
    let t = theme::resolve(theme_id, true);
    let d = inner.data.lock().unwrap();
    let paused = inner.ctl.paused.load(Ordering::Relaxed);
    let interval = inner.ctl.interval_ms.load(Ordering::Relaxed);
    let mut o = String::with_capacity(16 * 1024);
    let refresh = (interval / 1000).clamp(2, 10);
    let _ = write!(
        o,
        "<!doctype html><html lang=es><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>\
         <meta http-equiv=refresh content={refresh}><title>NeonTop</title><style>{}</style><header><h1>NEONTOP</h1><nav>",
        css(t)
    );
    for (id, label) in [("resumen", "Resumen"), ("procesos", "Procesos"), ("juegos", "Juegos"), ("energia", "Energía")] {
        let _ = write!(o, "<a href='/?tab={id}' class='{}'>{label}</a>", if id == tab { "on" } else { "" });
    }
    o.push_str("</nav>");
    if actions {
        let _ = write!(
            o,
            "<form method=post action=/accion><input type=hidden name=a value=pausa><input type=hidden name=tab value='{tab}'><button>{}</button></form>\
             <form method=post action=/accion><input type=hidden name=a value=muestreo><input type=hidden name=tab value='{tab}'><select name=ms>",
            if paused { "▶ Reanudar" } else { "⏸ Pausar" }
        );
        for ms in neontop_core::config::INTERVALS {
            let _ = write!(o, "<option value={ms} {}>{}</option>", if ms == interval { "selected" } else { "" }, fmt::interval(ms));
        }
        o.push_str("</select><button>✓</button></form>");
    }
    o.push_str("</header><main>");
    let Some(s) = d.snap.clone() else {
        o.push_str("<div class=p>Esperando la primera muestra…</div></main>");
        return o;
    };
    let h = &d.history;
    let w = h.window(Some(300.0));
    let hx = |x: u32| format!("#{x:06x}");
    let c = &t.colors;
    match tab {
        "procesos" => {
            o.push_str("<div class=p style='grid-column:1/-1'><h2>Procesos</h2><table><tr><th>Nombre</th><th>PID</th><th>CPU</th><th>RAM</th>");
            if actions && !inner.demo {
                o.push_str("<th></th>");
            }
            o.push_str("</tr>");
            if let Some(p) = &d.procs {
                let list = neontop_core::procs::filter_sort(&p.procs, "", false, neontop_core::procs::SortKey::Cpu, true);
                for p in list.iter().take(60) {
                    let _ = write!(o, "<tr><td>{}</td><td>{}</td><td>{:.1}%</td><td>{}</td>", esc(&p.name), p.pid, p.cpu, fmt::bytes(p.mem as f64));
                    if actions && !inner.demo {
                        let _ = write!(
                            o,
                            "<td><form method=post action=/accion><input type=hidden name=a value=senal><input type=hidden name=tab value=procesos><input type=hidden name=pid value={}><input type=hidden name=sig value=TERM><button title='Terminar'>✕</button></form></td>",
                            p.pid
                        );
                    }
                    o.push_str("</tr>");
                }
            } else {
                o.push_str("<tr><td colspan=4 class=m>Cargando la lista de procesos…</td></tr>");
            }
            o.push_str("</table></div>");
        }
        "juegos" => {
            if s.games.games.is_empty() {
                o.push_str("<div class=p><h2>Juegos</h2><p class=m>Ningún juego medido ahora mismo.</p></div>");
            }
            for g in &s.games.games {
                let fps = h.view(&format!("game.{}.fps", g.pid), w);
                let _ = write!(
                    o,
                    "<div class=p><h2>{}</h2><div class=big style='color:{}'>{:.0} FPS</div><div class=m>{} · {} · {}</div>{}</div>",
                    esc(&g.title),
                    hx(c.lime),
                    g.fps,
                    esc(&g.exe),
                    esc(&g.api),
                    fmt::duration(g.duration),
                    svg_spark(&fps, &hx(c.lime), None)
                );
            }
        }
        "energia" => {
            let e = &s.energy;
            let _ = write!(
                o,
                "<div class=p><h2>Consumo</h2><div class=big style='color:{}'>{}</div>{}</div>",
                hx(c.amber),
                fmt::watts(Some(e.total_w)),
                svg_spark(&h.view("energy.total", w), &hx(c.amber), None)
            );
            o.push_str("<div class=p><h2>Fuentes</h2><table>");
            for src in neontop_core::energy_log::top_sources(&e.sources) {
                let _ = write!(o, "<tr><td>{}</td><td>{}</td></tr>", esc(&src.label), fmt::watts(src.watts));
            }
            o.push_str("</table></div>");
            if let Some(u) = &e.ups {
                let _ = write!(
                    o,
                    "<div class=p><h2>SAI</h2><div>{} · {}</div><div class=m>batería {:.0} % · {} min · red {:.0} V</div></div>",
                    esc(&u.model),
                    if u.on_battery { "EN BATERÍA" } else { "con red" },
                    u.battery_pct.unwrap_or(0.0),
                    u.runtime_min.map(|m| format!("{m:.0}")).unwrap_or("?".into()),
                    u.line_v.unwrap_or(0.0)
                );
            }
        }
        _ => {
            let cpu = s.cpu.total.usage;
            let _ = write!(
                o,
                "<div class=p><h2>CPU</h2><div class=big style='color:{}'>{:.0}%</div><div class=m>{} · {}</div>{}</div>",
                hx(t.load_color(cpu)),
                cpu,
                s.cpu.temp.map(|t| format!("{t:.0} °C")).unwrap_or_default(),
                fmt::freq(s.cpu.freqs.iter().sum::<f32>() as f64 / s.cpu.freqs.len().max(1) as f64),
                svg_spark(&h.view("cpu.usage", w), &hx(c.cyan), Some(100.0))
            );
            let seg = mem_segments(&s.mem);
            let used: Vec<f32> = h.view("mem.free", w).iter().map(|f| s.mem.total as f32 - f).collect();
            let _ = write!(
                o,
                "<div class=p><h2>Memoria</h2><div class=big style='color:{}'>{}</div><div class=m>swap {}</div>{}</div>",
                hx(c.magenta),
                fmt::bytes_of(seg.used as f64, s.mem.total as f64),
                fmt::bytes(s.mem.swap_total.saturating_sub(s.mem.swap_free) as f64),
                svg_spark(&used, &hx(c.magenta), Some(s.mem.total as f32))
            );
            for (i, g) in s.gpus.iter().enumerate() {
                let _ = write!(
                    o,
                    "<div class=p><h2>GPU · {}</h2><div class=big style='color:{}'>{:.0}%</div><div class=m>{} · {} · VRAM {}</div>{}</div>",
                    esc(&g.name),
                    hx(c.violet),
                    g.busy.unwrap_or(0.0),
                    g.temp_hotspot.or(g.temp_edge).map(|t| format!("{t:.0} °C")).unwrap_or_default(),
                    fmt::watts(g.power.map(f64::from)),
                    fmt::bytes_of(g.vram_used as f64, g.vram_total as f64),
                    svg_spark(&h.view(&format!("gpu.{i}.busy"), w), &hx(c.violet), Some(100.0))
                );
            }
            let (rx, tx): (f64, f64) = s.net.ifaces.iter().filter(|i| i.kind != "loopback").fold((0.0, 0.0), |a, i| (a.0 + i.rx_rate, a.1 + i.tx_rate));
            let mut rxs = vec![0f32; 0];
            for k in h.keys_with_prefix("net.").into_iter().filter(|k| k.ends_with(".rx") && !k.starts_with("net.lo.")) {
                let v = h.view(k, w);
                if rxs.is_empty() {
                    rxs = v;
                } else {
                    rxs.iter_mut().zip(v).for_each(|(a, b)| *a += b);
                }
            }
            let _ = write!(
                o,
                "<div class=p><h2>Red</h2><div class=big style='color:{}'>↓ {}</div><div class=m>↑ {}</div>{}</div>",
                hx(c.lime),
                fmt::rate(rx),
                fmt::rate(tx),
                svg_spark(&rxs, &hx(c.lime), None)
            );
            o.push_str("<div class=p><h2>Discos</h2><table>");
            for f in &s.disks.filesystems {
                let frac = f.used as f64 / f.total.max(1) as f64;
                let color = if frac > 0.9 { hx(c.red) } else { hx(c.cyan) };
                let _ = write!(o, "<tr><td>{}</td><td>{:.0}%</td></tr><tr><td colspan=2>{}</td></tr>", esc(&f.mount), frac * 100.0, bar(frac, &color));
            }
            o.push_str("</table></div>");
            let _ = write!(
                o,
                "<div class=p><h2>Sistema</h2><div>{}</div><div class=m>{} · activo {} · muestreo {}{}</div></div>",
                esc(&neontop_core::collectors::system::info().hostname),
                fmt::watts(Some(s.energy.total_w)),
                fmt::duration(s.uptime),
                fmt::interval(interval),
                if paused { " · EN PAUSA" } else { "" }
            );
        }
    }
    o.push_str("</main></html>");
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpStream;

    fn get(port: u16, path: &str, cookie: Option<&str>) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let c = cookie.map(|c| format!("Cookie: {c}\r\n")).unwrap_or_default();
        write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n{c}Connection: close\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn server_requires_key_and_renders_pages() {
        let dir = std::env::temp_dir().join(format!("neontop-web-{}", std::process::id()));
        let cfg = Arc::new(ConfigStore::load(Some(dir.clone())));
        let port = 47_000 + (std::process::id() % 2000) as u16;
        cfg.update(|c| {
            c.web_enabled = true;
            c.web_port = port;
            c.web_actions = true;
        });
        let token = cfg.get().web_token;
        let (tx, _rx) = futures::channel::mpsc::unbounded();
        let web = Web::new(cfg, Arc::new(Control::default()), tx, true);
        web.apply();
        assert!(web.status().running, "{:?}", web.status().error);
        let mut demo = neontop_core::demo::Demo::default();
        let p = web.publisher();
        for i in 0..5 {
            p.metrics(&Arc::new(demo.sample(1000, 1_700_000_000_000 + i * 1000)));
        }
        p.procs(&Arc::new(demo.procs()));

        // sin clave: página de acceso
        assert!(get(port, "/", None).starts_with("HTTP/1.1 401"));
        assert!(get(port, "/?t=mala", None).starts_with("HTTP/1.1 401"));
        // con la clave en el enlace: cookie y redirección sin la clave
        let r = get(port, &format!("/?t={token}&tab=juegos"), None);
        assert!(r.starts_with("HTTP/1.1 303"), "{r}");
        assert!(r.contains(&format!("neontop={token}")) && r.contains("HttpOnly"));
        assert!(r.contains("Location: /?tab=juegos"));
        let cookie = format!("neontop={token}");
        for tab in ["resumen", "procesos", "juegos", "energia"] {
            let page = get(port, &format!("/?tab={tab}"), Some(&cookie));
            assert!(page.starts_with("HTTP/1.1 200"), "{tab}: {page}");
            assert!(page.contains("NEONTOP") && page.contains("</html>"), "{tab}");
        }
        assert!(get(port, "/?tab=juegos", Some(&cookie)).contains("Juego de demostración"));
        assert!(get(port, "/?tab=procesos", Some(&cookie)).contains("firefox"));
        let snap = get(port, "/api/snapshot", Some(&cookie));
        assert!(snap.contains("\"cpu\"") && snap.contains("application/json"));
        assert_eq!(web.status().clients, 1);
        web.stop();
        assert!(!web.status().running);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn helpers() {
        assert!(eq_const(b"abc", b"abc"));
        assert!(!eq_const(b"abc", b"abd"));
        assert!(!eq_const(b"abc", b"ab"));
        assert_eq!(url_decode("a+b%20c%2F"), "a b c/");
        assert_eq!(url_decode("100%"), "100%");
        assert_eq!(esc("<a href=\"x\">&</a>"), "&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;");
        let q = query("tab=procesos&t=k%2B1");
        assert_eq!(q["tab"], "procesos");
        assert_eq!(q["t"], "k+1");
        let svg = svg_spark(&[0.0, 50.0, 100.0], "#fff", Some(100.0));
        assert!(svg.contains("polyline") && svg.contains("300.0,2.0"));
        assert_eq!(svg_spark(&[1.0], "#fff", None), "<svg></svg>");
    }
}
