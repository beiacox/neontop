//! GPU: elegir la tarjeta, cifras, motores, procesos que la usan, tráfico de VRAM y PCIe,
//! memoria de vídeo, potencia, relojes y datos de la tarjeta.

use neontop_core::t;
use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, Hsla, SharedString, Window};
use neontop_core::format as fmt;

use super::{ht, hv, y_mhz, y_pct, y_rate};
use crate::app::Root;
use crate::charts::{self, Line, Plot};
use crate::ui::kit::{self, Panel};
use crate::ui::theme::{fade, hex, Th};

pub fn engine_name(e: &str) -> &str {
    match e {
        "gfx" | "3d" => t!("Gráficos 3D"),
        "render" | "rcs" => t!("Render 3D"),
        "compute" | "ccs" => t!("Cómputo"),
        "dec" => t!("Decodificación"),
        "enc" => t!("Codificación"),
        "enc_1" => t!("Codificación 2"),
        "vcn" => t!("Vídeo (VCN)"),
        "jpeg" => t!("JPEG"),
        "video" | "vcs" => t!("Vídeo"),
        "video-enhance" | "vecs" => t!("Mejora de vídeo"),
        "dma" => t!("Copia (DMA)"),
        "copy" | "bcs" => t!("Copia"),
        other => other,
    }
}

fn vendor(v: &str) -> (&'static str, u32) {
    match v {
        "amd" => (t!("AMD"), 0xed1c24),
        "nvidia" => (t!("NVIDIA"), 0x76b900),
        "intel" => (t!("Intel"), 0x0071c5),
        _ => (t!("GPU"), 0x888888),
    }
}

fn pcie_gen(gt: Option<f32>) -> &'static str {
    match gt {
        None => "?",
        Some(g) if g >= 64.0 => "6.0",
        Some(g) if g >= 32.0 => "5.0",
        Some(g) if g >= 16.0 => "4.0",
        Some(g) if g >= 8.0 => "3.0",
        Some(g) if g >= 5.0 => "2.0",
        Some(_) => "1.0",
    }
}

fn gbs(v: Option<f64>, d: usize) -> String {
    v.map(|v| format!("{:.*} GB/s", d, v / 1e9)).unwrap_or(t!("N/D").into())
}

