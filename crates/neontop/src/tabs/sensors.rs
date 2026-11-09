//! Sensores: temperaturas (mapa térmico, barras, historial), cada chip con sus lecturas,
//! ventiladores, potencia y baterías.

use gpui::{div, prelude::*, px, AnyElement, Context, Hsla, Window};
use neontop_core::collectors::sensors::{Chip, Reading};

use super::{ht, hv, y_deg};
use crate::app::Root;
use crate::charts::{self, Line, Plot};
use crate::ui::kit::{self, Panel};
use crate::ui::theme::Th;

fn unit(kind: &str) -> &'static str {
    match kind {
        "temp" => "°C",
        "fan" => " RPM",
        "in" => " V",
        "power" => " W",
        "curr" => " A",
        "pwm" => "%",
        _ => "",
    }
}

fn fmt_reading(r: &Reading) -> String {
    match r.kind.as_str() {
        "fan" => format!("{:.0} RPM", r.value),
        "in" => format!("{:.3} V", r.value),
        "curr" => format!("{:.2} A", r.value),
        "pwm" => format!("{:.0}%", r.value),
        k => format!("{:.1}{}", r.value, unit(k)),
    }
}

/// Límite para normalizar: crítico, máximo o 100 °C.
fn limit(r: &Reading) -> f64 {
    r.crit.or(r.max).unwrap_or(100.0)
}

