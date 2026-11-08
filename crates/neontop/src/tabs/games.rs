//! Juegos: FPS, 1 % y 0,1 % bajo, tiempos de fotograma, tirones, estabilidad, qué limita los FPS,
//! grabación de benchmarks, historial de partidas (con vista detallada) y medición automática.

use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, Hsla, SharedString, Window};
use neontop_core::fps::{main_gpu, stability_label, Benchmark, Percentiles, SessionDetail, LIVE_WINDOW};
use neontop_core::format as fmt;

use super::{ht, hv, y_fps, y_ms, y_pct};
use crate::app::Root;
use crate::charts::{self, Line, Plot, XAxis};
use crate::state::Notice;
use crate::ui::kit::{self, Panel};
use crate::ui::theme::{fade, Th};

pub struct GamesView {
    /// Duración de la grabación (s); `None` = hasta detenerla.
    pub rec_secs: Option<u32>,
    /// Registro del historial que se está viendo.
    pub viewing: Option<String>,
    pub filter_game: Option<String>,
    pub msg: Option<Notice>,
    pub confirm_delete: Option<String>,
}

impl Default for GamesView {
    fn default() -> Self {
        GamesView { rec_secs: Some(60), viewing: None, filter_game: None, msg: None, confirm_delete: None }
    }
}

fn api_name(a: &str) -> &str {
    match a {
        "vulkan" => "Vulkan",
        "opengl" => "OpenGL",
        "egl" => "OpenGL ES / EGL",
        x => x,
    }
}

fn f1(v: f32) -> String {
    if v > 0.0 && v.is_finite() { format!("{v:.1}") } else { "—".into() }
}

/// Distribución de tiempos de fotograma: cubos de 0,5 o 1 ms hasta ~1,5× el percentil 99,9;
/// en rojo los que superan el doble de la mediana (tirones).
fn hist_bins(counts: &[u32], step: f32, p999: f32, median: f32, th: &Th) -> (Vec<(f32, Hsla)>, f32) {
    let up_to = ((counts.len() as f32 * step).min((20f32).max(((p999 * 1.5) / 5.0).ceil() * 5.0))).max(step);
    let bin = if up_to <= 40.0 { 0.5 } else { 1.0 };
    let per = ((bin / step).round() as usize).max(1);
    let n = (up_to / bin).ceil() as usize;
    let mut bins = vec![0u32; n.max(1)];
    for (i, c) in counts.iter().enumerate() {
        bins[(i / per).min(n.max(1) - 1)] += c;
    }
    (bins.into_iter().enumerate().map(|(i, v)| (v as f32, if median > 0.0 && i as f32 * bin > median * 2.0 { th.red } else { th.lime })).collect(), bin)
}

/// FPS por percentil (media, mediana, 10/5/1/0,1 % bajos y mínimo).
fn pct_rows(avg: f32, p: &Percentiles, th: &Th, cx: &gpui::App) -> impl IntoElement {
    let fps = |ms: f32| if ms > 0.0 { 1000.0 / ms } else { 0.0 };
    let rows = [
        ("Media", avg, th.cyan),
        ("Mediana", fps(p.p50), th.teal),
        ("10 % bajo", fps(p.p90), th.lime),
        ("5 % bajo", fps(p.p95), th.lime),
        ("1 % bajo", fps(p.p99), th.amber),
        ("0,1 % bajo", fps(p.p999), th.orange),
        ("Mínimo", fps(p.max), th.red),
    ];
    let max = rows.iter().map(|r| r.1).fold(1.0, f32::max);
    div().flex().flex_col().gap(px(7.)).children(rows.into_iter().map(|(l, v, c)| kit::meter_row(l, v / max, f1(v), c, cx)))
}

fn export_csv(this: &mut Root, b: Benchmark, cx: &mut Context<Root>) {
    let csv = this.state.update(cx, |s, _| s.model.games.bench_csv(&b.id));
    let name = format!("neontop-{}-{}.csv", b.title.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect::<String>(), fmt::datetime(b.date).replace(['/', ' ', ':'], ""));
    let dir = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_else(|| "/".into());
    let rx = cx.prompt_for_new_path(&dir, Some(&name));
    cx.spawn(async move |this, cx| {
        let msg = match rx.await {
            Ok(Ok(Some(path))) => match std::fs::write(&path, csv) {
                Ok(()) => Notice::Ok(format!("Guardado en {}", path.display())),
                Err(e) => Notice::Err(format!("No se pudo guardar: {e}")),
            },
            Ok(Ok(None)) | Err(_) => return,
            Ok(Err(e)) => Notice::Err(e.to_string()),
        };
        let _ = this.update(cx, |this, cx| {
            this.games.msg = Some(msg);
            cx.notify();
        });
    })
    .detach();
}

