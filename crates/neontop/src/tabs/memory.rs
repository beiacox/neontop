//! Memoria: reparto de la RAM, historial, mapa de celdas, desglose del kernel, swap, presión,
//! tráfico de la RAM en vivo (contadores del controlador de memoria) y test de ancho de banda.

use gpui::{div, prelude::*, px, AnyElement, Context, Window};
use neontop_core::format as fmt;
use neontop_core::history::mem_segments;

use super::{ht, hv, y_bytes, y_gbs};
use crate::app::Root;
use crate::charts::{self, Line, Plot};
use crate::state::Notice;
use crate::ui::kit::{self, Panel};
use crate::ui::theme::Th;

pub fn render(root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    let th = Th::get(cx).clone();
    let st = root.state.read(cx);
    let s = st.model.snap.clone();
    let m = &s.mem;
    let t = ht(st);
    let seg = mem_segments(m);
    let total = m.total.max(1) as f64;
    let used_pct = seg.used as f64 / total * 100.0;
    let swap_used = m.swap_total.saturating_sub(m.swap_free);
    let commit_pct = m.committed_as as f64 / m.commit_limit.max(1) as f64 * 100.0;
    let parts = [("Apps", seg.apps, th.magenta), ("Compartida", seg.shmem, th.violet), ("Buffers", seg.buffers, th.amber), ("Caché", seg.cache, th.cyan), ("Libre", seg.free, th.dim)];
    let stats = vec![
        kit::stat("En uso", fmt::bytes(seg.used as f64), fmt::pct(used_pct, 1), th.magenta, Some(hv(st, "mem.apps")), cx).into_any_element(),
        kit::stat("Disponible", fmt::bytes(m.available as f64), format!("de {}", fmt::bytes(total)), th.cyan, None, cx).into_any_element(),
        kit::stat("Caché", fmt::bytes(seg.cache as f64), "", th.cyan, Some(hv(st, "mem.cache")), cx).into_any_element(),
        kit::stat("Buffers", fmt::bytes(m.buffers as f64), "", th.amber, Some(hv(st, "mem.buffers")), cx).into_any_element(),
        kit::stat("Swap", fmt::bytes(swap_used as f64), if m.swap_total > 0 { format!("de {}", fmt::bytes(m.swap_total as f64)) } else { "sin swap".into() }, th.violet, Some(hv(st, "mem.swap")), cx)
            .into_any_element(),
        kit::stat("Dirty", fmt::bytes(m.dirty as f64), format!("writeback {}", fmt::bytes(m.writeback as f64)), th.lime, Some(hv(st, "mem.dirty")), cx).into_any_element(),
        kit::stat("Zswap", fmt::bytes(m.zswap as f64), if m.zswapped > 0 { format!("comprime {}", fmt::bytes(m.zswapped as f64)) } else { "inactivo".into() }, th.teal, None, cx).into_any_element(),
        kit::stat("Comprometida", fmt::pct(commit_pct, 0), format!("{} / {}", fmt::bytes(m.committed_as as f64), fmt::bytes(m.commit_limit as f64)), th.orange, None, cx).into_any_element(),
    ];
    let hist = vec![
        Line::new(hv(st, "mem.apps"), th.magenta).fill(),
        Line::new(hv(st, "mem.shmem"), th.violet).fill(),
        Line::new(hv(st, "mem.buffers"), th.amber).fill(),
        Line::new(hv(st, "mem.cache"), th.cyan).fill(),
    ];
    let swap_lines = vec![Line::new(hv(st, "mem.swap"), th.violet).fill(), Line::new(hv(st, "mem.dirty"), th.lime), Line::new(hv(st, "mem.writeback"), th.amber).dashed()];
    let psi_lines = vec![Line::new(hv(st, "psi.mem"), th.amber).fill(), Line::new(hv(st, "psi.memFull"), th.red)];
    // desglose del kernel en escala logarítmica (lo pequeño también se ve)
    let kernel = [
        ("Activa (anon)", m.active_anon, th.magenta),
        ("Inactiva (anon)", m.inactive_anon, th.violet),
        ("Activa (ficheros)", m.active_file, th.cyan),
        ("Inactiva (ficheros)", m.inactive_file, th.teal),
        ("Mapeada", m.mapped, th.blue),
        ("Slab", m.slab, th.amber),
        ("Tablas de páginas", m.page_tables, th.orange),
        ("Pilas del kernel", m.kernel_stack, th.red),
    ];
    let lmax = (total).log10();
    let kernel_rows = div().flex().flex_col().gap(px(6.)).children(kernel.into_iter().map(|(l, v, c)| {
        let f = if v > 0 { ((v as f64).log10() / lmax).clamp(0.0, 1.0) as f32 } else { 0.0 };
        kit::meter_row(l, f, fmt::bytes(v as f64), c, cx)
    }));
    let swap_pct = if m.swap_total > 0 { swap_used as f32 / m.swap_total as f32 * 100.0 } else { 0.0 };

    // tráfico de RAM en vivo
    let bw = s.membw.clone();
    let bw_raw = st.model.history.raw("membw.total", st.window()).to_vec();
    let (bw_avg, bw_max) = if bw_raw.is_empty() { (0.0, 0.0) } else { (bw_raw.iter().map(|&x| x as f64).sum::<f64>() / bw_raw.len() as f64, bw_raw.iter().copied().fold(0.0f32, f32::max) as f64) };
    let bench = st.membench.clone();
    let bench_running = st.membench_running;
    let peak = bench.last().map(|b| b.read.max(b.copy));
    let split = bw.read.is_some() && bw.write.is_some() && bw.source.as_deref() != Some("amd_df");
    let bw_lines = if split {
        vec![Line::new(hv(st, "membw.read"), th.cyan).fill(), Line::new(hv(st, "membw.write"), th.magenta).fill()]
    } else {
        vec![Line::new(hv(st, "membw.total"), th.cyan).fill()]
    };
    let notice = st.notice.clone();
    let demo = st.demo;
    let _ = st;

    let live: AnyElement = if bw.status == "ok" {
        let num = |label: &str, value: String, sub: Option<String>| {
            div()
                .flex()
                .flex_col()
                .child(kit::muted(th.title(label), cx).text_size(px(9.5)))
                .child(div().font_family(th.display.clone()).text_size(px(th.dsize(18.))).text_color(th.cyan).child(value))
                .children(sub.map(|s| kit::muted(s, cx)))
        };
        let mut p = Plot::new(bw_lines, t.clone()).y_fmt(y_gbs).height(190.);
        if split {
            p = p.stacked();
        }
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(28.))
                    .child(num(
                        "ahora",
                        fmt::gbs(bw.total.unwrap_or(0.0), 1),
                        split.then(|| format!("▼ {}  ▲ {}", fmt::gbs(bw.read.unwrap_or(0.0), 1), fmt::gbs(bw.write.unwrap_or(0.0), 1))),
                    ))
                    .child(num("media", fmt::gbs(bw_avg, 1), None))
                    .child(num("pico", fmt::gbs(bw_max, 1), None))
                    .child(num(
                        "del máximo",
                        peak.map(|p| format!("{:.0} %", (bw.total.unwrap_or(0.0) / p * 100.0).min(100.0))).unwrap_or("—".into()),
                        Some(peak.map(|p| format!("test: {}", fmt::gbs(p, 1))).unwrap_or("ejecuta el test".into())),
                    )),
            )
            .child(p.into_element())
            .child(kit::muted(
                if bw.source.as_deref() == Some("amd_df") {
                    "Contadores del Data Fabric de AMD: tráfico real entre el controlador de memoria y la DRAM (lectura y escritura juntas; este procesador no las separa)."
                } else {
                    "Contadores del controlador de memoria de Intel (uncore_imc)."
                },
                cx,
            ))
            .when(bw.enabled && !demo, |d| {
                d.child(kit::button("membw-off", "Desactivar", th.muted, cx).on_click(cx.listener(|this, _, _, cx| membw_access(this, false, cx))))
            })
            .into_any_element()
    } else {
        let why = match bw.status.as_str() {
            "nomodule" => "Falta cargar el módulo del kernel que expone los contadores del controlador de memoria (amd_uncore).".to_string(),
            "noperm" => "Leer los contadores de todo el sistema requiere un permiso de administrador (CAP_PERFMON).".to_string(),
            "nopmu" => "El controlador de memoria no expone contadores en este sistema.".to_string(),
            "unsupported" => "Este procesador no tiene contadores de tráfico de memoria compatibles (soportados: AMD Zen 2/Zen 3 e Intel con uncore_imc).".to_string(),
            "missing" => "No se encuentra el lector de contadores (neontop-membw); reinstala el paquete de NeonTop.".to_string(),
            "starting" => "Iniciando…".to_string(),
            _ => bw.message.clone().unwrap_or(bw.status.clone()),
        };
        let fixable = bw.status == "nomodule" || bw.status == "noperm";
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().child(why))
            .when(fixable, |d| {
                d.child(kit::button_solid("membw-on", "Activar (pide contraseña)", th.cyan, cx).w(px(220.)).on_click(cx.listener(|this, _, _, cx| membw_access(this, true, cx))))
                    .child(kit::muted(
                        "Se hará como administrador, una sola vez: cargar el módulo amd_uncore (y dejarlo para cada arranque en /etc/modules-load.d/neontop-membw.conf) y dar a /usr/lib/neontop/neontop-membw el permiso CAP_PERFMON, que solo permite leer contadores de rendimiento. NeonTop no se ejecuta como root.",
                        cx,
                    ))
            })
            .into_any_element()
    };

    let bench_view = {
        let r = bench.last().cloned();
        let label = if bench_running { "Midiendo…" } else if r.is_some() { "↻ Repetir test" } else { "▶ Ejecutar test" };
        let mut col = div().flex().flex_col().gap(px(10.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(14.))
                .child(kit::button_solid("bench-run", label, th.cyan, cx).w(px(150.)).on_click(cx.listener(|this, _, _, cx| this.state.update(cx, |s, cx| s.run_membench(cx)))))
                .child(kit::muted(
                    format!(
                        "Mide el máximo que da tu RAM leyendo, escribiendo y copiando {} con todos los hilos (~1–2 s de carga). El uso real en cada momento está en «Tráfico de RAM en vivo».",
                        r.as_ref().map(|r| fmt::bytes_digits(r.buffer_bytes as f64, 0)).unwrap_or("512 MiB".into())
                    ),
                    cx,
                )),
        );
        if let Some(r) = r {
            let bars = [
                ("Lectura", r.read, th.cyan, format!("{} hilos", r.threads)),
                ("Escritura", r.write, th.magenta, format!("{} hilos", r.threads)),
                ("Copia", r.copy, th.violet, "lectura + escritura".into()),
                ("Lectura 1 hilo", r.read_single, th.teal, "un solo núcleo".into()),
            ];
            let top = bars.iter().map(|b| b.1).fold(1.0, f64::max);
            col = col.child(
                div()
                    .flex()
                    .gap(px(24.))
                    .child(div().flex_1().flex().flex_col().gap(px(8.)).children(bars.into_iter().map(|(l, v, c, hint)| kit::meter_row_w(format!("{l} · {hint}"), 190., (v / top) as f32, fmt::gbs(v, 1), c, cx))))
                    .child(
                        div()
                            .w(px(170.))
                            .flex()
                            .flex_col()
                            .child(kit::muted("LATENCIA", cx).text_size(px(9.5)))
                            .child(div().font_family(th.display.clone()).text_size(px(th.dsize(22.))).text_color(th.amber).child(format!("{:.1} ns", r.latency_ns)))
                            .child(kit::muted("acceso aleatorio a memoria", cx))
                            .child(kit::muted(format!("{} · {:.1} s", fmt::datetime(r.ts), r.duration_ms / 1000.0), cx)),
                    ),
            );
            if bench.len() > 1 {
                let xs: Vec<f64> = (0..bench.len()).map(|i| i as f64).collect();
                col = col.child(
                    Plot::new(
                        vec![
                            Line::new(bench.iter().map(|b| b.read as f32).collect(), th.cyan),
                            Line::new(bench.iter().map(|b| b.write as f32).collect(), th.magenta),
                            Line::new(bench.iter().map(|b| b.copy as f32).collect(), th.violet),
                        ],
                        xs,
                    )
                    .x_axis(charts::XAxis::None)
                    .y_fmt(y_gbs)
                    .height(120.)
                    .into_element(),
                );
            }
        }
        col
    };

    kit::grid(cx)
        .child(kit::stats_row(stats, cx))
        .child(Panel::new("Composición", th.magenta).child(charts::ring(
            parts.iter().filter(|p| p.0 != "Libre").map(|(_, v, c)| (*v as f32 / total as f32, *c)).collect(),
            fmt::pct(used_pct, 0),
            fmt::bytes_of(seg.used as f64, total),
            250.,
        )))
        .child(
            Panel::new("Historial de memoria", th.magenta)
                .span(3)
                .extra(kit::legend(parts[..4].iter().map(|(l, v, c)| (format!("{} {}", l.to_lowercase(), fmt::bytes(*v as f64)), *c)).collect()))
                .child(Plot::new(hist, t.clone()).stacked().y_max(total as f32).y_fmt(y_bytes).bytes().height(250.).into_element()),
        )
        .child(
            Panel::new("Mapa de RAM", th.cyan)
                .span(2)
                .extra(format!("cada celda ≈ {}", fmt::bytes(total / 200.0)))
                .child(charts::waffle(parts.iter().map(|(_, v, c)| (*v as f32 / total as f32, *c)).collect(), 200.))
                .child(kit::legend(parts.iter().map(|(l, v, c)| (format!("{l} {}", fmt::bytes(*v as f64)), *c)).collect())),
        )
        .child(Panel::new("Desglose del kernel", th.amber).span(2).extra("escala logarítmica").child(kernel_rows))
        .child(Panel::new("Swap", th.violet).child(charts::gauge(
            swap_pct / 100.0,
            th.violet,
            if m.swap_total > 0 { fmt::pct(swap_pct as f64, 1) } else { "N/D".into() },
            format!("caché {}", fmt::bytes(m.swap_cached as f64)),
            "SWAP".into(),
            210.,
        )))
        .child(
            Panel::new("Swap y escritura pendiente", th.violet)
                .span(2)
                .extra(kit::legend(vec![("swap".into(), th.violet), ("dirty".into(), th.lime), ("writeback".into(), th.amber)]))
                .child(Plot::new(swap_lines, t.clone()).y_fmt(y_bytes).bytes().height(200.).into_element()),
        )
        .child(Panel::new("Presión de memoria", th.red).extra("PSI avg10").child(Plot::new(psi_lines, t.clone()).y_fmt(|v| format!("{v:.1}%")).height(200.).into_element()))
        .child(Panel::new("Tráfico de RAM en vivo", th.cyan).span(4).extra("contadores del controlador de memoria").child(live).children(notice_line(notice, cx)))
        .child(Panel::new("Ancho de banda de RAM", th.cyan).span(4).extra("test bajo demanda").child(bench_view))
        .into_any_element()
}

/// Mensaje de la última acción (permisos…).
pub fn notice_line(n: Option<Notice>, cx: &gpui::App) -> Option<AnyElement> {
    let th = Th::get(cx);
    n.map(|n| match n {
        Notice::Ok(s) => div().text_color(th.lime).child(s).into_any_element(),
        Notice::Err(s) => div().text_color(th.red).child(s).into_any_element(),
    })
}

fn membw_access(this: &mut Root, enable: bool, cx: &mut Context<Root>) {
    this.state.update(cx, |s, cx| {
        let ctl = s.ctl.clone();
        s.notice = None;
        s.run_action(
            cx,
            move || {
                let r = neontop_core::helper::elevate(&[if enable { "membw-grant" } else { "membw-revoke" }]);
                ctl.collect.restart_membw.store(true, std::sync::atomic::Ordering::Relaxed);
                r
            },
            move |s, r| {
                s.notice = Some(match r {
                    Ok(()) => Notice::Ok(if enable { "Permiso concedido: la medición empieza en unos segundos.".into() } else { "Desactivado.".into() }),
                    Err(e) => Notice::Err(e),
                });
            },
        );
    });
}
