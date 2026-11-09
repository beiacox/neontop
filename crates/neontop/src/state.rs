//! Estado de la aplicación, compartido por todas las ventanas y vivo aunque no haya ninguna
//! abierta (con NeonTop en la bandeja se siguen midiendo energía, partidas y alertas).
//!
//! El hilo de muestreo manda cada muestra por un canal; una tarea del hilo principal las pasa al
//! `Model` y avisa a las ventanas para repintar, como mucho cada `UI_MIN_MS`.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::StreamExt;
use gpui::{App, AppContext, AsyncApp, Context, Entity, Global};
use neontop_core::collectors::membench::MemBench;
use neontop_core::collectors::processes::ProcSnapshot;
use neontop_core::config::{ConfigStore, UiSettings};
use neontop_core::launchers::FpsAuto;
use neontop_core::model::Model;
use neontop_core::sampler::{self, Control, Snapshot};

use crate::ui::theme;

/// Intervalo mínimo entre repintados: el muestreo puede ir a 100 ms (el historial guarda todas
/// las muestras), pero repintar decenas de gráficas más de ~4 veces por segundo no aporta nada.
pub const UI_MIN_MS: u64 = 250;

/// Mensajes que llegan al hilo principal.
pub enum Msg {
    Metrics(Arc<Snapshot>),
    Procs(Arc<ProcSnapshot>),
    /// Otra instancia o la bandeja piden mostrar la ventana.
    Show,
    /// Desde la bandeja.
    TogglePause,
    Quit,
}

/// Lado del hilo de muestreo: reenvía las muestras al hilo principal (y a la web).
pub struct Feed {
    pub tx: UnboundedSender<Msg>,
    pub web: Option<crate::web::Publisher>,
}

impl sampler::Sink for Feed {
    fn metrics(&mut self, snap: &Arc<Snapshot>) {
        if let Some(w) = &self.web {
            w.metrics(snap);
        }
        let _ = self.tx.unbounded_send(Msg::Metrics(snap.clone()));
    }
    fn procs(&mut self, list: &Arc<ProcSnapshot>) {
        if let Some(w) = &self.web {
            w.procs(list);
        }
        let _ = self.tx.unbounded_send(Msg::Procs(list.clone()));
    }
}

/// Resultado de una acción en segundo plano, para mostrarlo en la interfaz.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    Ok(String),
    Err(String),
}

pub struct AppState {
    pub model: Model,
    pub ctl: Arc<Control>,
    pub cfg: Arc<ConfigStore>,
    pub ui: UiSettings,
    pub demo: bool,
    /// Se incrementa en cada repintado (las vistas comparan para saber si hay datos nuevos).
    pub tick: u64,
    last_notify: Instant,
    pending: bool,
    /// Vistas que piden cada recogida cara ("procs" / "gpu_procs").
    wants: HashSet<(&'static str, &'static str)>,
    pub fps_auto: Option<FpsAuto>,
    /// Resultados del test de RAM (el último al final), guardados en disco.
    pub membench: Vec<MemBench>,
    pub membench_running: bool,
    /// Último mensaje de una acción (permisos RAPL, tráfico de RAM, Steam…).
    pub notice: Option<Notice>,
    pub web: crate::web::Web,
}

/// Acceso global al estado.
#[derive(Clone)]
pub struct StateHandle(pub Entity<AppState>);
impl Global for StateHandle {}

pub fn state(cx: &App) -> Entity<AppState> {
    cx.global::<StateHandle>().0.clone()
}

impl AppState {
    /// Recogida cara pedida (`on`) o liberada por la vista `who`.
    pub fn want(&mut self, kind: &'static str, who: &'static str, on: bool) {
        let changed = if on { self.wants.insert((kind, who)) } else { self.wants.remove(&(kind, who)) };
        if changed {
            let _ = self.ctl.collect.set(kind, who, on, false);
        }
    }

    fn save_ui(&self) {
        let ui = self.ui.clone();
        self.cfg.update(|c| c.ui = ui);
    }

    pub fn set_tab(&mut self, tab: &str, cx: &mut Context<Self>) {
        self.ui.tab = tab.to_string();
        self.save_ui();
        cx.notify();
    }

    pub fn set_theme(&mut self, id: &str, cx: &mut Context<Self>) {
        self.ui.theme = id.to_string();
        self.save_ui();
        theme::apply(cx, id, self.ui.glow, self.ui.scanlines);
        cx.notify();
        cx.refresh_windows();
    }

    pub fn set_effects(&mut self, glow: bool, scanlines: bool, cx: &mut Context<Self>) {
        self.ui.glow = glow;
        self.ui.scanlines = scanlines;
        self.save_ui();
        theme::apply(cx, &self.ui.theme.clone(), glow, scanlines);
        cx.notify();
        cx.refresh_windows();
    }