fn history(root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let th = Th::get(cx).clone();
    let list = root.state.read(cx).model.games.benchmarks.clone();
    let filter = root.games.filter_game.clone();
    // un juego por tarjeta: pulsarla filtra sus registros
    let mut groups: Vec<(String, String, u32, u32, f64, f32, u64)> = Vec::new();
    for b in &list {
        match groups.iter_mut().find(|g| g.0 == b.exe) {
            Some(g) => {
                if b.is_bench() {
                    g.3 += 1;
                } else {
                    g.2 += 1;
                }
                g.4 += b.seconds;
                g.5 = g.5.max(b.avg);
                if b.date > g.6 {
                    g.6 = b.date;
                    g.1 = b.title.clone();
                }
            }
            None => groups.push((b.exe.clone(), b.title.clone(), (!b.is_bench()) as u32, b.is_bench() as u32, b.seconds, b.avg, b.date)),
        }
    }
    groups.sort_by(|a, b| b.6.cmp(&a.6));
    let card = |id: String, on: bool, title: String, lines: Vec<String>, exe: Option<String>| {
        div()
            .id(SharedString::from(format!("g-{id}")))
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(8.))
            .min_w(px(160.))
            .cursor_pointer()
            .border_1()
            .border_color(if on { th.cyan } else { th.border(th.cyan) })
            .rounded(px(th.radius.min(6.)))
            .when(on, |d| d.bg(fade(th.cyan, 0.10)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.games.filter_game = if this.games.filter_game == exe { None } else { exe.clone() };
                cx.notify();
            }))
            .child(div().font_weight(FontWeight::BOLD).truncate().child(title))
            .children(lines.into_iter().map(|l| kit::muted(l, cx)))
    };
    let cards = div()
        .flex()
        .flex_wrap()
        .gap(px(8.))
        .child(card("todos".into(), filter.is_none(), "Todos".into(), vec![format!("{} registros", list.len())], None))
        .children(groups.iter().map(|g| {
            card(
                g.0.clone(),
                filter.as_deref() == Some(&g.0),
                g.1.clone(),
                vec![
                    format!("{} {}{} · {}", g.2, if g.2 == 1 { "partida" } else { "partidas" }, if g.3 > 0 { format!(" · {} bench.", g.3) } else { String::new() }, fmt::duration(g.4)),
                    format!("mejor media {:.0} FPS · {}", g.5, &fmt::datetime(g.6)[..5]),
                ],
                Some(g.0.clone()),
            )
        }));
    let cols: [(&str, f32); 13] = [
        ("Juego", 0.0),
        ("Tipo", 80.),
        ("Fecha", 120.),
        ("Duración", 74.),
        ("Media", 56.),
        ("1 %", 50.),
        ("0,1 %", 50.),
        ("Estab.", 50.),
        ("Tirones", 56.),
        ("GPU", 48.),
        ("CPU", 48.),
        ("W", 40.),
        ("Máx °C", 64.),
    ];
    let c = |w: f32| if w > 0.0 { div().w(px(w)).flex_shrink_0().flex().justify_end() } else { div().flex_1().min_w(px(140.)).truncate() };
    let head = div()
        .flex()
        .text_size(px(10.))
        .text_color(th.muted)
        .font_family(th.mono.clone())
        .border_b_1()
        .border_color(th.border(th.cyan))
        .pb(px(3.))
        .children(cols.iter().map(|(l, w)| c(*w).child(th.title(l))))
        .child(div().w(px(90.)));
    let confirm = root.games.confirm_delete.clone();
    let rows = list.into_iter().filter(|b| filter.as_ref().is_none_or(|f| &b.exe == f)).map(|b| {
        let (_, sc) = stability_label(b.stability, b.frames);
        let id = b.id.clone();
        let (id2, id3) = (b.id.clone(), b.id.clone());
        let bb = b.clone();
        let asking = confirm.as_deref() == Some(&b.id);
        div()
            .id(SharedString::from(format!("b-{}", b.id)))
            .flex()
            .items_center()
            .h(px(24.))
            .text_size(px(11.))
            .font_family(th.mono.clone())
            .cursor_pointer()
            .hover(|s| s.bg(fade(th.cyan, 0.08)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.games.viewing = Some(id.clone());
                cx.notify();
            }))
            .child(c(0.0).child(b.title.clone()))
            .child(c(80.).text_color(th.muted).child(if b.is_bench() { "benchmark" } else { "partida" }))
            .child(c(120.).child(fmt::datetime(b.date)))
            .child(c(74.).child(fmt::duration(b.seconds)))
            .child(c(56.).text_color(th.lime).font_weight(FontWeight::BOLD).child(format!("{:.1}", b.avg)))
            .child(c(50.).child(format!("{:.1}", b.low1)))
            .child(c(50.).child(format!("{:.1}", b.low01)))
            .child(c(50.).text_color(th.slot(sc)).child(format!("{:.0}", b.stability)))
            .child(c(56.).child(b.stutters.to_string()))
            .child(c(48.).child(format!("{:.0}%", b.gpu)))
            .child(c(48.).child(format!("{:.0}%", b.cpu)))
            .child(c(40.).child(format!("{:.0}", b.gpu_power)))
            .child(c(64.).child(format!("{:.0}/{:.0}", b.cpu_temp_max, b.gpu_temp_max)))
            .child(
                div()
                    .w(px(90.))
                    .flex()
                    .justify_end()
                    .gap(px(4.))
                    .when(b.is_bench(), |d| {
                        d.child(kit::button(SharedString::from(format!("csv-{}", b.id)), "CSV", th.cyan, cx).py(px(0.)).on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            export_csv(this, bb.clone(), cx);
                        })))
                    })
                    .child(kit::button(SharedString::from(format!("del-{}", b.id)), if asking { "¿Borrar?" } else { "✕" }, th.red, cx).py(px(0.)).on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this.games.confirm_delete.as_deref() == Some(&id2) {
                            let id = id3.clone();
                            this.state.update(cx, |s, cx| {
                                s.model.games.remove(&id);
                                cx.notify();
                            });
                            this.games.confirm_delete = None;
                        } else {
                            this.games.confirm_delete = Some(id2.clone());
                        }
                        cx.notify();
                    }))),
            )
    });
    let n = root.state.read(cx).model.games.benchmarks.len();
    Panel::new("Historial", th.cyan)
        .span(4)
        .extra(format!("partidas y benchmarks · {n} {}", if n == 1 { "registro" } else { "registros" }))
        .when(n == 0, |p| p.child(kit::muted("Aún no hay nada. Cada partida se guarda sola al cerrar el juego; para una medición controlada, pulsa «Grabar benchmark».", cx)))
        .when(n > 0, |p| {
            p.child(cards)
                .child(div().id("hist-table").overflow_x_scroll().child(div().min_w(px(1020.)).flex().flex_col().child(head).children(rows)))
                .children(root.games.msg.clone().and_then(|m| crate::tabs::memory::notice_line(Some(m), cx)))
                .child(kit::muted(
                    "Pulsa un registro para ver la partida completa: FPS durante toda la partida, tirones, CPU, GPU, temperaturas, distribución de tiempos y la comparación con tus otras partidas. Se guarda en este equipo (las partidas, solas al cerrar el juego si duran más de 10 s).",
                    cx,
                ))
        })
}

