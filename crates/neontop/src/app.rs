//! Ventana principal: barra lateral con las pestañas, cabecera (muestreo, ventana de tiempo,
//! pausa, reloj), contenido de la pestaña, avisos de alertas y panel de Ajustes.

use gpui::{
    canvas, div, fill, point, prelude::*, px, size, AnyElement, App, Bounds, Context, Decorations, Entity, FontWeight, Hsla, MouseButton, ResizeEdge, ScrollHandle,
    SharedString, Subscription, Tiling, Window, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions,
};
use neontop_core::config::{INTERVALS, WINDOWS};
use neontop_core::format as fmt;
use neontop_core::theme::Backdrop;

use crate::settings::SettingsView;
use crate::state::{state, AppState};
use crate::tabs;
use crate::ui::kit::{self, Cols};
use crate::ui::theme::{fade, Th};

/// Pestañas: id, nombre e icono.
pub const TABS: [(&str, &str, &str); 10] = [
    ("overview", "Resumen", "◈"),
    ("cpu", "CPU", "▦"),
    ("mem", "Memoria", "▤"),
    ("net", "Red", "⇅"),
    ("procs", "Procesos", "≣"),
    ("disks", "Discos", "◎"),
    ("sensors", "Sensores", "✺"),
    ("gpu", "GPU", "▣"),
    ("energy", "Energía", "ϟ"),
    ("games", "Juegos", "◭"),
];

/// Recogidas caras que necesita cada pestaña (procesos, uso de GPU por proceso).
fn wants_of(tab: &str) -> (bool, bool) {
    match tab {
        "procs" => (true, true),
        "overview" => (true, false),
        "gpu" => (false, true),
        "energy" | "games" => (true, true),
        _ => (false, false),
    }
}

pub struct Root {
    pub state: Entity<AppState>,
    pub settings: Option<Entity<SettingsView>>,
    pub procs: tabs::processes::ProcsView,
    pub games: tabs::games::GamesView,
    pub energy: tabs::energy::EnergyView,
    pub net_sel: Option<String>,
    pub net_virtual: bool,
    /// Totales de cada interfaz la primera vez que se vio (tráfico de la sesión).
    pub net_base: std::collections::HashMap<String, (u64, u64)>,
    pub disk_sel: Option<String>,
    pub disk_virtual: bool,
    pub gpu_sel: usize,
    /// Procesos en la GPU ordenados por uso (si no, por memoria).
    pub gpu_sort_busy: bool,
    scroll: ScrollHandle,
    _subs: Vec<Subscription>,
}