    pub fn set_interval(&mut self, ms: u64, cx: &mut Context<Self>) {
        self.ui.interval_ms = ms.clamp(100, 60_000);
        self.ctl.set_interval(self.ui.interval_ms);
        self.save_ui();
        cx.notify();
    }

    pub fn set_window(&mut self, sec: Option<u64>, cx: &mut Context<Self>) {
        self.ui.window_sec = sec;
        self.save_ui();
        cx.notify();
    }

    pub fn paused(&self) -> bool {
        self.ctl.paused.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        let p = !self.paused();
        self.ctl.paused.store(p, std::sync::atomic::Ordering::Relaxed);
        crate::tray::set_paused(p);
        cx.notify();
    }

    /// Cambia el idioma (`None` = el del sistema), lo guarda y redibuja todo.
    pub fn set_lang(&mut self, lang: Option<String>, cx: &mut Context<Self>) {
        neontop_core::i18n::init(lang.as_deref());
        self.ui.lang = lang;
        if !self.demo {
            self.save_ui();
        }
        crate::tray::refresh();
        cx.refresh_windows();
        cx.notify();
    }

    pub fn update_ui(&mut self, f: impl FnOnce(&mut UiSettings), cx: &mut Context<Self>) {
        f(&mut self.ui);
        self.save_ui();
        cx.notify();
    }

    /// Ventana visible de las gráficas.
    pub fn window(&self) -> neontop_core::history::Window {
        self.model.history.window(self.ui.window_sec.map(|s| s as f64))
    }

    fn on_msg(&mut self, msg: Msg, cx: &mut Context<Self>) {
        match msg {
            Msg::Metrics(s) => {
                let fired = self.model.ingest(s);
                crate::notify::send(&fired);
                crate::tray::update(&self.model.snap);
                self.pending = true;
                self.flush(cx);
            }
            Msg::Procs(p) => {
                self.model.ingest_procs(p);
                self.pending = true;
                self.flush(cx);
            }
            Msg::Show => {}
            Msg::TogglePause => self.toggle_pause(cx),
            Msg::Quit => {
                self.model.flush();
                crate::single::cleanup();
                cx.quit();
            }
        }
    }

    /// Repinta si hay datos nuevos y ha pasado el intervalo mínimo; si no, lo deja para luego.
    fn flush(&mut self, cx: &mut Context<Self>) {
        if !self.pending {
            return;
        }
        if self.last_notify.elapsed() >= Duration::from_millis(UI_MIN_MS) || self.model.samples <= 1 {
            self.pending = false;
            self.last_notify = Instant::now();
            self.tick += 1;
            cx.notify();
        } else {
            let wait = Duration::from_millis(UI_MIN_MS).saturating_sub(self.last_notify.elapsed());
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |s, cx| s.flush(cx));
            })
            .detach();
        }
    }

    /// Test de ancho de banda de la RAM (~1–2 s de carga, en segundo plano).
    pub fn run_membench(&mut self, cx: &mut Context<Self>) {
        if self.membench_running {
            return;
        }
        self.membench_running = true;
        cx.notify();
        let demo = self.demo;
        self.run_action(
            cx,
            move || {
                if demo {
                    std::thread::sleep(Duration::from_millis(800));
                    return Ok(MemBench { read: 41e9, write: 19e9, copy: 20e9, read_single: 27e9, latency_ns: 78.0, threads: 12, buffer_bytes: 512 << 20, duration_ms: 1200.0, ts: neontop_core::format::now_ms() });
                }
                Ok(neontop_core::collectors::membench::run())
            },
            |s, r: Result<MemBench, String>| {
                s.membench_running = false;
                if let Ok(b) = r {
                    s.membench.push(b);
                    let keep = s.membench.len().saturating_sub(30);
                    s.membench.drain(..keep);
                    if !s.demo
                        && let Some(d) = neontop_core::paths::data_dir() {
                            let _ = neontop_core::paths::write_json(&d.join("membench.json"), &s.membench);
                        }
                }
            },
        );
    }

    /// Vuelve a leer el estado de la medición automática de FPS (lectura de ficheros: en segundo plano).
    pub fn refresh_fps_auto(&mut self, cx: &mut Context<Self>) {
        // en la demostración no se leen ni tocan los lanzadores reales
        if self.demo {
            if self.fps_auto.is_none() {
                self.fps_auto = Some(neontop_core::demo::fps_auto());
            }
            return;
        }
        let cfg = self.cfg.clone();
        cx.spawn(async move |this, cx| {
            let st = cx.background_executor().spawn(async move { neontop_core::launchers::status(&cfg) }).await;
            let _ = this.update(cx, |s, cx| {
                s.fps_auto = Some(st);
                cx.notify();
            });
        })
        .detach();
    }

    /// Ejecuta una acción bloqueante (pkexec, ficheros, Steam…) fuera del hilo principal y
    /// muestra su resultado.
    pub fn run_action<R: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        work: impl FnOnce() -> Result<R, String> + Send + 'static,
        done: impl FnOnce(&mut AppState, Result<R, String>) + 'static,
    ) {
        cx.spawn(async move |this, cx| {
            let res = cx.background_executor().spawn(async move { work() }).await;
            let _ = this.update(cx, |s, cx| {
                done(s, res);
                cx.notify();
            });
        })
        .detach();
    }
}