/// Medición automática: NeonTop añade `neontop-fps` a los lanzadores de juegos.
fn fps_auto(root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let th = Th::get(cx).clone();
    let st = root.state.read(cx);
    let Some(fa) = st.fps_auto.clone() else { return div().child(kit::muted("Leyendo la configuración de los lanzadores…", cx)) };
    let demo = st.demo;
    let notice = st.notice.clone();
    let enabled = fa.enabled;
    let done = |l: &neontop_core::launchers::LauncherStatus| if enabled { l.applied >= l.total } else { l.applied == 0 };
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(kit::toggle("fps-auto", enabled, "Añadir neontop-fps automáticamente a los juegos", cx, cx.listener(move |this, _, _, cx| {
            this.state.update(cx, |s, cx| {
                let cfg = s.cfg.clone();
                s.run_action(
                    cx,
                    move || {
                        cfg.update(|c| {
                            c.fps_auto = !enabled;
                            c.fps_auto_cleanup = enabled;
                        });
                        Ok(neontop_core::launchers::apply(&cfg))
                    },
                    |s, r| s.fps_auto = r.ok(),
                );
            });
        })))
        .child(kit::muted(if enabled { "Cada juego instalado (también los nuevos) arranca con la medición, sin tocar nada." } else { "Desactivado: NeonTop quita lo que añadió." }, cx))
        .children(fa.launchers.into_iter().map(|l| {
            let ok = done(&l);
            let steam = l.id == "steam";
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(12.))
                .text_size(px(12.))
                .child(div().w(px(120.)).font_weight(FontWeight::BOLD).child(l.name))
                .child(kit::muted(l.scope, cx).w(px(260.)))
                .child(div().w(px(90.)).font_family(th.mono.clone()).child(if l.total > 1 { format!("{}/{} juegos", l.applied, l.total) } else if l.applied > 0 { "activo".into() } else { "sin activar".into() }))
                .child(if ok {
                    div().text_color(th.lime).child("● listo")
                } else if l.running {
                    div().text_color(th.amber).child(format!("● pendiente: se aplica al cerrar {}", l.name))
                } else {
                    div().text_color(th.amber).child("● aplicando…")
                })
                .when(steam && l.running && !ok && !demo, |d| {
                    d.child(kit::button("steam-restart", "Reiniciar Steam y aplicar", th.cyan, cx).on_click(cx.listener(|this, _, _, cx| {
                        this.state.update(cx, |s, cx| {
                            let cfg = s.cfg.clone();
                            s.notice = Some(Notice::Ok("Cerrando Steam…".into()));
                            s.run_action(cx, move || neontop_core::launchers::restart_steam(&cfg), |s, r| match r {
                                Ok(f) => {
                                    s.fps_auto = Some(f);
                                    s.notice = Some(Notice::Ok("Listo: Steam se está abriendo de nuevo con los cambios.".into()));
                                }
                                Err(e) => s.notice = Some(Notice::Err(e)),
                            });
                        });
                    })))
                })
        }))
        .children(crate::tabs::memory::notice_line(notice, cx))
}

fn help(root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let th = Th::get(cx).clone();
    let layer = root.state.read(cx).model.games.layer_installed;
    let code = |s: &str| div().px(px(8.)).py(px(4.)).bg(fade(th.bg, 0.7)).border_1().border_color(th.border(th.lime)).rounded(px(4.)).font_family(th.mono.clone()).text_color(th.lime).child(s.to_string());
    let col = |title: &str, body: Vec<AnyElement>| div().flex_1().min_w(px(220.)).flex().flex_col().gap(px(6.)).child(div().font_weight(FontWeight::BOLD).text_color(th.cyan).child(title.to_string())).children(body);
    Panel::new("Cómo medir un juego", th.lime)
        .span(4)
        .child(div().child("NeonTop mide cada fotograma con una capa propia (como MangoHud): funciona con juegos Vulkan, OpenGL y todos los de Proton/Wine (DirectX a través de DXVK y VKD3D). Se activa solo en los juegos que lances con ella: NeonTop puede ponerla por ti en Steam, Heroic, Lutris y Prism Launcher."))
        .child(div().p(px(10.)).border_1().border_color(th.border(th.lime)).bg(fade(th.lime, 0.04)).rounded(px(th.radius.min(6.))).child(fps_auto(root, cx)))
        .child(div().font_weight(FontWeight::BOLD).child("A mano (otros lanzadores o terminal)"))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(20.))
                .child(col("Steam", vec![kit::muted("Propiedades del juego → General → Opciones de lanzamiento:", cx).into_any_element(), code("neontop-fps %command%").into_any_element(), kit::muted("Combinable: neontop-fps gamemoderun mangohud %command%", cx).into_any_element()]))
                .child(col("Lutris, Heroic, Prism", vec![kit::muted("Comando envoltorio / prefijo de comando:", cx).into_any_element(), code("neontop-fps").into_any_element(), kit::muted("Lutris: Opciones del sistema · Heroic: Avanzado · Prism Launcher: Comandos personalizados", cx).into_any_element()]))
                .child(col("Terminal", vec![code("neontop-fps ./juego").into_any_element()])),
        )
        .child(if layer {
            div().child(div().text_color(th.lime).child("● Capa instalada. Abre un juego con la opción de arriba y aparecerá aquí automáticamente."))
        } else {
            div().child(div().text_color(th.amber).child("● La capa no está instalada (se instala con el paquete de NeonTop)."))
        })
}

