//! Energía: consumo total y por componente, SAI, permisos RAPL, fuentes, histórico por horas y
//! días, curva de eficiencia, límites, consumo estimado por proceso y coste.

use gpui::{div, prelude::*, px, AnyElement, Context, Entity, FontWeight, Hsla, Subscription, Window};
use neontop_core::collectors::energy::EnergySource;
use neontop_core::energy_log::{estimate_procs, fmt_cost, kind_color, kind_label, top_sources, REST_LABEL};
use neontop_core::format as fmt;

use super::{ht, hv, y_watts};
use crate::app::Root;
use crate::charts::{self, Line, Plot, XAxis};
use crate::state::Notice;
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::kit::{self, Panel};
use crate::ui::theme::{fade, Th};

#[derive(Default)]
pub struct EnergyView {
    price: Option<Entity<TextInput>>,
    currency: Option<Entity<TextInput>>,
    _subs: Vec<Subscription>,
}

fn method_label(m: &str) -> &'static str {
    match m {
        "ups" => "SAI: todo lo enchufado (PC, monitores…)",
        "platform" => "sensor de plataforma (equipo completo)",
        "psu" => "fuente de alimentación (equipo completo)",
        "battery" => "descarga de la batería (equipo completo)",
        "sum" => "suma de componentes medidos",
        _ => "sin fuentes medibles",
    }
}

fn role(x: &EnergySource, all: &[EnergySource]) -> String {
    if x.system {
        "mide todo el equipo".into()
    } else if let Some(p) = &x.parent {
        format!("dentro de {}", all.iter().find(|s| &s.id == p).map(|s| s.label.as_str()).unwrap_or(p))
    } else if x.in_total {
        "suma al total".into()
    } else if x.kind == "charge" {
        "potencia de carga".into()
    } else {
        "no suma (podría solaparse)".into()
    }
}

/// Campos de precio y moneda (se crean la primera vez que se muestra la pestaña).
fn ensure_inputs(root: &mut Root, cx: &mut Context<Root>) {
    if root.energy.price.is_some() {
        return;
    }
    let (price, currency) = {
        let ui = &root.state.read(cx).ui;
        (ui.price, ui.currency.clone())
    };
    let p = cx.new(|cx| {
        let mut t = TextInput::new(cx, "0.15").width(90.);
        t.set_text(format!("{price}"), cx);
        t
    });
    let c = cx.new(|cx| {
        let mut t = TextInput::new(cx, "$").width(52.);
        t.set_text(currency, cx);
        t
    });
    let save = |this: &mut Root, cx: &mut Context<Root>| {
        let price = this.energy.price.as_ref().map(|p| p.read(cx).text().replace(',', ".")).and_then(|s| s.trim().parse::<f64>().ok());
        let cur = this.energy.currency.as_ref().map(|c| c.read(cx).text().chars().take(5).collect::<String>()).unwrap_or_default();
        this.state.update(cx, |s, cx| {
            s.update_ui(
                |u| {
                    if let Some(p) = price {
                        u.price = p.max(0.0);
                    }
                    if !cur.trim().is_empty() {
                        u.currency = cur.trim().to_string();
                    }
                },
                cx,
            )
        });
    };
    root.energy._subs = vec![
        cx.subscribe(&p, move |this, _, _: &InputEvent, cx| save(this, cx)),
        cx.subscribe(&c, move |this, _, _: &InputEvent, cx| save(this, cx)),
    ];
    root.energy.price = Some(p);
    root.energy.currency = Some(c);
}

fn rapl(this: &mut Root, enable: bool, persistent: bool, cx: &mut Context<Root>) {
    this.state.update(cx, |s, cx| {
        let ctl = s.ctl.clone();
        s.notice = None;
        s.run_action(
            cx,
            move || {
                let args: Vec<&str> = if enable { vec!["rapl-grant", if persistent { "1" } else { "0" }] } else { vec!["rapl-revoke"] };
                let r = neontop_core::helper::elevate(&args);
                ctl.collect.rescan_energy.store(true, std::sync::atomic::Ordering::Relaxed);
                r
            },
            move |s, r| {
                s.notice = Some(match r {
                    Ok(()) => Notice::Ok(if enable { "Permiso concedido: el consumo de la CPU aparece en la siguiente muestra.".into() } else { "Permiso retirado.".into() }),
                    Err(e) => Notice::Err(e),
                });
            },
        );
    });
}