pub fn render(root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    let th = Th::get(cx).clone();
    let st = root.state.read(cx);
    let s = st.model.snap.clone();
    if s.gpus.is_empty() {
        return div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(kit::empty(t!("No se ha detectado ninguna GPU compatible."), cx))
            .child(kit::muted(t!("NeonTop soporta AMD (amdgpu), NVIDIA (driver propietario, vía NVML) e Intel (i915/xe)."), cx))
            .into_any_element();
    }
    let idx = root.gpu_sel.min(s.gpus.len() - 1);
    let g = s.gpus[idx].clone();
    let k = |n: &str| format!("gpu.{}.{}", idx, n);
    let t = ht(st);
    let bench_peak = st.membench.last().map(|b| b.read.max(b.copy));

    let picker = div().col_span(kit::cols(cx) as u16).flex().flex_wrap().gap(px(8.)).children(s.gpus.iter().enumerate().map(|(i, x)| {
        let (vl, vc) = vendor(&x.vendor);
        let on = i == idx;
        div()
            .id(SharedString::from(format!("gpu-{i}")))
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(px(6.))
            .cursor_pointer()
            .border_1()
            .border_color(if on { th.cyan } else { th.border(th.cyan) })
            .rounded(px(th.radius.min(6.)))
            .when(on, |d| d.bg(fade(th.cyan, 0.10)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.gpu_sel = i;
                cx.notify();
            }))
            .child(div().px(px(5.)).rounded(px(3.)).bg(hex(vc)).text_color(gpui::white()).text_size(px(10.)).font_weight(FontWeight::BOLD).child(vl))
            .child(div().font_weight(FontWeight::BOLD).child(x.name.clone()))
            .child(kit::muted(
                format!("{}{}{} · {}", if x.integrated { "integrada · " } else { "" }, x.driver, x.driver_version.as_ref().map(|v| format!(" {v}")).unwrap_or_default(), x.pci_slot),
                cx,
            ))
    }));
    let opt = |v: Option<f32>, u: &str| v.map(|v| format!("{v:.0}{u}")).unwrap_or(t!("N/D").into());
    let stats = vec![
        kit::stat(t!("Uso GPU"), g.busy.map(|b| fmt::pct(b as f64, 0)).unwrap_or(t!("N/D").into()), "", th.load_color(g.busy.unwrap_or(0.0)), Some(hv(st, &k("busy"))), cx).into_any_element(),
        kit::stat(t!("Controlador VRAM"), g.mem_busy.map(|b| fmt::pct(b as f64, 0)).unwrap_or(t!("N/D").into()), t!("actividad del bus de memoria"), th.violet, Some(hv(st, &k("memBusy"))), cx).into_any_element(),
        kit::stat(t!("Tráfico VRAM"), gbs(g.vram_bw, 2), t!("estimado"), th.magenta, Some(hv(st, &k("vramBw"))), cx).into_any_element(),
        kit::stat(
            if g.integrated { t!("Memoria GPU") } else { t!("VRAM usada") },
            fmt::bytes(g.vram_used as f64),
            if g.vram_total > 0 { t!("de {}", fmt::bytes_digits(g.vram_total as f64, 0)) } else { t!("suma de procesos").into() },
            th.cyan,
            Some(hv(st, &k("vramUsed"))),
            cx,
        )
        .into_any_element(),
        kit::stat(t!("GTT"), fmt::bytes(g.gtt_used as f64), t!("RAM del sistema usada por la GPU"), th.amber, Some(hv(st, &k("gttUsed"))), cx).into_any_element(),
        kit::stat(t!("Reloj núcleo"), opt(g.gfx_clock, t!(" MHz")), g.gfx_clock_max.map(|m| t!("máx. {} MHz", format!("{:.0}", m))).unwrap_or_default(), th.teal, Some(hv(st, &k("gfxClock"))), cx).into_any_element(),
        kit::stat(t!("Potencia"), opt(g.power, " W"), g.power_limit.map(|m| t!("límite {} W", format!("{:.0}", m))).unwrap_or_default(), th.orange, Some(hv(st, &k("power"))), cx).into_any_element(),
        kit::stat(
            t!("Temperatura"),
            opt(g.temp_edge, "°C"),
            g.temp_hotspot.map(|h| t!("unión {}° · mem {}°", format!("{:.0}", h), g.temp_mem.map(|m| format!("{m:.0}")).unwrap_or("?".into()))).unwrap_or_default(),
            th.temp_color(g.temp_edge.unwrap_or(0.0)),
            Some(hv(st, &k("tempEdge"))),
            cx,
        )
        .into_any_element(),
    ];
    let mut grid = kit::grid(cx).child(picker).child(kit::stats_row(stats, cx));
    if !g.throttle.is_empty() {
        grid = grid.child(
            div()
                .col_span(kit::cols(cx) as u16)
                .flex()
                .gap(px(8.))
                .px(px(10.))
                .py(px(6.))
                .border_1()
                .border_color(th.amber)
                .text_color(th.amber)
                .child(t!("⚠ Bajando relojes por:"))
                .children(g.throttle.iter().map(|x| div().font_weight(FontWeight::BOLD).child(x.clone()))),
        );
    }
    let engines = div().flex().flex_col().gap(px(6.)).children(g.engines.iter().map(|e| kit::meter_row(format!("{} · {}", engine_name(&e.name), e.name), e.busy / 100.0, format!("{:.1}%", e.busy), th.load_color(e.busy), cx)));
    let eng_lines: Vec<Line> = g.engines.iter().enumerate().map(|(i, e)| {
        let l = Line::new(hv(st, &format!("gpu.{}.eng.{}", idx, e.name)), [th.lime, th.cyan, th.magenta, th.amber, th.violet, th.teal][i % 6]);
        if i == 0 { l.fill() } else { l }
    }).collect();
    grid = grid
        .child(Panel::new(t!("Motores"), th.lime).span(2).extra(if g.processes_sampled { t!("desglose por motor (fdinfo)") } else { t!("contadores globales") }).child(if g.engines.is_empty() {
            kit::muted(t!("El driver no informa del uso por motor."), cx).into_any_element()
        } else {
            engines.into_any_element()
        }))
        .child(Panel::new(t!("Motores en el tiempo"), th.lime).span(2).child(Plot::new(eng_lines, t.clone()).y_max(100.).y_fmt(y_pct).height(200.).into_element()));

    // procesos que usan la GPU
    let busy_of = |p: &neontop_core::collectors::gpu::GpuProc| p.engines.iter().map(|e| e.busy).sum::<f32>();
    let mut procs = g.processes.clone();
    if root.gpu_sort_busy {
        procs.sort_by(|a, b| busy_of(b).total_cmp(&busy_of(a)));
    } else {
        procs.sort_by(|a, b| (b.vram + b.gtt).cmp(&(a.vram + a.gtt)));
    }
    let mut eng_cols: Vec<String> = procs.iter().flat_map(|p| p.engines.iter().map(|e| e.name.clone())).collect();
    eng_cols.sort();
    eng_cols.dedup();
    let sort_busy = root.gpu_sort_busy;
    let sort_btns = div()
        .flex()
        .gap(px(6.))
        .child("ordenar:")
        .child(div().id("gs-vram").cursor_pointer().text_color(if !sort_busy { th.cyan } else { th.muted }).child("memoria").on_click(cx.listener(|this, _, _, cx| {
            this.gpu_sort_busy = false;
            cx.notify();
        })))
        .child(div().id("gs-busy").cursor_pointer().text_color(if sort_busy { th.cyan } else { th.muted }).child("uso").on_click(cx.listener(|this, _, _, cx| {
            this.gpu_sort_busy = true;
            cx.notify();
        })));
    let head = div()
        .flex()
        .text_size(px(10.))
        .text_color(th.muted)
        .font_family(th.mono.clone())
        .border_b_1()
        .border_color(th.border(th.cyan))
        .pb(px(3.))
        .child(div().flex_1().min_w(px(120.)).child(th.title(t!("Proceso"))))
        .child(div().w(px(64.)).flex().justify_end().child(t!("PID")))
        .child(div().w(px(90.)).flex().justify_end().child(t!("VRAM")))
        .child(div().w(px(90.)).flex().justify_end().child(t!("GTT")))
        .children(eng_cols.iter().map(|e| div().w(px(96.)).flex().justify_end().truncate().child(engine_name(e).to_string())));
    let rows = procs.iter().take(40).map(|p| {
        div()
            .flex()
            .items_center()
            .h(px(22.))
            .font_family(th.mono.clone())
            .text_size(px(11.))
            .child(div().flex_1().min_w(px(120.)).truncate().child(if p.name.is_empty() { "?".into() } else { p.name.clone() }))
            .child(div().w(px(64.)).flex().justify_end().text_color(th.muted).child(p.pid.to_string()))
            .child(div().w(px(90.)).flex().justify_end().text_color(th.magenta).child(fmt::bytes(p.vram as f64)))
            .child(div().w(px(90.)).flex().justify_end().text_color(th.amber).child(fmt::bytes(p.gtt as f64)))
            .children(eng_cols.iter().map(|e| {
                let v = p.engines.iter().find(|x| &x.name == e).map(|x| x.busy).unwrap_or(0.0);
                div().w(px(96.)).pl(px(10.)).flex().items_center().gap(px(4.)).child(div().flex_1().child(charts::meter(v / 100.0, th.load_color(v), 4.))).child(div().w(px(40.)).flex().justify_end().child(format!("{v:.1}%")))
            }))
    });
    let proc_body: AnyElement = if !g.processes_sampled {
        kit::muted(t!("Midiendo…"), cx).into_any_element()
    } else if procs.is_empty() {
        kit::muted(t!("Ningún proceso visible está usando esta GPU."), cx).into_any_element()
    } else {
        div()
            .flex()
            .flex_col()
            .child(head)
            .children(rows)
            .child(kit::muted(
                if g.vendor == "nvidia" { t!("Datos de NVML.") } else { t!("Datos de /proc/*/fdinfo: solo se ven los procesos de tu usuario (los de root, como algunos servidores gráficos, no aparecen).") },
                cx,
            ))
            .into_any_element()
    };
    let mut vram_rows: Vec<(String, u64)> = g.processes.iter().filter(|p| p.vram > 0).map(|p| (p.name.clone(), p.vram)).collect();
    vram_rows.sort_by(|a, b| b.1.cmp(&a.1));
    let vtot = g.vram_total.max(vram_rows.iter().map(|v| v.1).sum()).max(1);
    let vram_panel: AnyElement = if vram_rows.is_empty() {
        kit::muted(t!("Ningún proceso visible usa VRAM ahora mismo."), cx).into_any_element()
    } else {
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(charts::ring(vram_rows.iter().take(8).enumerate().map(|(i, v)| (v.1 as f32 / vtot as f32, th.series(i))).collect(), fmt::bytes(g.vram_used as f64), t!("VRAM").into(), 160.))
            .children(vram_rows.iter().take(8).enumerate().map(|(i, v)| {
                div().flex().items_center().gap(px(6.)).text_size(px(11.)).child(div().size(px(7.)).bg(th.series(i))).child(div().flex_1().truncate().child(v.0.clone())).child(div().font_family(th.mono.clone()).child(fmt::bytes(v.1 as f64)))
            }))
            .into_any_element()
    };
    let bw_pct = match (g.vram_bw, g.vram_peak_bw) {
        (Some(b), Some(p)) if p > 0.0 => (b / p * 100.0) as f32,
        _ => 0.0,
    };
    let mut vram_plot = Plot::new(vec![Line::new(hv(st, &k("vramBw")), th.magenta).fill().width(2.)], t.clone()).y_fmt(|v| format!("{:.0} GB/s", v / 1e9)).height(240.);
    if let Some(p) = g.vram_peak_bw {
        vram_plot = vram_plot.y_max(p as f32);
    }
    grid = grid
        .child(Panel::new(t!("Procesos en la GPU"), th.cyan).span(3).extra(sort_btns).child(div().id("gproc").overflow_x_scroll().child(div().min_w(px(520.)).child(proc_body))))
        .child(Panel::new(t!("VRAM por proceso"), th.magenta).child(vram_panel))
        .child(
            Panel::new(t!("Tráfico de VRAM"), th.magenta)
                .span(3)
                .extra(kit::legend(vec![(t!("pico teórico {}", gbs(g.vram_peak_bw, 0)), th.muted), (t!("ahora {}", gbs(g.vram_bw, 2)), th.magenta)]))
                .child(vram_plot.into_element())
                .child(kit::muted(
                    t!("Estimación: actividad del controlador de memoria × ancho de banda al reloj actual ({}, {} bits). El eje llega al máximo teórico para ver la proporción.", g.vram_type.clone().unwrap_or("?".into()), g.bus_width.map(|b| b.to_string()).unwrap_or("?".into())),
                    cx,
                )),
        )
        .child(Panel::new(t!("Uso del bus de VRAM"), th.violet).child(charts::gauge(bw_pct / 100.0, th.magenta, fmt::pct(bw_pct as f64, 1), gbs(g.vram_bw, 2), t!("DEL MÁXIMO").into(), 250.)));
    if g.pcie_rx.is_some() {
        grid = grid.child(
            Panel::new(t!("Tráfico PCIe"), th.amber)
                .span(3)
                .extra(kit::legend(vec![(t!("▼ hacia la GPU {}", fmt::rate(g.pcie_rx.unwrap_or(0.0))), th.cyan), (t!("▲ desde la GPU {}", fmt::rate(g.pcie_tx.unwrap_or(0.0))), th.amber)]))
                .child(Plot::new(vec![Line::new(hv(st, &k("pcieRx")), th.cyan).fill(), Line::new(hv(st, &k("pcieTx")), th.amber).fill().negate()], t.clone()).y_fmt(y_rate).bytes().height(200.).into_element()),
        );
    }
    let pcie_info: Vec<(String, String)> = if g.integrated {
        vec![(t!("Enlace").into(), t!("integrada (sin PCIe externo)").into())]
    } else {
        vec![
            (t!("Enlace").into(), format!("PCIe {} x{} · {} GT/s", pcie_gen(g.pcie_speed), g.pcie_width.map(|w| w.to_string()).unwrap_or("?".into()), g.pcie_speed.map(|s| format!("{s}")).unwrap_or("?".into()))),
            (t!("Máximo").into(), format!("PCIe {} x{}", pcie_gen(g.pcie_max_speed), g.pcie_max_width.map(|w| w.to_string()).unwrap_or("?".into()))),
            (t!("Pico").into(), t!("{} por sentido", gbs(g.pcie_peak_bw, 1))),
            (t!("Tráfico").into(), g.pcie_rx.map(|rx| format!("▼ {} · ▲ {}", fmt::rate(rx), fmt::rate(g.pcie_tx.unwrap_or(0.0)))).unwrap_or(t!("no expuesto por el driver").into())),
        ]
    };
    grid = grid.child(
        Panel::new(t!("Bus PCIe"), th.amber)
            .child(kit::kv_grid(pcie_info, false, cx))
            .when(g.pcie_rx.is_none() && !g.integrated, |p| p.child(kit::muted(t!("El tráfico PCIe en vivo solo lo publica NVML (NVIDIA); amdgpu e Intel no lo exponen en estas generaciones."), cx))),
    );
    // anchos de banda del sistema (escala logarítmica): VRAM, PCIe y RAM
    let mut bws: Vec<(&str, f64, Hsla)> = Vec::new();
    if let Some(v) = g.vram_peak_bw {
        bws.push((t!("VRAM (pico)"), v, th.magenta));
    }
    if let Some(v) = g.vram_bw {
        bws.push((t!("VRAM (ahora)"), v, th.violet));
    }
    if let Some(v) = bench_peak {
        bws.push((t!("RAM (test)"), v, th.cyan));
    }
    if let Some(v) = g.pcie_peak_bw {
        bws.push((t!("PCIe (pico)"), v, th.amber));
    }
    let bmax = bws.iter().map(|b| b.1).fold(1e9, f64::max).log10();
    let bw_rows = div().flex().flex_col().gap(px(6.)).children(bws.iter().map(|(l, v, c)| kit::meter_row(*l, ((v.max(1e6)).log10() - 6.0) as f32 / (bmax - 6.0).max(0.1) as f32, gbs(Some(*v), 1), *c, cx)));
    let vram_frac = g.vram_used as f32 / g.vram_total.max(1) as f32;
    let gtt_frac = g.gtt_used as f32 / g.gtt_total.max(1) as f32;
    grid.child(
        Panel::new(t!("GPU vs memoria"), th.cyan)
            .span(if g.pcie_rx.is_some() { 4 } else { 3 })
            .extra(kit::legend(vec![(t!("motor gráfico").into(), th.cyan), (t!("controlador VRAM").into(), th.violet)]))
            .child(Plot::new(vec![Line::new(hv(st, &k("busy")), th.cyan).fill(), Line::new(hv(st, &k("memBusy")), th.violet)], t.clone()).y_max(100.).y_fmt(y_pct).height(200.).into_element()),
    )
    .child(
        Panel::new(t!("Anchos de banda del sistema"), th.amber)
            .span(2)
            .extra(t!("escala logarítmica"))
            .child(bw_rows)
            .when(bench_peak.is_none(), |p| p.child(kit::muted(t!("Ejecuta el test de RAM (pestaña Memoria) para compararla aquí."), cx))),
    )
    .child(
        Panel::new(t!("Memoria de vídeo"), th.magenta)
            .extra(kit::legend(vec![(t!("VRAM").into(), th.magenta), (t!("GTT").into(), th.amber)]))
            .child(charts::ring(vec![(vram_frac, th.magenta)], fmt::pct(vram_frac as f64 * 100.0, 0), fmt::bytes_of(g.vram_used as f64, g.vram_total as f64), 170.))
            .child(kit::meter_row(t!("GTT"), gtt_frac, fmt::bytes_of(g.gtt_used as f64, g.gtt_total as f64), th.amber, cx)),
    )
    .child(Panel::new(t!("Potencia"), th.orange).child(charts::gauge(
        g.power.unwrap_or(0.0) / g.power_limit.unwrap_or(300.0).max(1.0),
        th.orange,
        opt(g.power, " W"),
        g.power_limit.map(|l| t!("límite {} W", format!("{:.0}", l))).unwrap_or_default(),
        t!("POTENCIA").into(),
        220.,
    )))
    .child(
        Panel::new(t!("Relojes"), th.teal)
            .span(2)
            .extra(kit::legend(vec![(t!("núcleo").into(), th.teal), ("memoria".into(), th.amber)]))
            .child(Plot::new(vec![Line::new(hv(st, &k("gfxClock")), th.teal).fill(), Line::new(hv(st, &k("memClock")), th.amber)], t).y_fmt(y_mhz).height(180.).into_element()),
    )
    .child(Panel::new(t!("Tarjeta"), th.blue).span(2).child(kit::kv_grid(
        vec![
            (t!("Modelo").into(), g.name.clone()),
            (t!("Driver").into(), format!("{}{}", g.driver, g.driver_version.as_ref().map(|v| format!(" {v}")).unwrap_or_default())),
            (
                t!("Memoria").into(),
                format!(
                    "{} {}{}{}",
                    if g.vram_total > 0 { fmt::bytes_digits(g.vram_total as f64, 0) } else { t!("compartida").into() },
                    g.vram_type.clone().unwrap_or_default(),
                    g.bus_width.map(|b| format!(" · bus de {b} bits")).unwrap_or_default(),
                    g.vram_peak_bw.map(|p| format!(" · {}", gbs(Some(p), 0))).unwrap_or_default()
                ),
            ),
            (t!("Ranura").into(), g.pci_slot.clone()),
            (
                t!("Ventilador").into(),
                match (g.fan_rpm, g.fan_pct) {
                    (None, None) => "—".into(),
                    (r, p) => format!("{}{}", r.map(|r| format!("{r} RPM")).unwrap_or_default(), p.map(|p| format!(" ({p:.0}%)")).unwrap_or_default()),
                },
            ),
            (
                t!("Temperaturas").into(),
                [g.temp_edge.map(|t| t!("borde {}°", format!("{:.0}", t))), g.temp_hotspot.map(|t| t!("unión {}°", format!("{:.0}", t))), g.temp_mem.map(|t| t!("memoria {}°", format!("{:.0}", t)))].into_iter().flatten().collect::<Vec<_>>().join(" · "),
            ),
        ],
        false,
        cx,
    )))
    .into_any_element()
}
