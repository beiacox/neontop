//! Resumen: velocímetros de CPU, RAM, swap y temperatura; historial de CPU, memoria, red y
//! discos; núcleos, presión, carga, temperaturas y los procesos que más gastan.

use neontop_core::t;
use gpui::{div, prelude::*, px, AnyElement, Context, Window};
use neontop_core::format as fmt;
use neontop_core::history::mem_segments;

use super::{hsum, ht, hv, y_bytes, y_pct, y_rate};
use crate::app::Root;
use crate::charts::{self, Line, Plot};
use crate::ui::kit::{self, Panel};
use crate::ui::theme::Th;

pub fn render(root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    let th = Th::get(cx).clone();
    let st = root.state.read(cx);
    let s = st.model.snap.clone();
    let info = st.model.info.clone();
    let t = ht(st);
    let seg = mem_segments(&s.mem);
    let mem_pct = seg.used as f32 / s.mem.total.max(1) as f32 * 100.0;
    let swap_used = s.mem.swap_total.saturating_sub(s.mem.swap_free);
    let swap_pct = if s.mem.swap_total > 0 { swap_used as f32 / s.mem.swap_total as f32 * 100.0 } else { 0.0 };
    let freq_avg = s.cpu.freqs.iter().sum::<f32>() / s.cpu.freqs.len().max(1) as f32;
    let temp = s.cpu.temp.unwrap_or(0.0);

    let cpu_lines = vec![
        Line::new(hv(st, "cpu.user"), th.cyan).fill(),
        Line::new(hv(st, "cpu.system"), th.magenta).fill(),
        Line::new(hv(st, "cpu.iowait"), th.amber).fill(),
    ];
    let mem_lines = vec![
        Line::new(hv(st, "mem.apps"), th.magenta).fill(),
        Line::new(hv(st, "mem.shmem"), th.violet).fill(),
        Line::new(hv(st, "mem.buffers"), th.amber).fill(),
        Line::new(hv(st, "mem.cache"), th.cyan).fill(),
    ];
    let phys: Vec<_> = s.net.ifaces.iter().filter(|i| i.kind != "loopback" && !i.name.starts_with("veth") && !i.name.starts_with("docker")).collect();
    let rx_keys: Vec<String> = phys.iter().map(|i| format!("net.{}.rx", i.name)).collect();
    let tx_keys: Vec<String> = phys.iter().map(|i| format!("net.{}.tx", i.name)).collect();
    let (rx_now, tx_now) = phys.iter().fold((0.0, 0.0), |a, i| (a.0 + i.rx_rate, a.1 + i.tx_rate));
    let net_lines = vec![Line::new(hsum(st, &rx_keys), th.cyan).fill(), Line::new(hsum(st, &tx_keys), th.magenta).fill().negate()];
    let disks: Vec<_> = s.disks.disks.iter().collect();
    let dr: Vec<String> = disks.iter().map(|d| format!("disk.{}.r", d.name)).collect();
    let dw: Vec<String> = disks.iter().map(|d| format!("disk.{}.w", d.name)).collect();
    let (r_now, w_now) = disks.iter().fold((0.0, 0.0), |a, d| (a.0 + d.read_rate, a.1 + d.write_rate));
    let disk_lines = vec![Line::new(hsum(st, &dr), th.cyan).fill(), Line::new(hsum(st, &dw), th.magenta).fill().negate()];
    let load_lines = vec![
        Line::new(hv(st, "load.0"), th.lime).fill(),
        Line::new(hv(st, "load.1"), th.amber),
        Line::new(hv(st, "load.2"), th.red).dashed(),
    ];
    // procesos que más gastan (si la lista está disponible)
    let procs = st.model.procs.list.clone();
    let _ = st;

    let gauge = |title: &str, accent, frac: f32, value: String, sub: String, label: &str| {
        Panel::new(title.to_string(), accent).child(charts::gauge(frac, accent, value, sub, label.to_string(), 170.))
    };

    let cores = div().grid().grid_cols(if kit::cols(cx) >= 2 { 2 } else { 1 }).gap_x(px(16.)).gap_y(px(5.)).children(s.cpu.cores.iter().enumerate().map(|(i, c)| {
        let color = th.load_color(c.usage);
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(11.))
            .font_family(th.mono.clone())
            .child(div().w(px(30.)).text_color(th.muted).child(format!("C{i}")))
            .child(charts::blocks(c.usage / 100.0, color, 24, 10.))
            .child(div().w(px(110.)).flex().justify_end().text_color(color).child(format!("{:>3.0}% {}", c.usage, fmt::freq(*s.cpu.freqs.get(i).unwrap_or(&0.0) as f64))))
    }));

    let psi = [(t!("CPU"), s.pressure.cpu.some, th.cyan), (t!("MEM"), s.pressure.memory.some, th.magenta), ("I/O", s.pressure.io.some, th.amber)];
    let psi_rows = div().flex().flex_col().gap(px(6.)).children(psi.into_iter().map(|(l, v, c)| {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .font_family(th.mono.clone())
            .text_size(px(11.))
            .child(div().w(px(34.)).text_color(th.muted).child(l))
            .child(charts::blocks(v / 100.0, c, 20, 10.))
            .child(div().w(px(56.)).flex().justify_end().text_color(c).child(format!("{v:.2}%")))
    }));

    let temps: Vec<(String, f64, f64)> = s
        .sensors
        .chips
        .iter()
        .flat_map(|chip| chip.readings.iter().filter(|r| r.kind == "temp").map(move |r| (format!("{} · {}", chip.label, r.label), r.value, r.crit.unwrap_or(100.0))))
        .take(8)
        .collect();
    let temp_rows = div().flex().flex_col().gap(px(5.)).children(temps.into_iter().map(|(label, v, crit)| {
        let c = th.temp_color(v as f32);
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .font_family(th.mono.clone())
            .text_size(px(11.))
            .child(div().w(px(170.)).truncate().text_color(th.muted).child(label))
            .child(charts::blocks((v / crit) as f32, c, 30, 9.))
            .child(div().w(px(60.)).flex().justify_end().text_color(c).child(format!("{v:.1}°C")))
    }));

    let top = |by_mem: bool| {
        let mut v: Vec<_> = procs.procs.iter().collect();
        if by_mem {
            v.sort_by(|a, b| b.mem.cmp(&a.mem));
        } else {
            v.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        }
        let max = if by_mem { v.first().map(|p| p.mem as f32).unwrap_or(1.0) } else { 100.0 };
        div().flex().flex_col().gap(px(4.)).children(v.into_iter().take(6).map(|p| {
            let (frac, val, color) = if by_mem {
                (p.mem as f32 / max.max(1.0), fmt::bytes(p.mem as f64), th.magenta)
            } else {
                ((p.cpu / 100.0).min(1.0), format!("{:.1}%", p.cpu), th.load_color(p.cpu))
            };
            kit::meter_row(p.name.clone(), frac, val, color, cx)
        }))
    };

    kit::grid(cx)
        .child(gauge(t!("CPU"), th.load_color(s.cpu.total.usage), s.cpu.total.usage / 100.0, fmt::pct(s.cpu.total.usage as f64, 0), fmt::freq(freq_avg as f64), t!("USO")))
        .child(gauge(t!("Memoria"), th.magenta, mem_pct / 100.0, fmt::pct(mem_pct as f64, 0), fmt::bytes_of(seg.used as f64, s.mem.total as f64), t!("RAM")))
        .child(gauge(
            t!("Swap"),
            th.violet,
            swap_pct / 100.0,
            if s.mem.swap_total > 0 { fmt::pct(swap_pct as f64, 0) } else { t!("N/D").into() },
            fmt::bytes_of(swap_used as f64, s.mem.swap_total as f64),
            t!("SWAP"),
        ))
        .child(gauge(
            t!("Temp. CPU"),
            th.temp_color(temp),
            temp / 100.0,
            s.cpu.temp.map(|t| format!("{t:.1}°")).unwrap_or(t!("N/D").into()),
            s.cpu.temps.iter().map(|t| format!("{} {:.0}°", t.label, t.celsius)).collect::<Vec<_>>().join("  "),
            "°C",
        ))
        .child(Panel::new(t!("Historial CPU"), th.cyan).span(2).extra(t!("user · system · iowait")).child(Plot::new(cpu_lines, t.clone()).stacked().y_max(100.).y_fmt(y_pct).height(200.).into_element()))
        .child(Panel::new(t!("Núcleos"), th.lime).span(2).extra(t!("{} físicos · {} hilos", info.physical_cores, info.logical_cores)).child(cores))
        .child(
            Panel::new(t!("Memoria"), th.magenta)
                .span(2)
                .extra(t!("apps · shmem · buffers · caché"))
                .child(Plot::new(mem_lines, t.clone()).stacked().y_max(s.mem.total as f32).y_fmt(y_bytes).bytes().height(200.).into_element()),
        )
        .child(
            Panel::new(t!("Red"), th.teal)
                .span(2)
                .extra(kit::legend(vec![(format!("▼ {}", fmt::rate(rx_now)), th.cyan), (format!("▲ {}", fmt::rate(tx_now)), th.magenta)]))
                .child(Plot::new(net_lines, t.clone()).y_fmt(y_rate).bytes().height(200.).into_element()),
        )
        .child(
            Panel::new(t!("Presión (PSI)"), th.amber)
                .extra(t!("avg10 · some"))
                .child(psi_rows)
                .child(kit::muted(t!("% de tiempo en que alguna tarea estuvo esperando por el recurso."), cx)),
        )
        .child(
            Panel::new(t!("Carga media"), th.lime)
                .extra(s.cpu.load_avg.iter().map(|l| format!("{l:.2}")).collect::<Vec<_>>().join(" · "))
                .child(Plot::new(load_lines, t.clone()).height(150.).into_element()),
        )
        .child(
            Panel::new(t!("Sistema"), th.blue).span(2).child(kit::kv_grid(
                vec![
                    (t!("Host").into(), info.hostname.clone()),
                    (t!("SO").into(), info.os.clone()),
                    (t!("Kernel").into(), info.kernel.clone()),
                    (t!("CPU").into(), info.cpu_model.clone()),
                    (t!("Procesos").into(), t!("{} total · {} ejecutando · {} bloqueados", s.cpu.procs_total, s.cpu.procs_running, s.cpu.procs_blocked)),
                    (t!("Sockets").into(), t!("{} TCP establecidos · {} escuchando · {} UDP", s.net.tcp_established, s.net.tcp_listen, s.net.udp_sockets)),
                ],
                true,
                cx,
            )),
        )
        .child(
            Panel::new(t!("Discos"), th.amber)
                .span(2)
                .extra(kit::legend(vec![(format!("▼ {}", fmt::rate(r_now)), th.cyan), (format!("▲ {}", fmt::rate(w_now)), th.magenta)]))
                .child(Plot::new(disk_lines, t.clone()).y_fmt(y_rate).bytes().height(180.).into_element()),
        )
        .child(Panel::new(t!("Temperaturas"), th.red).span(2).child(temp_rows))
        .when(!procs.procs.is_empty(), |g| {
            g.child(
                Panel::new(t!("Top procesos"), th.lime).span(4).extra(t!("por CPU (% de un núcleo) · por memoria")).child(
                    div().grid().grid_cols(if kit::cols(cx) >= 2 { 2 } else { 1 }).gap(px(24.)).child(top(false)).child(top(true)),
                ),
            )
        })
        .into_any_element()
}