fn session_view(root: &mut Root, id: String, cx: &mut Context<Root>) -> AnyElement {
    let th = Th::get(cx).clone();
    let (b, detail, siblings): (Option<Benchmark>, Option<SessionDetail>, Vec<Benchmark>) = root.state.update(cx, |s, _| {
        let g = &mut s.model.games;
        let b = g.benchmarks.iter().find(|x| x.id == id).cloned();
        let d = g.detail(&id).cloned();
        let sib = b.as_ref().map(|b| g.benchmarks.iter().filter(|x| x.exe == b.exe && x.is_bench() == b.is_bench()).cloned().collect()).unwrap_or_default();
        (b, d, sib)
    });
    let back = kit::button("back", "← Volver", th.cyan, cx).on_click(cx.listener(|this, _, _, cx| {
        this.games.viewing = None;
        cx.notify();
    }));
    let Some(b) = b else {
        return kit::grid(cx).child(Panel::new("Registro no encontrado", th.muted).span(4).child(kit::muted("Este registro ya no está en el historial.", cx)).child(back)).into_any_element();
    };
    let (label, lc) = stability_label(b.stability, b.frames);
    let bb = b.clone();
    let del_id = b.id.clone();
    let head = div()
        .col_span(kit::cols(cx) as u16)
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(14.))
        .child(back)
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .child(div().font_family(th.display.clone()).font_weight(FontWeight::BOLD).text_size(px(th.dsize(22.))).text_color(th.lime).child(b.title.clone()))
                .child(kit::muted(format!("{} · {} · {} · {} · {}", if b.is_bench() { "BENCHMARK" } else { "PARTIDA" }, fmt::datetime(b.date), fmt::duration(b.seconds), b.exe, b.api), cx)),
        )
        .when(b.is_bench() && detail.as_ref().is_some_and(|d| d.frametimes.is_some()), |d| d.child(kit::button("s-csv", "CSV", th.cyan, cx).on_click(cx.listener(move |this, _, _, cx| export_csv(this, bb.clone(), cx)))))
        .child(kit::button("s-del", if root.games.confirm_delete.as_deref() == Some(&b.id) { "¿Borrar? Confirmar" } else { "Borrar" }, th.red, cx).on_click(cx.listener(move |this, _, _, cx| {
            if this.games.confirm_delete.as_deref() == Some(&del_id) {
                let id = del_id.clone();
                this.state.update(cx, |s, _| s.model.games.remove(&id));
                this.games.viewing = None;
                this.games.confirm_delete = None;
            } else {
                this.games.confirm_delete = Some(del_id.clone());
            }
            cx.notify();
        })));
    let stats = vec![
        kit::stat("Media", f1(b.avg), format!("{} fotogramas", b.frames), th.cyan, None, cx).into_any_element(),
        kit::stat("1 % bajo", f1(b.low1), detail.as_ref().map(|d| format!("{} ms", f1(d.pct.p99))).unwrap_or_default(), th.amber, None, cx).into_any_element(),
        kit::stat("0,1 % bajo", f1(b.low01), detail.as_ref().map(|d| format!("{} ms", f1(d.pct.p999))).unwrap_or_default(), th.orange, None, cx).into_any_element(),
        kit::stat("Mín / máx", format!("{:.0} / {:.0}", b.min_fps, b.max_fps), "FPS por segundo", th.violet, None, cx).into_any_element(),
        kit::stat("Tirones", b.stutters.to_string(), format!("{:.1}/min · {} parones", if b.seconds > 0.0 { b.stutters as f64 / b.seconds * 60.0 } else { 0.0 }, b.hitches), if b.hitches > 0 { th.red } else { th.magenta }, None, cx).into_any_element(),
        kit::stat("Ritmo", format!("±{:.2} ms", b.jitter_ms), "variación entre fotogramas", th.teal, None, cx).into_any_element(),
        kit::stat("Estabilidad", format!("{:.0}", b.stability), label, th.slot(lc), None, cx).into_any_element(),
        kit::stat("CPU / GPU", format!("{:.0} / {:.0} %", b.cpu, b.gpu), format!("{:.0} W · máx {:.0}/{:.0} °C", b.gpu_power, b.cpu_temp_max, b.gpu_temp_max), th.lime, None, cx).into_any_element(),
    ];
    let mut g = kit::grid(cx).child(head).child(kit::stats_row(stats, cx));
    match detail {
        None => {
            g = g.child(Panel::new("Sin datos detallados", th.muted).span(4).child(kit::muted("Este registro se guardó con una versión anterior de NeonTop, que solo guardaba el resumen. Las partidas nuevas se guardan completas.", cx)));
        }
        Some(d) => {
            let xs = d.t.clone();
            let xy = |lines: Vec<Line>| Plot::new(lines, xs.clone()).x_axis(XAxis::Value(" s"));
            let (bins, bin) = hist_bins(&d.hist.counts, d.hist.step, d.pct.p999, d.pct.median, &th);
            g = g
                .child(Panel::new("FPS durante la partida", th.lime).span(4).extra(kit::legend(vec![("FPS".into(), th.lime), ("1 % bajo (5 s)".into(), th.amber)])).child(xy(vec![Line::new(d.fps.clone(), th.lime).fill(), Line::new(d.low1.clone(), th.amber).width(1.3)]).y_fmt(y_fps).height(240.).into_element()))
                .child(Panel::new("Peor fotograma", th.red).span(2).extra("ms · los picos son tirones").child(xy(vec![Line::new(d.ft_max.clone(), th.red).fill().width(1.2)]).y_fmt(y_ms).height(200.).into_element()))
                .child(Panel::new("CPU y GPU", th.magenta).span(2).extra(kit::legend(vec![("GPU".into(), th.magenta), ("CPU".into(), th.cyan)])).child(xy(vec![Line::new(d.gpu.clone(), th.magenta), Line::new(d.cpu.clone(), th.cyan).width(1.3)]).y_max(100.).y_fmt(y_pct).height(200.).into_element()))
                .child(Panel::new("Temperaturas", th.amber).span(2).extra(kit::legend(vec![("GPU".into(), th.amber), ("CPU".into(), th.teal)])).child(xy(vec![Line::new(d.gpu_temp.clone(), th.amber), Line::new(d.cpu_temp.clone(), th.teal).width(1.3)]).y_fmt(super::y_deg).height(180.).into_element()))
                .child(Panel::new("Potencia de la GPU", th.orange).span(2).child(xy(vec![Line::new(d.gpu_power.clone(), th.orange).fill()]).y_fmt(super::y_watts).height(180.).into_element()))
                .child(Panel::new("Distribución de tiempos", th.lime).span(2).extra("escala logarítmica · rojo = tirón").child(charts::histogram(bins, bin, " ms", 220.)))
                .child(Panel::new("FPS por percentil", th.amber).span(2).child(pct_rows(b.avg, &d.pct, &th, cx)));
        }
    }
    if siblings.len() > 1 {
        let mut sib = siblings.clone();
        sib.sort_by_key(|x| x.date);
        let cols: Vec<(String, f32, Hsla)> = sib.iter().map(|x| (fmt::datetime(x.date)[..5].to_string(), x.avg, if x.id == b.id { th.lime } else { fade(th.cyan, 0.6) })).collect();
        let max = cols.iter().map(|c| c.1).fold(1.0, f32::max);
        g = g.child(
            Panel::new(if b.is_bench() { "Benchmarks de este juego" } else { "Partidas de este juego" }, th.cyan)
                .span(4)
                .extra("FPS medios · la resaltada es esta")
                .child(charts::columns(cols, charts::nice_max(max), vec![], y_fps, 200.)),
        );
    }
    g.into_any_element()
}