/// Crea el estado, arranca el muestreo (real o simulado) y la tarea que recibe las muestras.
pub fn init(cx: &mut App, demo: bool) -> (Entity<AppState>, UnboundedSender<Msg>) {
    // en la demostración la configuración solo vive en memoria: no toca la del usuario
    let cfg = Arc::new(ConfigStore::load(if demo { None } else { neontop_core::paths::config_dir() }));
    let ui = cfg.get().ui;
    let ctl = Arc::new(Control::default());
    ctl.set_interval(ui.interval_ms);
    let (tx, rx) = unbounded::<Msg>();
    let mut model = if demo { Model::new(neontop_core::demo::info(), false) } else { Model::for_this_machine(true) };
    if demo {
        model.games.benchmarks = neontop_core::demo::history(neontop_core::format::now_ms());
    }
    let web = crate::web::Web::new(cfg.clone(), ctl.clone(), tx.clone(), demo);
    let entity = cx.new(|_| AppState {
        model,
        ctl: ctl.clone(),
        cfg: cfg.clone(),
        ui,
        demo,
        tick: 0,
        last_notify: Instant::now() - Duration::from_secs(1),
        pending: false,
        wants: HashSet::new(),
        fps_auto: None,
        membench: if demo {
            vec![MemBench { read: 41e9, write: 19e9, copy: 20e9, read_single: 27e9, latency_ns: 72.4, threads: 12, buffer_bytes: 512 << 20, duration_ms: 900.0, ts: neontop_core::format::now_ms() }]
        } else { neontop_core::paths::data_dir().map(|d| neontop_core::paths::read_json(&d.join("membench.json"))).unwrap_or_default() },
        membench_running: false,
        notice: None,
        web: web.clone(),
    });
    cx.set_global(StateHandle(entity.clone()));

    if demo {
        spawn_demo(cx, ctl, tx.clone(), web.publisher());
    } else {
        sampler::spawn(ctl.clone(), Feed { tx: tx.clone(), web: Some(web.publisher()) });
        // medición de FPS automática: vigila los lanzadores de juegos y aplica cuando están cerrados
        neontop_core::launchers::spawn(cfg.clone());
        web.apply();
    }
    receive(cx, entity.clone(), rx);
    (entity, tx)
}

fn receive(cx: &mut App, entity: Entity<AppState>, mut rx: UnboundedReceiver<Msg>) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Some(msg) = rx.next().await {
            let r = match msg {
                // abrir la ventana lee el estado: no puede hacerse dentro de su actualización
                Msg::Show => cx.update(crate::app::show_main_window).map_err(|_| ()),
                m => entity.update(cx, |s, cx| s.on_msg(m, cx)).map_err(|_| ()),
            };
            if r.is_err() {
                break;
            }
        }
    })
    .detach();
}

/// Modo demostración: muestras sintéticas en lugar del hilo de muestreo.
fn spawn_demo(_cx: &mut App, ctl: Arc<Control>, tx: UnboundedSender<Msg>, web: crate::web::Publisher) {
    std::thread::Builder::new()
        .name("neontop-demo".into())
        .spawn(move || {
            let mut demo = neontop_core::demo::Demo::default();
            if let Some(end) = std::env::var("NEONTOP_DEMO_FIN").ok().and_then(|v| v.parse().ok()) {
                demo.game_end = end;
            }
            let mut feed = Feed { tx, web: Some(web) };
            use neontop_core::sampler::Sink;
            // historial previo para que las gráficas no empiecen vacías
            let warm = std::env::var("NEONTOP_DEMO_WARM").ok().and_then(|v| v.parse().ok()).unwrap_or(120u64);
            let now = neontop_core::format::now_ms();
            for i in 0..warm {
                let ms = ctl.interval_ms.load(std::sync::atomic::Ordering::Relaxed);
                feed.metrics(&Arc::new(demo.sample(ms, now - (warm - i) * ms)));
            }
            let mut n = 0u64;
            loop {
                let ms = ctl.interval_ms.load(std::sync::atomic::Ordering::Relaxed);
                if !ctl.paused.load(std::sync::atomic::Ordering::Relaxed) {
                    feed.metrics(&Arc::new(demo.sample(ms, neontop_core::format::now_ms())));
                    if ctl.collect.procs.load(std::sync::atomic::Ordering::Relaxed) || n.is_multiple_of(5) {
                        feed.procs(&Arc::new(demo.procs()));
                    }
                    n += 1;
                }
                ctl.collect.refresh();
                std::thread::sleep(Duration::from_millis(ms));
            }
        })
        .expect("hilo de demostración");
}
