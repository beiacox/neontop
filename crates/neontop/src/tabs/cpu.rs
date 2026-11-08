//! CPU: cifras principales, reparto del tiempo, perfil de carga, mapa de calor por hilo, uso
//! radial, frecuencias y temperatura.

use gpui::{prelude::*, AnyElement, Context, Window};
use neontop_core::format as fmt;

use super::{ht, hv, y_mhz, y_pct};
use crate::app::Root;
use crate::charts::{self, Line, Plot};
use crate::ui::kit::{self, Panel};
use crate::ui::theme::{fade, Th};

pub fn render(root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    let th = Th::get(cx).clone();
    let st = root.state.read(cx);
    let s = st.model.snap.clone();
    let info = st.model.info.clone();
    let t = ht(st);
    let c = &s.cpu;
    let tot = &c.total;
    let freq_avg = c.freqs.iter().sum::<f32>() / c.freqs.len().max(1) as f32;
    let breakdown = [
        ("user", tot.user, th.cyan),
        ("nice", tot.nice, th.teal),
        ("system", tot.system, th.magenta),
        ("iowait", tot.iowait, th.amber),
        ("irq", tot.irq, th.violet),
        ("softirq", tot.softirq, th.blue),
        ("steal", tot.steal, th.red),
    ];
    let stats = vec![
        kit::stat("Uso total", fmt::pct(tot.usage as f64, 1), "", th.load_color(tot.usage), Some(hv(st, "cpu.usage")), cx).into_any_element(),
        kit::stat("Frecuencia media", fmt::freq(freq_avg as f64), "", th.violet, Some(hv(st, "freq.avg")), cx).into_any_element(),
        kit::stat("Temperatura", c.temp.map(|t| format!("{t:.1}°C")).unwrap_or("N/D".into()), "", th.temp_color(c.temp.unwrap_or(0.0)), Some(hv(st, "cpu.temp")), cx).into_any_element(),
        kit::stat("Carga 1m", format!("{:.2}", c.load_avg[0]), format!("5m {:.2} · 15m {:.2}", c.load_avg[1], c.load_avg[2]), th.lime, None, cx).into_any_element(),
        kit::stat("Cambios ctx/s", fmt::num(c.ctxt_rate as f64), "", th.cyan, Some(hv(st, "cpu.ctxt")), cx).into_any_element(),
        kit::stat("Interrupciones/s", fmt::num(c.intr_rate as f64), "", th.amber, Some(hv(st, "cpu.intr")), cx).into_any_element(),
        kit::stat("Forks/s", fmt::num(c.fork_rate as f64), "", th.magenta, Some(hv(st, "cpu.fork")), cx).into_any_element(),
        kit::stat("Procesos", format!("{}/{}", c.procs_running, c.procs_total), format!("{} bloqueados", c.procs_blocked), th.teal, Some(hv(st, "cpu.running")), cx).into_any_element(),
    ];
    let split = Plot::new(breakdown.iter().map(|(k, _, col)| Line::new(hv(st, &format!("cpu.{k}")), *col).fill()).collect(), t.clone())
        .stacked()
        .y_max(100.)
        .y_fmt(y_pct)
        .height(240.);
    // radar con escala de raíz cuadrada: así lo pequeño (irq, softirq) también se ve
    let radar_axes: Vec<(String, f32)> = breakdown.iter().map(|(k, v, _)| (k.to_string(), v.max(0.0).sqrt() * 10.0)).collect();
    let rows: Vec<Vec<f32>> = (0..c.cores.len()).map(|i| hv(st, &format!("core.{i}"))).collect();
    let labels: Vec<String> = (0..c.cores.len()).map(|i| format!("C{i}")).collect();
    let fmax = info.freq_max.iter().copied().fold(0.0f32, f32::max).max(c.freqs.iter().copied().fold(0.0, f32::max));
    let freq_cols: Vec<(String, f32, gpui::Hsla)> = c.freqs.iter().enumerate().map(|(i, f)| (format!("C{i}"), *f, if i % 2 == 0 { th.cyan } else { th.violet })).collect();
    let freq_lines: Vec<Line> = (0..c.freqs.len()).map(|i| Line::new(hv(st, &format!("freq.{i}")), fade(if i % 2 == 1 { th.violet } else { th.cyan }, 0.55)).width(1.0)).collect();
    let temp_load = vec![Line::new(hv(st, "cpu.temp"), th.amber).fill(), Line::new(hv(st, "cpu.usage"), th.cyan).dashed().width(1.0)];
    let temp = c.temp.unwrap_or(0.0);
    let n_cores = c.cores.len();
    let _ = st;

    kit::grid(cx)
        .child(kit::stats_row(stats, cx))
        .child(
            Panel::new("Reparto del tiempo de CPU", th.cyan)
                .span(3)
                .extra(kit::legend(breakdown.iter().map(|(k, v, col)| (format!("{k} {v:.1}"), *col)).collect()))
                .child(split.into_element()),
        )
        .child(Panel::new("Perfil de carga", th.magenta).child(charts::radar(radar_axes, th.magenta, 250.)))
        .child(
            Panel::new("Mapa de calor por núcleo", th.violet)
                .span(4)
                .extra("uso de cada hilo en la ventana de tiempo")
                .child(charts::heatmap(rows, labels, (n_cores as f32 * 16.0).max(160.))),
        )
        .child(Panel::new("Uso radial", th.lime).child(charts::polar(c.cores.iter().map(|x| x.usage).collect(), fmt::pct(tot.usage as f64, 0), 260.)))
        .child(
            Panel::new("Frecuencia por núcleo", th.violet)
                .span(2)
                .extra("GHz · ━ máx.")
                .child(charts::columns(freq_cols, (fmax * 1.05).max(1.0), info.freq_max.clone(), y_mhz, 260.)),
        )
        .child(
            Panel::new("Temperatura", th.temp_color(temp)).child(charts::gauge(
                temp / 100.0,
                th.temp_color(temp),
                c.temp.map(|t| format!("{t:.1}°")).unwrap_or("N/D".into()),
                c.temps.iter().map(|t| format!("{} {:.0}°", t.label, t.celsius)).collect::<Vec<_>>().join("  "),
                "°C".into(),
                260.,
            )),
        )
        .child(Panel::new("Frecuencia en el tiempo", th.violet).span(2).child(Plot::new(freq_lines, t.clone()).y_fmt(y_mhz).height(200.).into_element()))
        .child(
            Panel::new("Temperatura y carga", th.amber)
                .span(2)
                .extra(kit::legend(vec![("temperatura °C".into(), th.amber), ("uso %".into(), th.cyan)]))
                .child(Plot::new(temp_load, t).y_max(100.).height(200.).into_element()),
        )
        .into_any_element()
}