impl Root {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let st = state(cx);
        let subs = vec![cx.observe(&st, |_, _, cx| cx.notify()), cx.observe_window_activation(window, |_, _, cx| cx.notify())];
        let mut r = Root {
            state: st,
            settings: None,
            procs: tabs::processes::ProcsView::new(window, cx),
            games: tabs::games::GamesView::default(),
            energy: tabs::energy::EnergyView::default(),
            net_sel: None,
            net_virtual: false,
            net_base: Default::default(),
            disk_sel: None,
            disk_virtual: false,
            gpu_sel: 0,
            gpu_sort_busy: false,
            scroll: ScrollHandle::new(),
            _subs: subs,
        };
        r.sync_wants(cx);
        r
    }

    pub fn tab(&self, cx: &App) -> String {
        self.state.read(cx).ui.tab.clone()
    }

    /// Pide al muestreo lo que necesita la pestaña abierta (y suelta lo demás).
    fn sync_wants(&mut self, cx: &mut Context<Self>) {
        let (procs, gpu) = wants_of(&self.tab(cx));
        self.state.update(cx, |s, cx| {
            s.want("procs", "desktop", procs);
            s.want("gpu_procs", "desktop", gpu);
            if s.ui.tab == "games" {
                s.refresh_fps_auto(cx);
            }
        });
    }

    pub fn set_tab(&mut self, tab: &str, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| s.set_tab(tab, cx));
        self.scroll.set_offset(point(px(0.), px(0.)));
        self.sync_wants(cx);
        cx.notify();
    }

    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_none() {
            let root = cx.entity().downgrade();
            self.settings = Some(cx.new(|cx| SettingsView::new(root, window, cx)));
        } else {
            self.settings = None;
        }
        cx.notify();
    }

    pub fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings = None;
        cx.notify();
    }

    fn sidebar(&self, wide: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let th = Th::get(cx).clone();
        let cur = self.tab(cx);
        let snap = self.state.read(cx).model.snap.clone();
        let demo = self.state.read(cx).demo;
        let mut items: Vec<AnyElement> = Vec::new();
        for (id, label, icon) in TABS {
            let on = id == cur;
            let hint = tab_hint(id, &snap, &self.state.read(cx).model);
            let item = div()
                .id(SharedString::from(format!("tab-{id}")))
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(12.))
                .py(px(if wide { 9. } else { 6. }))
                .flex_shrink_0()
                .cursor_pointer()
                .rounded(px(th.radius.min(6.)))
                .border_1()
                .border_color(if on { fade(th.cyan, 0.7) } else { gpui::transparent_black() })
                .when(on, |d| d.bg(fade(th.cyan, 0.10)))
                .hover(|s| s.bg(fade(th.cyan, 0.07)))
                .on_click(cx.listener(move |this, _, _, cx| this.set_tab(id, cx)))
                .child(div().w(px(16.)).text_color(if on { th.cyan } else { th.muted }).child(icon))
                .child(div().text_size(px(12.5)).text_color(if on { th.text } else { fade(th.text, 0.8) }).child(label))
                .when(wide, |d| d.child(div().flex_1().flex().justify_end().text_size(px(10.)).font_family(th.mono.clone()).text_color(th.muted).child(hint)));
            items.push(item.into_any_element());
        }
        let logo = div()
            .id("logo")
            .flex()
            .items_baseline()
            .font_family(th.display.clone())
            .font_weight(FontWeight::BOLD)
            .text_size(px(th.dsize(if wide { 22. } else { 16. })))
            .child(div().text_color(th.cyan).child("NEON"))
            .child(div().text_color(th.magenta).child("TOP"))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move());
        let version = format!("v{}{}", env!("CARGO_PKG_VERSION"), if demo { " · modo demo" } else { "" });
        if wide {
            div()
                .w(px(190.))
                .flex_shrink_0()
                .h_full()
                .flex()
                .flex_col()
                .gap(px(4.))
                .px(px(10.))
                .py(px(14.))
                .border_r_1()
                .border_color(th.border(th.cyan))
                .bg(fade(th.panel, 0.55))
                .child(div().px(px(6.)).pb(px(14.)).child(logo))
                .children(items)
                .child(div().flex_1())
                .child(div().px(px(6.)).text_size(px(10.)).text_color(th.muted).font_family(th.mono.clone()).child(version))
                .into_any_element()
        } else {
            div()
                .id("tabbar")
                .w_full()
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(8.))
                .py(px(6.))
                .overflow_x_scroll()
                .border_b_1()
                .border_color(th.border(th.cyan))
                .child(div().pr(px(8.)).flex_shrink_0().child(logo))
                .children(items)
                .into_any_element()
        }
    }

    fn header(&self, window: &mut Window, narrow: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let th = Th::get(cx).clone();
        let st = self.state.read(cx);
        let tab = st.ui.tab.clone();
        let title = TABS.iter().find(|t| t.0 == tab).map(|t| t.1).unwrap_or("NeonTop");
        let info = &st.model.info;
        let paused = st.paused();
        let interval = st.ui.interval_ms;
        let win = st.ui.window_sec;
        let ts = st.model.snap.ts;
        let uptime = st.model.snap.uptime;
        let csd = matches!(window.window_decorations(), Decorations::Client { .. });
        let sub = format!("{} · {} · {}", info.hostname, info.os, info.kernel);

        let mut intervals: Vec<(u64, String)> = INTERVALS.iter().map(|&ms| (ms, fmt::interval(ms))).collect();
        if !INTERVALS.contains(&interval) {
            intervals.push((interval, fmt::interval(interval)));
        }
        let windows: Vec<(Option<u64>, String)> = WINDOWS.iter().map(|&w| (w, fmt::window(w))).collect();
        let s1 = self.state.clone();
        let s2 = self.state.clone();
        let controls = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .child(
                kit::button("pause", if paused { "▶" } else { "❚❚" }, if paused { th.amber } else { th.cyan }, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.state.update(cx, |s, cx| s.toggle_pause(cx)))),
            )
            .child(
                div().flex().items_center().gap(px(6.)).child(kit::muted("MUESTREO", cx).text_size(px(9.))).child(kit::segmented("iv", intervals, &interval, cx, move |ms, _, cx| {
                    let ms = *ms;
                    s1.update(cx, |s, cx| s.set_interval(ms, cx));
                })),
            )
            .child(
                div().flex().items_center().gap(px(6.)).child(kit::muted("VENTANA", cx).text_size(px(9.))).child(kit::segmented("win", windows, &win, cx, move |w, _, cx| {
                    let w = *w;
                    s2.update(cx, |s, cx| s.set_window(w, cx));
                })),
            )
            .child(kit::button("settings", "⚙", th.cyan, cx).on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .child(div().font_family(th.display.clone()).font_weight(FontWeight::BOLD).text_size(px(th.dsize(17.))).text_color(th.magenta).child(if ts > 0 { fmt::clock(ts) } else { "--:--:--".into() }))
                    .child(div().text_size(px(9.5)).text_color(th.muted).font_family(th.mono.clone()).child(format!("up {}", fmt::duration(uptime)))),
            );
        div()
            .id("header")
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(10.))
            .px(px(16.))
            .py(px(10.))
            .border_b_1()
            .border_color(th.border(th.cyan))
            .on_mouse_down(MouseButton::Left, |ev, window, _| {
                // arrastrar la ventana desde la cabecera; doble clic maximiza
                if ev.click_count == 2 {
                    window.zoom_window();
                } else {
                    window.start_window_move();
                }
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w(px(160.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(div().font_family(th.display.clone()).font_weight(FontWeight::BOLD).text_size(px(th.dsize(20.))).text_color(th.text).child(th.title(title)))
                            .when(paused, |d| d.child(kit::badge("EN PAUSA", th.amber))),
                    )
                    .when(!narrow, |d| d.child(div().text_size(px(10.)).text_color(th.muted).font_family(th.mono.clone()).truncate().child(sub))),
            )
            .child(controls)
            .when(csd, |d| d.child(window_controls(cx)))
    }

    fn toasts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let th = Th::get(cx).clone();
        let toasts = self.state.read(cx).model.alerts.toasts.clone();
        div().absolute().right(px(16.)).bottom(px(16.)).flex().flex_col().gap(px(8.)).w(px(330.)).children(toasts.into_iter().map(|t| {
            let (rule, key, id) = (t.rule.clone(), t.key.clone(), t.id);
            let (rule2, key2) = (rule.clone(), key.clone());
            div()
                .id(SharedString::from(format!("toast-{id}")))
                .occlude()
                .flex()
                .flex_col()
                .gap(px(6.))
                .p(px(10.))
                .bg(th.bg)
                .border_1()
                .border_color(th.red)
                .rounded(px(th.radius.min(6.)))
                .shadow(vec![gpui::BoxShadow { color: fade(th.red, 0.25), offset: point(px(0.), px(0.)), blur_radius: px(16.), spread_radius: px(0.) }])
                .child(div().text_color(th.red).font_weight(FontWeight::BOLD).text_size(px(12.5)).child(t.title.clone()))
                .child(div().text_color(th.muted).text_size(px(11.)).child(t.body.clone()))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .child(kit::button(SharedString::from(format!("mute-{id}")), "Silenciar 1 h", th.text, cx).on_click(cx.listener(move |this, _, _, cx| {
                            let rule = rule.clone();
                            this.state.update(cx, |s, cx| {
                                let now = fmt::now_ms();
                                s.model.alerts.mute(&rule, 60, now);
                                cx.notify();
                            });
                        })))
                        .when(!key.is_empty(), |d| {
                            d.child(kit::button(SharedString::from(format!("ign-{id}")), format!("Ignorar {key}"), th.text, cx).on_click(cx.listener(move |this, _, _, cx| {
                                let (rule, key) = (rule2.clone(), key2.clone());
                                this.state.update(cx, |s, cx| {
                                    s.model.alerts.ignore(&rule, &key);
                                    cx.notify();
                                });
                            })))
                        })
                        .child(kit::button(SharedString::from(format!("close-{id}")), "✕", th.muted, cx).on_click(cx.listener(move |this, _, _, cx| {
                            this.state.update(cx, |s, cx| {
                                s.model.alerts.dismiss(id);
                                cx.notify();
                            });
                        }))),
                )
        }))
    }
}