pub fn render(root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    let th = Th::get(cx).clone();
    let st = root.state.read(cx);
    let s = st.model.snap.clone();
    let t = ht(st);
    let chips = &s.sensors.chips;
    let cat_color = |c: &str| match c {
        "cpu" => th.cyan,
        "gpu" => th.magenta,
        "disk" => th.amber,
        "network" => th.lime,
        "board" => th.violet,
        _ => th.teal,
    };
    let cat_icon = |c: &str| match c {
        "cpu" => "▦",
        "gpu" => "▣",
        "disk" => "▰",
        "network" => "◉",
        "board" => "▤",
        _ => "✺",
    };
    // temperaturas principales: sin los sensores internos redundantes de los NVMe
    let temps: Vec<(&Chip, &Reading)> = chips
        .iter()
        .flat_map(|c| {
            c.readings
                .iter()
                .filter(move |r| r.kind == "temp" && !(c.category == "disk" && r.label.starts_with("Sensor ") && c.readings.iter().any(|x| x.label == "Composite")))
                .map(move |r| (c, r))
        })
        .collect();
    let all_temps = chips.iter().flat_map(|c| c.readings.iter().filter(|r| r.kind == "temp")).count();
    let hottest = temps.iter().max_by(|a, b| a.1.value.total_cmp(&b.1.value));
    let find = |cat: &str, labels: &[&str]| {
        let c = chips.iter().find(|x| x.category == cat)?;
        labels.iter().find_map(|l| c.readings.iter().find(|r| r.label == *l)).or_else(|| c.readings.iter().find(|r| r.kind == "temp")).cloned()
    };
    let cpu_t = find("cpu", &["Tctl", "Tdie", "Package id 0"]);
    let gpu_t = find("gpu", &["junction", "edge"]);
    let fans: Vec<(&Chip, &Reading)> = chips.iter().flat_map(|c| c.readings.iter().filter(|r| r.kind == "fan").map(move |r| (c, r))).collect();
    let power: Vec<(&Chip, &Reading)> = chips.iter().flat_map(|c| c.readings.iter().filter(|r| r.kind == "power").map(move |r| (c, r))).collect();
    let has_board = chips.iter().any(|c| c.category == "board");
    let deg = |r: &Option<Reading>| r.as_ref().map(|r| format!("{:.1}°C", r.value)).unwrap_or("N/D".into());

    let mut stats = vec![
        kit::stat("CPU", deg(&cpu_t), cpu_t.as_ref().map(|r| r.label.clone()).unwrap_or_default(), th.temp_color(cpu_t.as_ref().map(|r| r.value as f32).unwrap_or(0.0)), cpu_t.as_ref().map(|r| hv(st, &format!("sensor.{}", r.id))), cx).into_any_element(),
        kit::stat("GPU", deg(&gpu_t), gpu_t.as_ref().map(|r| r.label.clone()).unwrap_or_default(), th.temp_color(gpu_t.as_ref().map(|r| r.value as f32).unwrap_or(0.0)), gpu_t.as_ref().map(|r| hv(st, &format!("sensor.{}", r.id))), cx).into_any_element(),
        kit::stat(
            "Más caliente",
            hottest.map(|h| format!("{:.1}°C", h.1.value)).unwrap_or("N/D".into()),
            hottest.map(|h| format!("{} · {}", h.0.label, h.1.label)).unwrap_or_default(),
            th.temp_color(hottest.map(|h| h.1.value as f32).unwrap_or(0.0)),
            None,
            cx,
        )
        .into_any_element(),
        kit::stat(
            "Media",
            if temps.is_empty() { "N/D".into() } else { format!("{:.1}°C", temps.iter().map(|t| t.1.value).sum::<f64>() / temps.len() as f64) },
            format!("{all_temps} sensores de temperatura"),
            th.teal,
            None,
            cx,
        )
        .into_any_element(),
    ];
    for (c, r) in fans.iter().take(2) {
        stats.push(kit::stat(format!("Ventilador {}", c.label), format!("{:.0} RPM", r.value), r.max.map(|m| format!("máx. {m:.0} RPM")).unwrap_or(r.label.clone()), th.cyan, Some(hv(st, &format!("sensor.{}", r.id))), cx).into_any_element());
    }
    for (c, r) in power.iter().take(2) {
        stats.push(kit::stat(format!("Potencia {}", c.label), format!("{:.1} W", r.value), r.label.clone(), th.orange, Some(hv(st, &format!("sensor.{}", r.id))), cx).into_any_element());
    }
    stats.push(kit::stat("Chips", chips.len().to_string(), chips.iter().map(|c| c.driver.as_str()).collect::<Vec<_>>().join(", "), th.violet, None, cx).into_any_element());

    // rótulos cortos en el radar: primera palabra del chip + sensor ("NVMe Composite", "GPU edge")
    let radar: Vec<(String, f32)> = temps
        .iter()
        .map(|(c, r)| (format!("{} {}", c.label.split_whitespace().next().unwrap_or(""), r.label), ((r.value / limit(r)) * 100.0).min(100.0) as f32))
        .collect();
    let bars = div().flex().flex_col().gap(px(6.)).children(temps.iter().map(|(c, r)| {
        let lim = limit(r);
        kit::meter_row(format!("{} · {}", c.label, r.label), (r.value / lim) as f32, format!("{:.1}°C / {:.0}°", r.value, lim), th.temp_color(r.value as f32), cx)
    }));
    let temp_lines: Vec<Line> = temps.iter().enumerate().map(|(i, (_, r))| Line::new(hv(st, &format!("sensor.{}", r.id)), th.series(i)).width(1.4)).collect();
    let temp_legend: Vec<(String, Hsla)> = temps.iter().enumerate().map(|(i, (c, r))| (format!("{} {}", c.label, r.label), th.series(i))).collect();

    let mut g = kit::grid(cx)
        .child(kit::stats_row(stats, cx))
        .child(Panel::new("Mapa térmico", th.amber).span(2).extra("% del límite crítico de cada sensor").child(charts::radar(radar, th.amber, 320.)))
        .child(Panel::new("Temperaturas", th.red).span(2).extra("valor / límite crítico").child(bars))
        .child(Panel::new("Historial de temperaturas", th.amber).span(4).extra(kit::legend(temp_legend.into_iter().take(6).collect())).child(Plot::new(temp_lines, t.clone()).y_fmt(y_deg).height(240.).into_element()));

    for c in chips {
        let accent = cat_color(&c.category);
        let rows = div().flex().flex_col().gap(px(6.)).children(c.readings.iter().map(|r| {
            let color = if r.kind == "temp" { th.temp_color(r.value as f32) } else { accent };
            let pct = match r.kind.as_str() {
                "temp" => r.value / limit(r),
                "fan" => r.max.map(|m| r.value / m).unwrap_or(0.0),
                "pwm" => r.value / 100.0,
                _ => 0.0,
            };
            let lim = match (r.max, r.crit) {
                (Some(m), Some(c)) => format!("máx {m:.0}{u} · crít {c:.0}{u}", u = unit(&r.kind)),
                (Some(m), None) => format!("máx {m:.0}{}", unit(&r.kind)),
                (None, Some(c)) => format!("crít {c:.0}{}", unit(&r.kind)),
                _ => String::new(),
            };
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().w(px(90.)).truncate().text_color(th.muted).child(r.label.clone()))
                        .child(div().flex_1().child(charts::sparkline(hv(st, &format!("sensor.{}", r.id)), color, 18., None)))
                        .child(div().w(px(84.)).flex().justify_end().font_family(th.mono.clone()).text_color(color).child(fmt_reading(r))),
                )
                .when(pct > 0.0, |d| d.child(charts::meter(pct as f32, color, 3.)))
                .when(!lim.is_empty(), |d| d.child(kit::muted(lim, cx).text_size(px(9.5))))
        }));
        g = g.child(Panel::new(format!("{} {}", cat_icon(&c.category), c.label), accent).extra(c.driver.clone()).child(rows));
    }
    if !fans.is_empty() || !power.is_empty() {
        let mut lines: Vec<Line> = fans.iter().enumerate().map(|(i, (_, r))| Line::new(hv(st, &format!("sensor.{}", r.id)), [th.cyan, th.teal, th.blue][i % 3])).collect();
        lines.extend(power.iter().enumerate().map(|(i, (_, r))| Line::new(hv(st, &format!("sensor.{}", r.id)), [th.orange, th.amber][i % 2]).dashed()));
        let mut legend: Vec<(String, Hsla)> = fans.iter().enumerate().map(|(i, (c, r))| (format!("{} {} RPM", c.label, r.label), [th.cyan, th.teal, th.blue][i % 3])).collect();
        legend.extend(power.iter().enumerate().map(|(i, (c, r))| (format!("{} {} W", c.label, r.label), [th.orange, th.amber][i % 2])));
        g = g.child(Panel::new("Ventiladores y potencia", th.cyan).span(2).extra(kit::legend(legend)).child(Plot::new(lines, t.clone()).height(200.).into_element()));
    }
    for (title, r) in [("Temp. CPU", &cpu_t), ("Temp. GPU", &gpu_t)] {
        if let Some(r) = r {
            let c = th.temp_color(r.value as f32);
            g = g.child(Panel::new(title, c).child(charts::gauge((r.value / limit(r)) as f32, c, format!("{:.1}°", r.value), format!("límite {:.0}°", limit(r)), r.label.clone(), 200.)));
        }
    }
    for b in &s.sensors.batteries {
        g = g.child(
            Panel::new(format!("Batería {}", b.name), th.lime)
                .span(2)
                .extra(format!("{}{}", b.status, if s.sensors.ac_online == Some(true) { " · conectado a corriente" } else { "" }))
                .child(
                    div()
                        .flex()
                        .gap(px(16.))
                        .child(div().w(px(200.)).child(charts::gauge(
                            b.capacity / 100.0,
                            if b.capacity < 20.0 { th.red } else { th.lime },
                            format!("{:.0}%", b.capacity),
                            b.power.map(|p| format!("{p:.1} W")).unwrap_or_default(),
                            "CARGA".into(),
                            180.,
                        )))
                        .child(div().flex_1().child(kit::kv_grid(
                            vec![
                                ("Energía".into(), format!("{} / {} Wh", b.energy_now.map(|v| format!("{v:.1}")).unwrap_or("?".into()), b.energy_full.map(|v| format!("{v:.1}")).unwrap_or("?".into()))),
                                ("Salud".into(), match (b.energy_full, b.energy_design) {
                                    (Some(f), Some(d)) if d > 0.0 => format!("{:.0}% del diseño", f / d * 100.0),
                                    _ => "N/D".into(),
                                }),
                                ("Ciclos".into(), b.cycles.map(|c| c.to_string()).unwrap_or("N/D".into())),
                            ],
                            false,
                            cx,
                        ))),
                ),
        );
    }
    if !has_board {
        g = g.child(Panel::new("Sensores de placa base", th.muted).span(4).child(kit::muted(
            "No se detectan sensores de la placa base (ventiladores de caja, voltajes, temperatura del chipset). Suelen requerir un módulo del kernel según el chip: nct6775 (Nuvoton, la mayoría de placas ASUS/MSI/ASRock), it87 (ITE, Gigabyte) o asus_ec_sensors. Puedes detectarlo con «sudo sensors-detect» (paquete lm_sensors); NeonTop los mostrará automáticamente al cargarse.",
            cx,
        )));
    }
    g.into_any_element()
}