pub fn render(root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    ensure_inputs(root, cx);
    let th = Th::get(cx).clone();
    let st = root.state.read(cx);
    let s = st.model.snap.clone();
    let e = &s.energy;
    let t = ht(st);
    let log = &st.model.energy;
    let (price, currency) = (st.ui.price, st.ui.currency.clone());
    let cost = |wh: f64| fmt_cost(wh, price, &currency);
    let desktop = !st.demo;
    let notice = st.notice.clone();
    let sources: Vec<EnergySource> = top_sources(&e.sources).into_iter().cloned().chain(e.sources.iter().filter(|x| x.parent.is_some()).cloned()).collect();
    let summed: Vec<&EnergySource> = e.sources.iter().filter(|x| x.in_total).collect();
    let ups_src = e.sources.iter().find(|x| x.kind == "ups");
    let parts_w: f64 = summed.iter().map(|x| x.watts.unwrap_or(0.0)).sum();
    let rest_w = ups_src.map(|u| (u.watts.unwrap_or(0.0) - parts_w).max(0.0)).unwrap_or(0.0);
    let cpu_w = neontop_core::energy_log::cpu_watts(&s);
    let gpu_srcs: Vec<&EnergySource> = e.sources.iter().filter(|x| x.kind == "gpu" || x.kind == "igpu").collect();
    let gpu_w: f64 = gpu_srcs.iter().filter_map(|x| x.watts).sum();
    let elapsed = ((s.ts.saturating_sub(log.start_ts)) as f64 / 1000.0).max(1.0);
    let session_wh = e.total_j / 3600.0;
    // la energía total se cuenta desde que NeonTop empezó a medir
    let avg_w = e.total_j / elapsed;
    let today = log.today(s.ts).cloned().unwrap_or_default();
    let month_wh = log.month_wh(s.ts);
    let proj = |hours: f64| avg_w * hours / 1000.0;
    let cpu_src = e.sources.iter().find(|x| x.kind == "cpu").map(|x| x.id.clone());

    let stats = vec![
        kit::stat("Consumo ahora", if e.total_method == "none" { "N/D".into() } else { fmt::watts(Some(e.total_w)) }, method_label(&e.total_method), th.amber, Some(hv(st, "energy.total")), cx).into_any_element(),
        kit::stat(
            "CPU",
            fmt::watts(cpu_w),
            if cpu_w.is_none() { if e.rapl == "denied" { "sin permiso RAPL" } else { "no disponible" } } else { "paquete (RAPL)" },
            th.cyan,
            cpu_src.map(|id| hv(st, &format!("energy.src.{id}"))),
            cx,
        )
        .into_any_element(),
        kit::stat("GPU", if gpu_srcs.is_empty() { "N/D".into() } else { fmt::watts(Some(gpu_w)) }, if gpu_srcs.is_empty() { "sin sensor".into() } else { gpu_srcs.iter().map(|x| x.label.as_str()).collect::<Vec<_>>().join(", ") }, th.magenta, None, cx).into_any_element(),
        kit::stat("Media sesión", fmt::watts(Some(avg_w)), format!("pico {} · mín {}", fmt::watts(Some(log.peak_w)), if log.min_w.is_finite() { fmt::watts(Some(log.min_w)) } else { "N/D".into() }), th.teal, None, cx).into_any_element(),
        kit::stat("Sesión", fmt::wh(session_wh), format!("{} · {}", cost(session_wh), fmt::duration(elapsed)), th.lime, None, cx).into_any_element(),
        kit::stat("Hoy", fmt::wh(today.wh), cost(today.wh), th.violet, None, cx).into_any_element(),
        kit::stat("Este mes", fmt::wh(month_wh), cost(month_wh), th.orange, None, cx).into_any_element(),
        kit::stat("Proyección 30 días", format!("{:.1} kWh", proj(24.0 * 30.0)), format!("{currency}{:.2} al ritmo medio", proj(24.0 * 30.0) * price), th.red, None, cx).into_any_element(),
    ];
    let mut g = kit::grid(cx).child(kit::stats_row(stats, cx));

    // SAI
    if let Some(u) = &e.ups {
        let (stxt, scol) = if u.low_battery {
            ("BATERÍA BAJA", th.red)
        } else if u.on_battery {
            ("EN BATERÍA", th.amber)
        } else if u.status.contains("CHARG") {
            ("EN RED · CARGANDO", th.lime)
        } else {
            ("EN RED", th.lime)
        };
        let power = u.power_w.unwrap_or(0.0);
        let nominal = u.nominal_w.unwrap_or(0.0);
        let n = |v: Option<f64>, d: usize, unit: &str| v.map(|v| format!("{v:.d$}{unit}")).unwrap_or("—".into());
        let step = (nominal > 0.0).then(|| nominal / 100.0);
        g = g
            .child(
                Panel::new(format!("SAI · {}", u.model), scol).span(2).extra(kit::badge(stxt, scol)).child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(20.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(kit::muted("CONSUMO DEL PUESTO", cx).text_size(px(9.5)))
                                .child(div().font_family(th.display.clone()).font_weight(FontWeight::BOLD).text_size(px(th.dsize(26.))).text_color(scol).child(fmt::watts(Some(power))))
                                .child(kit::muted(format!("{} de {}{}", n(u.load_pct, 0, " %"), n(u.nominal_w, 0, " W"), step.map(|s| format!(" · resolución {s:.0} W")).unwrap_or_default()), cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(220.))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(charts::segments(vec![((parts_w.min(power) / power.max(1.0)) as f32, th.cyan), ((rest_w / power.max(1.0)) as f32, th.violet)], 10.))
                                .child(kit::legend(vec![(format!("PC medido (CPU, GPU) {}", fmt::watts(Some(parts_w))), th.cyan)]))
                                .child(kit::legend(vec![(format!("resto: placa, RAM, discos, monitores, pérdidas… {}", fmt::watts(Some(rest_w))), th.violet)])),
                        ),
                ).child(kit::kv_grid(
                    [
                        Some(("Batería".into(), format!("{} · {}{}", n(u.battery_pct, 0, " %"), n(u.battery_v, 1, " V"), u.battery_nominal_v.map(|v| format!(" (nominal {v} V)")).unwrap_or_default()))),
                        Some(("Autonomía".into(), u.runtime_min.map(|m| format!("{m:.0} min con esta carga")).unwrap_or("—".into()))),
                        Some(("Red".into(), format!("{}{}{}", n(u.line_v, 1, " V"), u.line_hz.map(|h| format!(" · {h} Hz")).unwrap_or_default(), u.output_v.map(|o| format!(" · salida {o} V")).unwrap_or_default()))),
                        Some((
                            "Cortes".into(),
                            format!(
                                "{} desde que arrancó apcupsd{}{}",
                                u.transfers.unwrap_or(0),
                                u.last_transfer.as_ref().filter(|_| u.transfers.unwrap_or(0) > 0).map(|l| format!(" · último: {l}")).unwrap_or_default(),
                                u.on_battery_total_s.filter(|&s| s > 0.0).map(|s| format!(" · {} en batería", fmt::duration(s))).unwrap_or_default()
                            ),
                        )),
                        u.battery_date.as_ref().map(|d| ("Batería de".into(), d.clone())),
                        u.temp_c.map(|t| ("Temp.".into(), format!("{t:.1} °C"))),
                    ]
                    .into_iter()
                    .flatten()
                    .collect(),
                    true,
                    cx,
                )),
            )
            .child(Panel::new("Carga del SAI", th.amber).child(charts::gauge(
                (power / nominal.max(1.0)) as f32,
                if u.load_pct.unwrap_or(0.0) > 80.0 { th.red } else { th.amber },
                fmt::watts(Some(power)),
                n(u.load_pct, 0, " %"),
                format!("MÁX {nominal:.0} W"),
                210.,
            )))
            .child(Panel::new("Batería", scol).child(charts::gauge(
                (u.battery_pct.unwrap_or(0.0) / 100.0) as f32,
                if u.battery_pct.unwrap_or(100.0) < 30.0 { th.red } else { th.lime },
                n(u.battery_pct, 0, "%"),
                u.runtime_min.map(|m| format!("{m:.0} min")).unwrap_or_default(),
                "BATERÍA".into(),
                210.,
            )));
        let lv = u.line_v.unwrap_or(120.0).round() as f32;
        let mut p = Plot::new(vec![Line::new(hv(st, "ups.lineV"), th.cyan)], t.clone()).y_fmt(|v| format!("{v:.0} V")).height(140.).y_max(lv + 8.0);
        p.y_min = (lv - 8.0).max(0.0);
        g = g.child(Panel::new("Red eléctrica", th.cyan).span(4).extra("tensión de entrada (V)").child(p.into_element()));
    }

    if e.rapl == "denied" && desktop {
        g = g.child(
            Panel::new("Consumo de CPU bloqueado", th.amber)
                .span(4)
                .child(div().child("Tu procesador mide su propio consumo (contadores RAPL), pero el kernel solo deja leerlos a root desde 2020, por la vulnerabilidad PLATYPUS (CVE-2020-8694): con lecturas muy frecuentes de energía, un programa local podría llegar a deducir datos que procesa la CPU. En un equipo personal de un solo usuario el riesgo es bajo, pero es tu decisión."))
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(kit::button("rapl-tmp", "Permitir hasta reiniciar", th.amber, cx).on_click(cx.listener(|this, _, _, cx| rapl(this, true, false, cx))))
                        .child(kit::button_solid("rapl-perm", "Permitir siempre", th.amber, cx).on_click(cx.listener(|this, _, _, cx| rapl(this, true, true, cx)))),
                )
                .child(kit::muted("Se pedirá tu contraseña en la ventana de autenticación del sistema (polkit). «Siempre» instala la regla /etc/udev/rules.d/60-neontop-rapl.rules; para revertirlo, borra ese fichero o usa «Retirar permiso».", cx))
                .children(crate::tabs::memory::notice_line(notice.clone(), cx)),
        );
    }

    // potencia por componente (apilada) y reparto de la energía
    let mut stack: Vec<Line> = summed.iter().map(|x| Line::new(hv(st, &format!("energy.src.{}", x.id)), th.slot(kind_color(&x.kind))).fill().width(1.4)).collect();
    let mut stack_legend: Vec<(String, Hsla)> = summed.iter().map(|x| (x.label.clone(), th.slot(kind_color(&x.kind)))).collect();
    if let Some(u) = ups_src {
        let total = hv(st, &format!("energy.src.{}", u.id));
        let parts: Vec<Vec<f32>> = summed.iter().map(|x| hv(st, &format!("energy.src.{}", x.id))).collect();
        let rest: Vec<f32> = total.iter().enumerate().map(|(i, v)| (v - parts.iter().map(|p| p.get(i).copied().unwrap_or(0.0)).sum::<f32>()).max(0.0)).collect();
        stack.push(Line::new(rest, th.violet).fill().width(1.4));
        stack_legend.push((REST_LABEL.into(), th.violet));
    }
    let mut share: Vec<(String, f64, Hsla)> = summed.iter().map(|x| (x.label.clone(), x.joules / 3600.0, th.slot(kind_color(&x.kind)))).collect();
    if let Some(u) = ups_src {
        let rest = (u.joules - summed.iter().map(|x| x.joules).sum::<f64>()).max(0.0) / 3600.0;
        share.push((REST_LABEL.into(), rest, th.violet));
    }
    let share_tot: f64 = share.iter().map(|x| x.1).sum::<f64>().max(1e-9);
    g = g
        .child(
            Panel::new("Potencia por componente", th.amber)
                .span(3)
                .extra(kit::legend(stack_legend.into_iter().chain([(if ups_src.is_some() { "apilado = total del SAI".into() } else { "apilado = total medido".into() }, th.muted)]).collect()))
                .child(Plot::new(stack, t.clone()).stacked().y_fmt(y_watts).height(260.).into_element()),
        )
        .child(
            Panel::new("Reparto de la energía", th.lime)
                .extra("desde el arranque")
                .child(charts::ring(share.iter().map(|x| ((x.1 / share_tot) as f32, x.2)).collect(), fmt::wh(share_tot), "sesión".into(), 170.))
                .child(div().flex().flex_col().gap(px(3.)).children(share.iter().map(|x| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(11.))
                        .child(div().size(px(7.)).bg(x.2))
                        .child(div().flex_1().truncate().child(x.0.clone()))
                        .child(div().font_family(th.mono.clone()).child(fmt::wh(x.1)))
                        .child(div().w(px(36.)).flex().justify_end().text_color(th.muted).child(format!("{:.0}%", x.1 / share_tot * 100.0)))
                }))),
        );

    // una tarjeta por fuente
    let w = st.window();
    let cards = div().col_span(kit::cols(cx) as u16).grid().grid_cols((kit::cols(cx) * 2).clamp(1, 4) as u16).gap(px(10.)).children(sources.iter().map(|x| {
        let color = th.slot(kind_color(&x.kind));
        let raw = st.model.history.raw(&format!("energy.src.{}", x.id), w);
        let stats = (!raw.is_empty()).then(|| {
            let (mn, mx, sum) = raw.iter().fold((f32::MAX, 0.0f32, 0.0f32), |a, &v| (a.0.min(v), a.1.max(v), a.2 + v));
            format!("mín {mn:.1} · media {:.1} · máx {mx:.1} W", sum / raw.len() as f32)
        });
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(10.))
            .bg(th.panel)
            .border_1()
            .border_color(th.border(color))
            .rounded(px(th.radius.min(6.)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(kit::badge(kind_label(&x.kind), color))
                    .child(div().flex_1().truncate().child(x.label.clone()))
                    .child(kit::muted(if x.method == "counter" { "contador" } else { "sensor" }, cx)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().w(px(84.)).font_family(th.display.clone()).font_weight(FontWeight::BOLD).text_size(px(th.dsize(18.))).text_color(color).child(fmt::watts(x.watts)))
                    .child(div().flex_1().child(charts::sparkline(hv(st, &format!("energy.src.{}", x.id)), color, 28., None))),
            )
            .children(x.limit.map(|l| charts::meter((x.watts.unwrap_or(0.0) / l) as f32, color, 4.)))
            .child(kit::muted(stats.unwrap_or("sin datos".into()), cx))
            .child(kit::muted(format!("{}{}", fmt::wh(x.joules / 3600.0), x.limit.map(|l| format!(" · límite {l:.0} W")).unwrap_or_default()), cx))
            .child(div().text_size(px(10.)).text_color(fade(color, 0.8)).child(role(x, &e.sources)))
    }));
    g = g.child(if sources.is_empty() { kit::muted("No se encontró ninguna fuente de consumo medible.", cx).into_any_element() } else { cards.into_any_element() });

    let mut all_lines = vec![Line::new(hv(st, "energy.total"), th.amber).width(2.)];
    let mut all_legend = vec![("total".to_string(), th.amber)];
    for (i, x) in e.sources.iter().enumerate() {
        let c = th.series(i + 1);
        all_lines.push(Line::new(hv(st, &format!("energy.src.{}", x.id)), c).width(1.2));
        all_legend.push((x.label.clone(), c));
    }
    // histórico: hoy por horas y últimos 30 días
    let hours: Vec<(String, f32, Hsla)> = today.hours.iter().enumerate().map(|(h, wh)| (format!("{h}"), *wh as f32, th.cyan)).collect();
    let hmax = hours.iter().map(|h| h.1).fold(0.0, f32::max);
    let mut days: Vec<(&String, &neontop_core::energy_log::DayEnergy)> = log.days.iter().collect();
    days.sort_by(|a, b| a.0.cmp(b.0));
    let days: Vec<(String, f32, Hsla)> = days.iter().rev().take(30).rev().map(|(k, d)| (k[8..].to_string(), d.wh as f32, th.violet)).collect();
    let dmax = days.iter().map(|d| d.1).fold(0.0, f32::max);
    g = g
        .child(Panel::new("Todas las fuentes", th.cyan).span(4).extra(kit::legend(all_legend.into_iter().take(6).collect())).child(Plot::new(all_lines, t.clone()).y_fmt(y_watts).height(220.).into_element()))
        .child(Panel::new("Hoy, por horas", th.cyan).span(2).extra(format!("{} · {}", fmt::wh(today.wh), cost(today.wh))).child(charts::columns(hours, charts::nice_max(hmax.max(1.0)), vec![], |v| format!("{v:.0} Wh"), 220.)))
        .child(Panel::new("Últimos 30 días", th.violet).span(2).extra("se guarda mientras NeonTop está abierto").child(charts::columns(days, charts::nice_max(dmax.max(1.0)), vec![], |v| fmt::wh(v as f64), 220.)));

    // curva de eficiencia: uso de CPU frente a su potencia
    let eff: AnyElement = match e.sources.iter().find(|x| x.kind == "cpu") {
        Some(c) => {
            let ws = st.model.history.raw(&format!("energy.src.{}", c.id), w);
            let us = st.model.history.raw("cpu.usage", w);
            let dots: Vec<(f64, f32, Hsla)> = us.iter().zip(ws).rev().take(600).map(|(u, w)| (*u as f64, *w, fade(th.cyan, 0.6))).collect();
            let ymax = dots.iter().map(|d| d.1).fold(1.0, f32::max) * 1.1;
            Plot::new(vec![], vec![]).x_range(0.0, 100.0).x_axis(XAxis::Value("%")).y_max(ymax).y_fmt(y_watts).dots(dots).height(240.).into_element().into_any_element()
        }
        None => kit::muted("Hace falta la potencia de la CPU para dibujar la curva.", cx).into_any_element(),
    };
    g = g.child(Panel::new("Curva de eficiencia", th.teal).span(2).extra("uso de CPU frente a su potencia").child(eff));
    for x in e.sources.iter().filter(|x| x.limit.is_some() && x.watts.is_some()).take(2) {
        let c = th.slot(kind_color(&x.kind));
        let (wv, l) = (x.watts.unwrap_or(0.0), x.limit.unwrap_or(1.0));
        g = g.child(Panel::new(format!("{} · límite", kind_label(&x.kind)), c).extra(x.label.clone()).child(charts::gauge((wv / l) as f32, c, fmt::watts(Some(wv)), format!("{:.0}%", wv / l * 100.0), format!("LÍMITE {l:.0} W"), 240.)));
    }

    // consumo estimado por proceso
    let procs = &st.model.procs.list.procs;
    let rows = estimate_procs(&s, procs, &s.gpus);
    let pmax = rows.first().map(|r| r.total_w).unwrap_or(1.0).max(0.1);
    let proc_body: AnyElement = if cpu_w.is_none() && gpu_srcs.is_empty() {
        kit::muted("Sin potencia de CPU ni de GPU no se puede repartir el consumo entre procesos.", cx).into_any_element()
    } else {
        div()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                div()
                    .flex()
                    .text_size(px(10.))
                    .text_color(th.muted)
                    .font_family(th.mono.clone())
                    .child(div().flex_1().child(th.title("Proceso")))
                    .child(div().w(px(52.)).flex().justify_end().child("CPU"))
                    .child(div().w(px(52.)).flex().justify_end().child("GPU"))
                    .child(div().w(px(150.)).pl(px(10.)).child("Total"))
                    .child(div().w(px(70.)).flex().justify_end().child("Acum.")),
            )
            .children(rows.iter().take(14).map(|r| {
                div()
                    .flex()
                    .items_center()
                    .h(px(20.))
                    .font_family(th.mono.clone())
                    .text_size(px(11.))
                    .child(div().flex_1().truncate().child(format!("{}{}", r.name, if r.count > 1 { format!(" ×{}", r.count) } else { String::new() })))
                    .child(div().w(px(52.)).flex().justify_end().child(r.cpu_w.map(|w| format!("{w:.1}")).unwrap_or("—".into())))
                    .child(div().w(px(52.)).flex().justify_end().child(r.gpu_w.map(|w| format!("{w:.1}")).unwrap_or("—".into())))
                    .child(
                        div()
                            .w(px(150.))
                            .pl(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(div().flex_1().child(charts::segments(vec![((r.cpu_w.unwrap_or(0.0) / pmax) as f32, th.cyan), ((r.gpu_w.unwrap_or(0.0) / pmax) as f32, th.magenta)], 6.)))
                            .child(div().w(px(56.)).flex().justify_end().child(format!("{:.1} W", r.total_w))),
                    )
                    .child(div().w(px(70.)).flex().justify_end().text_color(th.muted).child(fmt::wh(log.proc_j.get(&r.name).copied().unwrap_or(0.0) / 3600.0)))
            }))
            .child(kit::muted(
                format!(
                    "Estimación: la potencia de la CPU se reparte según el tiempo de CPU de cada proceso y la de la GPU según el uso de sus motores; incluye el consumo en reposo repartido. «Acum.» cuenta mientras se mide la lista de procesos.{}",
                    if cpu_w.is_none() { " Sin permiso RAPL solo se reparte la GPU." } else { "" }
                ),
                cx,
            ))
            .into_any_element()
    };
    g = g.child(Panel::new("Consumo estimado por proceso", th.magenta).span(2).extra("reparto de la potencia medida").child(proc_body));

    // coste
    let cost_rows: Vec<(String, f64)> = vec![
        (format!("Sesión ({})", fmt::duration(elapsed)), session_wh),
        ("Hoy".into(), today.wh),
        ("Este mes".into(), month_wh),
        ("1 hora al ritmo medio".into(), avg_w),
        ("24 horas".into(), avg_w * 24.0),
        ("30 días".into(), avg_w * 24.0 * 30.0),
        ("1 año".into(), avg_w * 24.0 * 365.0),
    ];
    let rapl_ok = e.rapl == "ok" && desktop;
    let rapl_rule = e.rapl_rule;
    let cost_panel = Panel::new("Coste", th.orange)
        .span(2)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child("Precio por kWh")
                .children(root.energy.currency.clone())
                .children(root.energy.price.clone()),
        )
        .child(div().flex().flex_col().gap(px(3.)).children(cost_rows.into_iter().enumerate().map(|(i, (l, wh))| {
            div()
                .flex()
                .text_size(px(11.5))
                .when(i == 3, |d| d.border_t_1().border_color(th.border(th.orange)).pt(px(4.)))
                .child(div().flex_1().child(l))
                .child(div().w(px(100.)).flex().justify_end().font_family(th.mono.clone()).child(fmt::wh(wh)))
                .child(div().w(px(90.)).flex().justify_end().font_family(th.mono.clone()).text_color(th.orange).child(cost(wh)))
        })))
        .child(kit::muted(
            if e.total_method == "ups" {
                "El total es lo que mide el SAI: todo lo enchufado a él (PC, monitores y lo que tengas conectado), incluidas las pérdidas de la fuente. Es el consumo real del puesto en el enchufe.".to_string()
            } else {
                format!("El total es lo medido: {}. Sin un sensor del equipo completo (SAI, plataforma, fuente con sensor o batería) no se incluyen placa base, discos, ventiladores, RAM (en AMD), pérdidas de la fuente ni el monitor, así que el consumo real en el enchufe es mayor.", method_label(&e.total_method))
            },
            cx,
        ))
        .when(rapl_ok, |p| {
            p.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(kit::muted(format!("RAPL: acceso {}.", if rapl_rule { "permanente (regla udev)" } else { "hasta reiniciar" }), cx))
                    .when(!rapl_rule, |d| d.child(kit::button("rapl-make", "Hacer permanente", th.amber, cx).on_click(cx.listener(|this, _, _, cx| rapl(this, true, true, cx)))))
                    .child(kit::button("rapl-revoke", "Retirar permiso", th.muted, cx).on_click(cx.listener(|this, _, _, cx| rapl(this, false, false, cx)))),
            )
        })
        .when(e.rapl == "absent", |p| p.child(kit::muted("Este procesador o kernel no expone contadores RAPL: el consumo de la CPU no se puede medir.", cx)))
        .children(crate::tabs::memory::notice_line(notice, cx));
    g.child(cost_panel).into_any_element()
}