pub fn render(root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    if let Some(id) = root.games.viewing.clone() {
        return session_view(root, id, cx);
    }
    let th = Th::get(cx).clone();
    let st = root.state.read(cx);
    let games = &st.model.games;
    let Some(tr) = games.current() else {
        return kit::grid(cx).child(help(root, cx)).child(history(root, cx)).into_any_element();
    };
    let s = st.model.snap.clone();
    let t = ht(st);
    let pid = tr.pid;
    let info = tr.info.clone();
    let sess = &tr.session;
    let (count, avg, low1, low01) = (sess.count, sess.avg_fps(), sess.low1(), sess.low01());
    let (p99, p999) = (sess.percentile(99.0), sess.percentile(99.9));
    let (stutters, hitches, jitter, stability, seconds) = (sess.stutters, sess.hitches, sess.jitter_ms(), sess.stability(), sess.seconds());
    let (label, lc) = stability_label(stability, count);
    let median = tr.median(120);
    let ended = tr.ended;
    let rec = tr.rec.as_ref().map(|r| (r.until, r.start));
    let now = s.ts as f64 / 1000.0;
    let (fx, fy) = tr.last_frames(10.0);
    let min_fps = tr.min_fps;
    let max_fps = tr.max_fps;
    let pct = Percentiles {
        p50: sess.percentile(50.0),
        p90: sess.percentile(90.0),
        p95: sess.percentile(95.0),
        p99,
        p999,
        max: sess.max,
        median: sess.percentile(50.0),
    };
    let hist_counts = sess.grouped(0.25, 100.0);
    let live_ft = if info.fps > 0.0 { 1000.0 / info.fps } else { 0.0 };
    let list: Vec<(u32, String, bool, f32, String, bool)> = games.list().into_iter().map(|g| (g.pid, g.info.title.clone(), g.info.active, g.info.fps, g.info.api.clone(), g.ended.is_some())).collect();
    let last_bench = games.last_bench.clone().filter(|b| b.exe == info.exe);
    let prev = last_bench.as_ref().and_then(|b| games.previous(b).cloned());
    let proc = st.model.procs.get(pid).cloned();
    let gpu_procs: Vec<_> = s.gpus.iter().flat_map(|g| g.processes.iter().filter(|p| p.pid == pid).cloned()).collect();
    let vram: u64 = gpu_procs.iter().map(|p| p.vram).sum();
    let gpu_use: f32 = gpu_procs.iter().map(|p| p.engines.iter().map(|e| e.busy).fold(0.0, f32::max)).sum();
    let fps_hist = hv(st, &format!("game.{pid}.fps"));
    let low_hist = hv(st, &format!("game.{pid}.low"));
    let gbusy = hv(st, "gpu.0.busy");
    let cbusy = hv(st, "cpu.usage");

    // qué limita los FPS ahora
    let verdict: Option<(String, String, Hsla)> = (info.active && count >= 60).then(|| {
        let g = main_gpu(&s.gpus);
        let gpu = g.and_then(|g| g.busy).unwrap_or(0.0);
        let max_core = s.cpu.cores.iter().map(|c| c.usage).fold(0.0, f32::max);
        let (_, y) = tr.last_frames(3.0);
        let mean = y.iter().sum::<f32>() / y.len().max(1) as f32;
        let cv = (y.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / y.len().max(1) as f32).sqrt() / mean.max(0.01);
        if gpu >= 92.0 {
            ("Limitado por la GPU".into(), format!("GPU al {gpu:.0} %: la tarjeta gráfica es el techo. Bajar resolución o calidad gráfica sube los FPS."), th.magenta)
        } else if cv < 0.06 && gpu < 85.0 {
            (format!("Tope de FPS / sincronía vertical (~{:.0} FPS)", 1000.0 / mean.max(0.01)), format!("Ritmo muy constante con la GPU al {gpu:.0} %: hay un límite de FPS o VSync activo."), th.lime)
        } else if max_core >= 88.0 && gpu < 85.0 {
            ("Limitado por la CPU".into(), format!("Un núcleo al {max_core:.0} % con la GPU al {gpu:.0} %: el hilo principal del juego no da más."), th.cyan)
        } else {
            ("Sin un límite claro".into(), format!("GPU {gpu:.0} % · núcleo más cargado {max_core:.0} %. Puede influir el motor, la red o el disco."), th.amber)
        }
    });
    let _ = st;

    let picker = (list.len() > 1).then(|| {
        div().col_span(kit::cols(cx) as u16).flex().flex_wrap().gap(px(8.)).children(list.iter().map(|(gpid, title, active, fps, api, ended)| {
            let gp = *gpid;
            let on = gp == pid;
            div()
                .id(SharedString::from(format!("game-{gp}")))
                .flex()
                .flex_col()
                .px(px(12.))
                .py(px(6.))
                .cursor_pointer()
                .border_1()
                .border_color(if on { th.lime } else { fade(th.lime, 0.3) })
                .when(on, |d| d.bg(fade(th.lime, 0.14)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.state.update(cx, |s, _| s.model.games.selected = Some(gp));
                    cx.notify();
                }))
                .child(div().font_weight(FontWeight::BOLD).child(title.clone()))
                .child(kit::muted(format!("{} · {}", if *ended { "terminado".into() } else if *active { format!("{fps:.0} FPS") } else { "en pausa".into() }, api_name(api)), cx))
        }))
    });

    let rec_opts: Vec<(Option<u32>, String)> = vec![(Some(30), "30 s".into()), (Some(60), "60 s".into()), (Some(120), "2 min".into()), (Some(300), "5 min".into()), (None, "manual".into())];
    let actions = if ended.is_some() {
        div().flex().items_center().gap(px(8.)).child(kit::muted("Resumen final de la partida · guardado en el historial", cx)).child(kit::button("g-close", "Cerrar", th.cyan, cx).on_click(cx.listener(move |this, _, _, cx| {
            this.state.update(cx, |s, cx| {
                s.model.games.dismiss(pid);
                cx.notify();
            });
        })))
    } else if let Some((until, start)) = rec {
        let txt = match until {
            Some(u) => format!("● GRABANDO · quedan {:.0} s", (u - now).max(0.0)),
            None => format!("● GRABANDO · {:.0} s", now - start),
        };
        div().flex().items_center().gap(px(8.)).child(div().text_color(th.red).font_weight(FontWeight::BOLD).child(txt)).child(kit::button_solid("g-stop", "Detener", th.red, cx).on_click(cx.listener(move |this, _, _, cx| {
            this.state.update(cx, |s, cx| {
                let ts = s.model.snap.ts;
                s.model.games.stop(pid, ts);
                cx.notify();
            });
        })))
    } else {
        let secs = root.games.rec_secs;
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(kit::segmented("rec", rec_opts, &secs, cx, {
                let view = cx.entity().downgrade();
                move |v, _, cx| {
                    let v = *v;
                    let _ = view.update(cx, |this, cx| {
                        this.games.rec_secs = v;
                        cx.notify();
                    });
                }
            }))
            .child(kit::button("g-rec", "● Grabar benchmark", th.red, cx).on_click(cx.listener(move |this, _, _, cx| {
                let secs = this.games.rec_secs.map(|s| s as f64);
                this.state.update(cx, |s, cx| {
                    let now = s.model.snap.ts as f64 / 1000.0;
                    s.model.games.start(pid, secs, now);
                    cx.notify();
                });
            })))
            .child(kit::button("g-reset", "Reiniciar", th.cyan, cx).on_click(cx.listener(move |this, _, _, cx| {
                this.state.update(cx, |s, cx| {
                    if let Some(t) = s.model.games.tracks.get_mut(&pid) {
                        t.reset_session();
                    }
                    cx.notify();
                });
            })))
    };
    let head = div()
        .col_span(kit::cols(cx) as u16)
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(10.))
        .child(
            div()
                .flex()
                .flex_col()
                .child(div().font_family(th.display.clone()).font_weight(FontWeight::BOLD).text_size(px(th.dsize(22.))).text_color(th.lime).child(info.title.clone()))
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(kit::muted(
                            format!(
                                "{} · PID {pid} · {} · {} bits{} · {}",
                                info.exe,
                                api_name(&info.api),
                                info.bits,
                                info.steam_app_id.map(|a| format!(" · Steam {a}")).unwrap_or_default(),
                                fmt::duration(info.duration)
                            ),
                            cx,
                        ))
                        .when(ended.is_some(), |d| d.child(kit::badge(format!("SESIÓN TERMINADA · {}", fmt::clock(ended.unwrap_or(0))), th.cyan)))
                        .when(ended.is_none() && !info.active, |d| d.child(kit::badge("EN PAUSA / MINIMIZADO", th.amber))),
                ),
        )
        .child(actions);
    let stats = vec![
        kit::stat("FPS", if info.active { format!("{:.0}", info.fps) } else { "—".into() }, format!("{} ms por fotograma", f1(live_ft)), th.lime, Some(fps_hist.clone()), cx).into_any_element(),
        kit::stat("Media", f1(avg), format!("{count} fotogramas"), th.cyan, None, cx).into_any_element(),
        kit::stat("1 % bajo", f1(low1), format!("{} ms", f1(p99)), th.amber, Some(low_hist.clone()), cx).into_any_element(),
        kit::stat("0,1 % bajo", f1(low01), format!("{} ms", f1(p999)), th.orange, None, cx).into_any_element(),
        kit::stat("Mín / máx", format!("{} / {}", if min_fps.is_finite() { format!("{min_fps:.0}") } else { "—".into() }, if max_fps > 0.0 { format!("{max_fps:.0}") } else { "—".into() }), "FPS por segundo", th.violet, None, cx).into_any_element(),
        kit::stat("Tirones", stutters.to_string(), format!("{:.1}/min · {hitches} parones", if seconds > 0.0 { stutters as f64 / seconds * 60.0 } else { 0.0 }), if hitches > 0 { th.red } else { th.magenta }, None, cx).into_any_element(),
        kit::stat("Ritmo", format!("±{jitter:.2} ms"), "variación entre fotogramas", th.teal, None, cx).into_any_element(),
        kit::stat("Estabilidad", if count >= 30 { format!("{stability:.0}") } else { "—".into() }, if count >= 30 { label } else { "midiendo…" }, th.slot(lc), None, cx).into_any_element(),
    ];
    // tiempos de fotograma de los últimos 10 s, con los tirones marcados
    let stutter_dots: Vec<(f64, f32, Hsla)> = fx.iter().zip(&fy).filter(|(_, y)| median > 0.0 && **y > median * 2.0).map(|(x, y)| (*x, *y, th.red)).collect();
    let ymax = fy.iter().copied().fold(0.0f32, f32::max).max(1000.0 / 30.0 * 1.15);
    let frames = Plot::new(vec![Line::new(fy, th.lime).width(1.2)], fx)
        .x_axis(XAxis::Secs)
        .x_range(-10.0, 0.0)
        .y_max(charts::nice_max(ymax))
        .y_fmt(y_ms)
        .hline(1000.0 / 60.0, th.cyan, "60 FPS")
        .hline(1000.0 / 30.0, th.amber, "30 FPS")
        .dots(stutter_dots)
        .height(230.);
    let (bins, bin) = hist_bins(&hist_counts, 0.25, p999, median, &th);
    let mut g = kit::grid(cx);
    if let Some(p) = picker {
        g = g.child(p);
    }
    g = g
        .child(head)
        .child(kit::stats_row(stats, cx))
        .child(Panel::new("Tiempos de fotograma", th.lime).span(4).extra(kit::legend(vec![(format!("últimos 10 s · mediana {median:.2} ms"), th.muted), ("● tirón (> 2× mediana)".into(), th.red)])).child(frames.into_element()))
        .child(
            Panel::new("FPS durante la sesión", th.lime)
                .span(3)
                .extra(kit::legend(vec![("FPS".into(), th.lime), (format!("1 % bajo ({LIVE_WINDOW:.0} s)"), th.amber)]))
                .child(Plot::new(vec![Line::new(fps_hist, th.lime).fill(), Line::new(low_hist, th.amber).width(1.4)], t.clone()).y_fmt(y_fps).height(220.).into_element()),
        )
        .child(
            Panel::new("Estabilidad", th.slot(lc))
                .child(charts::gauge(stability / 100.0, th.slot(lc), if count >= 30 { format!("{stability:.0}") } else { "—".into() }, label.into(), "ESTABILIDAD".into(), 170.))
                .child(kit::kv_grid(
                    vec![
                        ("Constancia".into(), if avg > 0.0 { format!("{:.0} % (1 % bajo / media)", low1 / avg * 100.0) } else { "—".into() }),
                        ("Ritmo".into(), format!("±{jitter:.2} ms")),
                        ("Parones".into(), format!("{hitches} (> 50 ms)")),
                    ],
                    false,
                    cx,
                )),
        )
        .child(
            Panel::new("Qué limita los FPS", verdict.as_ref().map(|v| v.2).unwrap_or(th.muted))
                .span(2)
                .extra(kit::legend(vec![("GPU".into(), th.magenta), ("CPU".into(), th.cyan)]))
                .child(match &verdict {
                    Some((txt, detail, c)) => div().flex().flex_col().gap(px(4.)).child(div().text_size(px(16.)).font_weight(FontWeight::BOLD).text_color(*c).child(txt.clone())).child(kit::muted(detail.clone(), cx)),
                    None => div().child(kit::muted(if ended.is_some() { "El juego se ha cerrado." } else if info.active { "Midiendo…" } else { "El juego está en pausa o minimizado." }, cx)),
                })
                .child(Plot::new(vec![Line::new(gbusy, th.magenta), Line::new(cbusy, th.cyan).width(1.4)], t.clone()).y_max(100.).y_fmt(y_pct).height(150.).into_element()),
        )
        .child(
            Panel::new("Proceso del juego", th.violet).span(2).child(match &proc {
                Some(p) => kit::kv_grid(
                    vec![
                        ("CPU".into(), format!("{:.0} % (de un núcleo)", p.cpu)),
                        ("Hilos".into(), p.threads.to_string()),
                        ("RAM".into(), fmt::bytes(p.mem as f64)),
                        ("VRAM".into(), if vram > 0 { fmt::bytes(vram as f64) } else { "—".into() }),
                        ("GPU".into(), if gpu_use > 0.0 { format!("{gpu_use:.0} %") } else { "—".into() }),
                        ("Prioridad".into(), format!("nice {}", p.nice)),
                    ],
                    true,
                    cx,
                )
                .into_any_element(),
                None => kit::muted(format!("No se ve el proceso {pid} (puede estar en otro espacio de nombres, p. ej. dentro de un contenedor de Steam)."), cx).into_any_element(),
            }).children(s.gpus.first().map(|gpu| {
                kit::kv_grid(
                    vec![
                        ("Reloj GPU".into(), gpu.gfx_clock.map(|c| format!("{c:.0} MHz")).unwrap_or("—".into())),
                        ("Potencia".into(), gpu.power.map(|p| format!("{p:.0} W")).unwrap_or("—".into())),
                        ("Temp. GPU".into(), gpu.temp_hotspot.or(gpu.temp_edge).map(|t| format!("{t:.0} °C")).unwrap_or("—".into())),
                        ("Temp. CPU".into(), s.cpu.temp.map(|t| format!("{t:.0} °C")).unwrap_or("—".into())),
                    ],
                    true,
                    cx,
                )
            })),
        )
        .child(Panel::new("Distribución de tiempos", th.lime).span(2).extra("escala logarítmica · rojo = tirón").child(charts::histogram(bins, bin, " ms", 220.)))
        .child(Panel::new("FPS por percentil", th.amber).span(2).extra("sesión completa").child(pct_rows(avg, &pct, &th, cx)));

    if let Some(b) = last_bench {
        let delta = |a: f32, p: Option<f32>| p.filter(|p| *p > 0.0).map(|p| (a - p) / p * 100.0);
        let items: Vec<(&str, f32, Option<f32>)> = vec![
            ("Media", b.avg, prev.as_ref().map(|p| p.avg)),
            ("1 % bajo", b.low1, prev.as_ref().map(|p| p.low1)),
            ("0,1 % bajo", b.low01, prev.as_ref().map(|p| p.low01)),
            ("Estabilidad", b.stability, prev.as_ref().map(|p| p.stability)),
        ];
        let tile = |name: String, v: String, sub: Option<(String, Hsla)>| {
            div()
                .flex()
                .flex_col()
                .min_w(px(110.))
                .child(kit::muted(th.title(&name), cx).text_size(px(9.5)))
                .child(div().font_family(th.display.clone()).font_weight(FontWeight::BOLD).text_size(px(th.dsize(20.))).child(v))
                .children(sub.map(|(s, c)| div().text_size(px(11.)).text_color(c).child(s)))
        };
        g = g.child(
            Panel::new("Último benchmark", th.red)
                .span(4)
                .extra(format!("{} · {:.0} s", fmt::datetime(b.date), b.seconds))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(24.))
                        .children(items.into_iter().map(|(n, v, p)| tile(n.into(), format!("{v:.1}"), delta(v, p).map(|d| (format!("{} {:.1} %", if d > 0.0 { "▲" } else { "▼" }, d.abs()), if d > 0.0 { th.lime } else { th.red })))))
                        .child(tile("Tirones".into(), b.stutters.to_string(), Some((format!("{} parones", b.hitches), th.muted))))
                        .child(tile("GPU / CPU".into(), format!("{:.0} / {:.0} %", b.gpu, b.cpu), Some((format!("{:.0} W", b.gpu_power), th.muted)))),
                )
                .children(prev.map(|p| kit::muted(format!("Comparado con la grabación anterior de este juego ({}).", fmt::datetime(p.date)), cx))),
        );
    }
    g.child(history(root, cx)).child(Panel::new("Medición automática", th.lime).span(4).child(fps_auto(root, cx))).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_bins() {
        let th = Th::new(neontop_core::theme::resolve("neon", true), true, true);
        let mut counts = vec![0u32; 400];
        counts[40] = 100; // 10 ms
        counts[200] = 1; // 50 ms
        // mediana 5 ms: los cubos de más de 10 ms son tirones
        let (bins, bin) = hist_bins(&counts, 0.25, 12.0, 5.0, &th);
        assert_eq!(bin, 0.5);
        assert_eq!(bins.len(), 40, "hasta 20 ms en cubos de 0,5 ms");
        assert_eq!(bins.iter().map(|b| b.0).sum::<f32>(), 101.0, "lo que pasa del eje cae en el último cubo");
        assert_eq!(bins[20].0, 100.0);
        assert_eq!(bins[39].1, th.red, "más del doble de la mediana es tirón");
        assert_eq!(bins[20].1, th.lime, "10 ms no supera el doble de la mediana");
        assert_eq!(bins[21].1, th.red);
    }
}