/// Indicador corto de cada pestaña en la barra lateral (uso, velocidad, FPS…).
fn tab_hint(id: &str, s: &neontop_core::sampler::Snapshot, m: &neontop_core::model::Model) -> String {
    if s.ts == 0 {
        return String::new();
    }
    match id {
        "cpu" => format!("{:.0}%", s.cpu.total.usage),
        "mem" => format!("{:.0}%", neontop_core::history::mem_segments(&s.mem).used as f64 / s.mem.total.max(1) as f64 * 100.0),
        "net" => fmt::rate(s.net.ifaces.iter().filter(|i| i.kind != "loopback").map(|i| i.rx_rate + i.tx_rate).sum()),
        "procs" => format!("{}", s.cpu.procs_total),
        "disks" => fmt::rate(s.disks.disks.iter().map(|d| d.read_rate + d.write_rate).sum()),
        "sensors" => s.cpu.temp.map(|t| format!("{t:.0}°")).unwrap_or_default(),
        "gpu" => s.gpus.first().and_then(|g| g.busy).map(|b| format!("{b:.0}%")).unwrap_or_default(),
        "energy" => fmt::watts(Some(s.energy.total_w)),
        "games" => m.games.current().filter(|g| g.info.active).map(|g| format!("{:.0} FPS", g.info.fps)).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Botones de ventana propios (con decoraciones del lado del cliente).
fn window_controls(cx: &mut Context<Root>) -> impl IntoElement {
    let th = Th::get(cx).clone();
    let b = |id: &'static str, s: &'static str, c: Hsla| {
        div()
            .id(id)
            .w(px(28.))
            .h(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .text_color(c)
            .cursor_pointer()
            .rounded(px(4.))
            .hover(move |st| st.bg(fade(c, 0.18)))
            .child(s)
    };
    div()
        .flex()
        .gap(px(2.))
        .child(b("win-min", "—", th.muted).on_click(|_, window, _| window.minimize_window()))
        .child(b("win-max", "▢", th.muted).on_click(|_, window, _| window.zoom_window()))
        .child(b("win-close", "✕", th.red).on_click(|_, window, _| window.remove_window()))
}

/// Fondo del tema (rejilla, puntos, líneas…) y líneas de escaneo CRT.
fn backdrop(cx: &App) -> impl IntoElement {
    let th = Th::get(cx).clone();
    canvas(
        |_, _, _| (),
        move |b, _, window, _| {
            let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
            let o = b.origin;
            match th.backdrop() {
                Backdrop::Grid => {
                    let c = fade(th.cyan, 0.045);
                    let mut x = 0.0;
                    while x < w {
                        window.paint_quad(fill(Bounds::new(point(o.x + px(x), o.y), size(px(1.), b.size.height)), c));
                        x += 44.0;
                    }
                    let mut y = 0.0;
                    while y < h {
                        window.paint_quad(fill(Bounds::new(point(o.x, o.y + px(y)), size(b.size.width, px(1.))), c));
                        y += 44.0;
                    }
                }
                Backdrop::Dots => {
                    let c = fade(th.cyan, 0.18);
                    let mut y = 10.0;
                    while y < h {
                        let mut x = 10.0;
                        while x < w {
                            window.paint_quad(fill(Bounds::new(point(o.x + px(x), o.y + px(y)), size(px(2.), px(2.))), c));
                            x += 24.0;
                        }
                        y += 24.0;
                    }
                }
                Backdrop::Lines | Backdrop::Paper => {
                    let c = fade(if th.light() { th.text } else { th.cyan }, 0.035);
                    let mut y = 0.0;
                    while y < h {
                        window.paint_quad(fill(Bounds::new(point(o.x, o.y + px(y)), size(b.size.width, px(1.))), c));
                        y += 28.0;
                    }
                }
                Backdrop::Aurora => {
                    // dos manchas de color difusas (degradados verticales)
                    for (x0, c) in [(0.0, th.violet), (0.55, th.cyan)] {
                        let bounds = Bounds::new(point(o.x + px(w * x0), o.y), size(px(w * 0.45), px(h * 0.6)));
                        window.paint_quad(
                            fill(bounds, gpui::linear_gradient(180., gpui::linear_color_stop(fade(c, 0.10), 0.), gpui::linear_color_stop(fade(c, 0.0), 1.))).corner_radii(px(w * 0.2)),
                        );
                    }
                }
                Backdrop::Plain => {}
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn scanlines(cx: &App) -> impl IntoElement {
    let th = Th::get(cx).clone();
    canvas(
        |_, _, _| (),
        move |b, _, window, _| {
            let c = fade(gpui::black(), if th.light() { 0.02 } else { 0.10 });
            let h = f32::from(b.size.height);
            let mut y = 0.0;
            while y < h {
                window.paint_quad(fill(Bounds::new(point(b.origin.x, b.origin.y + px(y)), size(b.size.width, px(1.))), c));
                y += 3.0;
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = Th::get(cx).clone();
        let vw = f32::from(window.viewport_size().width);
        let wide = vw >= 900.0;
        let content_w = if wide { vw - 190.0 } else { vw };
        let cols = if content_w >= 1180.0 {
            4
        } else if content_w >= 640.0 {
            2
        } else {
            1
        };
        cx.set_global(Cols(cols));
        let tab = self.tab(cx);
        let body: AnyElement = match tab.as_str() {
            "cpu" => tabs::cpu::render(self, window, cx),
            "mem" => tabs::memory::render(self, window, cx),
            "net" => tabs::network::render(self, window, cx),
            "procs" => tabs::processes::render(self, window, cx),
            "disks" => tabs::disks::render(self, window, cx),
            "sensors" => tabs::sensors::render(self, window, cx),
            "gpu" => tabs::gpu::render(self, window, cx),
            "energy" => tabs::energy::render(self, window, cx),
            "games" => tabs::games::render(self, window, cx),
            _ => tabs::overview::render(self, window, cx),
        };
        let ready = self.state.read(cx).model.samples > 0;
        let content = div()
            .id("content")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(px(if cols == 1 { 10. } else { 16. }))
            .child(if ready { body } else { kit::empty("INICIALIZANDO SENSORES…", cx).into_any_element() });
        let main = div().flex_1().min_w_0().h_full().flex().flex_col().child(self.header(window, cols == 1, cx)).child(content);
        let layout = if wide {
            div().size_full().flex().child(self.sidebar(true, cx)).child(main)
        } else {
            div().size_full().flex().flex_col().child(self.sidebar(false, cx)).child(main)
        };
        let decorations = window.window_decorations();
        div()
            .id("root")
            .size_full()
            .relative()
            .bg(th.bg)
            .text_color(th.text)
            .font_family(th.body.clone())
            .text_size(px(12.))
            .child(backdrop(cx))
            .child(layout)
            .when(th.scanlines, |d| d.child(scanlines(cx)))
            .child(self.toasts(cx))
            .children(self.settings.clone().map(|s| div().absolute().top_0().right_0().h_full().w(px((vw - 20.0).min(560.))).child(s)))
            .when(matches!(decorations, Decorations::Client { .. }), |d| d.child(resize_handles(decorations)))
    }
}

/// Bordes para redimensionar la ventana (con decoraciones del lado del cliente).
fn resize_handles(decorations: Decorations) -> impl IntoElement {
    let tiling = match decorations {
        Decorations::Client { tiling } => tiling,
        Decorations::Server => Tiling::default(),
    };
    let edge = |id: &'static str, e: ResizeEdge| div().id(id).absolute().occlude().on_mouse_down(MouseButton::Left, move |_, window, _| window.start_window_resize(e));
    const T: f32 = 5.0;
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .when(!tiling.top, |d| d.child(edge("rz-t", ResizeEdge::Top).top_0().left(px(T)).right(px(T)).h(px(T)).cursor_ns_resize()))
        .when(!tiling.bottom, |d| d.child(edge("rz-b", ResizeEdge::Bottom).bottom_0().left(px(T)).right(px(T)).h(px(T)).cursor_ns_resize()))
        .when(!tiling.left, |d| d.child(edge("rz-l", ResizeEdge::Left).left_0().top(px(T)).bottom(px(T)).w(px(T)).cursor_ew_resize()))
        .when(!tiling.right, |d| d.child(edge("rz-r", ResizeEdge::Right).right_0().top(px(T)).bottom(px(T)).w(px(T)).cursor_ew_resize()))
        .child(edge("rz-tl", ResizeEdge::TopLeft).top_0().left_0().size(px(T * 2.)).cursor_nwse_resize())
        .child(edge("rz-tr", ResizeEdge::TopRight).top_0().right_0().size(px(T * 2.)).cursor_nesw_resize())
        .child(edge("rz-bl", ResizeEdge::BottomLeft).bottom_0().left_0().size(px(T * 2.)).cursor_nesw_resize())
        .child(edge("rz-br", ResizeEdge::BottomRight).bottom_0().right_0().size(px(T * 2.)).cursor_nwse_resize())
}

/// Ventana principal abierta (si la hay).
fn main_window(cx: &App) -> Option<gpui::WindowHandle<Root>> {
    cx.windows().into_iter().find_map(|w| w.downcast::<Root>())
}

/// Abre la ventana principal, o la trae al frente si ya está abierta.
pub fn show_main_window(cx: &mut App) {
    if let Some(w) = main_window(cx) {
        let _ = w.update(cx, |_, window, _| window.activate_window());
        return;
    }
    let size_hint = std::env::var("NEONTOP_SIZE").ok().and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some(size(px(w.parse().ok()?), px(h.parse().ok()?)))
    });
    let bounds = Bounds::centered(None, size_hint.unwrap_or(size(px(1480.), px(940.))), cx);
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        app_id: Some("neontop".into()),
        window_min_size: Some(size(px(360.), px(480.))),
        window_decorations: Some(WindowDecorations::Client),
        window_background: WindowBackgroundAppearance::Opaque,
        focus: true,
        show: true,
        ..Default::default()
    };
    match cx.open_window(opts, |window, cx| {
        window.set_window_title("NeonTop");
        cx.new(|cx| Root::new(window, cx))
    }) {
        Ok(h) => {
            let _ = h.update(cx, |root, window, cx| {
                window.activate_window();
                // revisión visual: abrir Ajustes directamente
                if std::env::var_os("NEONTOP_QA_SETTINGS").is_some() {
                    root.open_settings(window, cx);
                }
            });
        }
        Err(e) => eprintln!("NeonTop: no se pudo abrir la ventana: {e}"),
    }
}

/// Al cerrarse una ventana: soltar las recogidas caras y salir si no queda nada abierto (salvo
/// que NeonTop deba quedarse en la bandeja).
pub fn on_window_closed(cx: &mut App) {
    if main_window(cx).is_some() {
        return;
    }
    let st = state(cx);
    let to_tray = st.read(cx).ui.close_to_tray && crate::tray::available();
    st.update(cx, |s, _| {
        s.want("procs", "desktop", false);
        s.want("gpu_procs", "desktop", false);
    });
    if !to_tray {
        st.update(cx, |s, _| s.model.flush());
        crate::single::cleanup();
        cx.quit();
    }
}
