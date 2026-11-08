//! Discos: cifras totales, tarjeta por disco (elegir uno), E/S, ocupación, operaciones,
//! latencia, mapa de actividad y sistemas de ficheros.

use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, SharedString, Window};
use neontop_core::format as fmt;

use super::{hsum, ht, hv, y_num, y_rate};
use crate::app::Root;
use crate::charts::{self, Line, Plot};
use crate::ui::kit::{self, Panel};
use crate::ui::theme::{fade, Th};

fn kind(k: &str) -> (&'static str, &'static str) {
    match k {
        "nvme" => ("▰", "NVMe"),
        "ssd" => ("▭", "SSD SATA"),
        "hdd" => ("◉", "HDD"),
        "usb" => ("⏏", "USB"),
        "virtual" => ("◇", "Virtual"),
        _ => ("◇", "Disco"),
    }
}

pub fn render(root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    let th = Th::get(cx).clone();
    let st = root.state.read(cx);
    let s = st.model.snap.clone();
    let t = ht(st);
    let show_virtual = root.disk_virtual;
    let disks: Vec<_> = s.disks.disks.iter().filter(|d| show_virtual || d.kind != "virtual").collect();
    let real: Vec<_> = s.disks.disks.iter().filter(|d| d.kind != "virtual").collect();
    let sel = disks.iter().find(|d| Some(&d.name) == root.disk_sel.as_ref()).or_else(|| disks.first()).map(|d| (*d).clone());
    let (read, write) = real.iter().fold((0.0, 0.0), |a, d| (a.0 + d.read_rate, a.1 + d.write_rate));
    let iops: f64 = real.iter().map(|d| d.read_iops + d.write_iops).sum();
    let busiest = real.iter().max_by(|a, b| a.util.total_cmp(&b.util));
    let max_temp = real.iter().flat_map(|d| d.temps.iter().map(|t| t.celsius)).fold(None, |a: Option<f32>, t| Some(a.map_or(t, |a| a.max(t))));
    let (fs_used, fs_total) = s.disks.filesystems.iter().fold((0u64, 0u64), |a, f| (a.0 + f.used, a.1 + f.total));
    let capacity: u64 = real.iter().map(|d| d.size).sum();
    let rkeys: Vec<String> = real.iter().map(|d| format!("disk.{}.r", d.name)).collect();
    let wkeys: Vec<String> = real.iter().map(|d| format!("disk.{}.w", d.name)).collect();
    let (rsum, wsum) = (hsum(st, &rkeys), hsum(st, &wkeys));
    let stats = vec![
        kit::stat("Lectura", fmt::rate(read), "", th.cyan, Some(rsum.clone()), cx).into_any_element(),
        kit::stat("Escritura", fmt::rate(write), "", th.magenta, Some(wsum.clone()), cx).into_any_element(),
        kit::stat("IOPS", fmt::num(iops), "lecturas + escrituras/s", th.lime, None, cx).into_any_element(),
        kit::stat(
            "Más ocupado",
            busiest.map(|d| fmt::pct(d.util as f64, 0)).unwrap_or("—".into()),
            busiest.map(|d| d.name.clone()).unwrap_or_default(),
            th.load_color(busiest.map(|d| d.util).unwrap_or(0.0)),
            None,
            cx,
        )
        .into_any_element(),
        kit::stat("Temp. máx.", max_temp.map(|t| format!("{t:.0}°C")).unwrap_or("N/D".into()), "discos con sensor", th.temp_color(max_temp.unwrap_or(0.0)), None, cx).into_any_element(),
        kit::stat(
            "Espacio usado",
            fmt::pct(fs_used as f64 / fs_total.max(1) as f64 * 100.0, 0),
            format!("{} / {}", fmt::bytes_digits(fs_used as f64, 0), fmt::bytes_digits(fs_total as f64, 0)),
            th.amber,
            None,
            cx,
        )
        .into_any_element(),
        kit::stat("Discos", real.len().to_string(), format!("{} en total", fmt::bytes(capacity as f64)), th.violet, None, cx).into_any_element(),
        kit::stat("Sist. ficheros", s.disks.filesystems.len().to_string(), "montados", th.teal, None, cx).into_any_element(),
    ];

    let per_row = (kit::cols(cx) * 2).clamp(1, 4) as u16;
    let cards = div()
        .col_span(kit::cols(cx) as u16)
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(div().grid().grid_cols(per_row).gap(px(10.)).children(disks.iter().map(|d| {
            let on = sel.as_ref().is_some_and(|s| s.name == d.name);
            let name = d.name.clone();
            let (icon, label) = kind(&d.kind);
            let temp = d.temps.iter().find(|t| t.label == "Composite").or(d.temps.first());
            div()
                .id(SharedString::from(format!("disk-{}", d.name)))
                .flex()
                .flex_col()
                .gap(px(4.))
                .p(px(10.))
                .cursor_pointer()
                .bg(th.panel)
                .border_1()
                .border_color(if on { th.cyan } else { th.border(th.cyan) })
                .rounded(px(th.radius.min(6.)))
                .when(on, |e| e.bg(fade(th.cyan, 0.08)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.disk_sel = Some(name.clone());
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().text_color(th.cyan).child(icon))
                        .child(div().font_weight(FontWeight::BOLD).child(d.name.clone()))
                        .child(kit::badge(label, th.muted))
                        .child(div().flex_1())
                        .children(temp.map(|t| div().text_color(th.temp_color(t.celsius)).font_family(th.mono.clone()).child(format!("{:.0}°", t.celsius)))),
                )
                .child(kit::muted(format!("{} · {}", if d.model.is_empty() { "—" } else { &d.model }, fmt::bytes_digits(d.size as f64, 0)), cx).truncate())
                .child(charts::meter(d.util / 100.0, th.load_color(d.util), 4.))
                .child(charts::sparkline(hv(st, &format!("disk.{}.r", d.name)), th.cyan, 18., None))
                .child(charts::sparkline(hv(st, &format!("disk.{}.w", d.name)), th.magenta, 18., None))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .font_family(th.mono.clone())
                        .text_size(px(10.5))
                        .child(div().text_color(th.cyan).child(format!("▼{}", fmt::rate(d.read_rate))))
                        .child(div().text_color(th.magenta).child(format!("▲{}", fmt::rate(d.write_rate))))
                        .child(div().child(format!("{:.0}%", d.util))),
                )
        })))
        .child(kit::toggle("disk-virtual", show_virtual, "mostrar virtuales (zram…)", cx, cx.listener(|this, _, _, cx| {
            this.disk_virtual = !this.disk_virtual;
            cx.notify();
        })));

    let mut g = kit::grid(cx).child(kit::stats_row(stats, cx)).child(cards);
    if let Some(d) = sel {
        let k = |n: &str| format!("disk.{}.{n}", d.name);
        g = g
            .child(
                Panel::new(format!("E/S · {}", d.name), th.cyan)
                    .span(3)
                    .extra(kit::legend(vec![(format!("▼ lectura {}", fmt::rate(d.read_rate)), th.cyan), (format!("▲ escritura {}", fmt::rate(d.write_rate)), th.magenta)]))
                    .child(Plot::new(vec![Line::new(hv(st, &k("r")), th.cyan).fill().width(2.), Line::new(hv(st, &k("w")), th.magenta).fill().width(2.).negate()], t.clone()).y_fmt(y_rate).bytes().height(240.).into_element()),
            )
            .child(Panel::new("Ocupación", th.load_color(d.util)).child(charts::gauge(
                d.util / 100.0,
                th.load_color(d.util),
                fmt::pct(d.util as f64, 1),
                format!("cola {:.2}", d.queue),
                "TIEMPO OCUPADO".into(),
                250.,
            )))
            .child(
                Panel::new("Operaciones por segundo", th.lime)
                    .span(2)
                    .extra(kit::legend(vec![("lecturas/s".into(), th.lime), ("escrituras/s".into(), th.amber)]))
                    .child(Plot::new(vec![Line::new(hv(st, &k("riops")), th.lime).fill(), Line::new(hv(st, &k("wiops")), th.amber).fill().negate()], t.clone()).y_fmt(y_num).height(200.).into_element()),
            )
            .child(
                Panel::new("Latencia y cola", th.amber)
                    .span(2)
                    .extra("ms por operación · profundidad media de cola")
                    .child(
                        Plot::new(vec![Line::new(hv(st, &k("rlat")), th.cyan), Line::new(hv(st, &k("wlat")), th.magenta), Line::new(hv(st, &k("queue")), th.amber).dashed().width(1.)], t.clone())
                            .height(200.)
                            .into_element(),
                    ),
            );
    }
    let rows: Vec<Vec<f32>> = disks.iter().map(|d| hv(st, &format!("disk.{}.util", d.name))).collect();
    let labels: Vec<String> = disks.iter().map(|d| d.name.clone()).collect();
    let fs_color = |pct: f64| if pct > 90.0 { th.red } else if pct > 75.0 { th.amber } else { th.cyan };
    let fslist = div().flex().flex_col().gap(px(10.)).children(s.disks.filesystems.iter().map(|f| {
        let pct = f.used as f64 / f.total.max(1) as f64 * 100.0;
        let c = fs_color(pct);
        let mut extra = format!("{} · {}", f.device, f.fstype);
        if f.readonly {
            extra.push_str(" · solo lectura");
        }
        if !f.other_mounts.is_empty() {
            extra.push_str(&format!(" · también en {}", f.other_mounts.join(", ")));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .child(div().font_weight(FontWeight::BOLD).child(f.mount.clone()))
                    .child(kit::muted(extra, cx).flex_1().truncate())
                    .child(div().text_color(c).font_family(th.mono.clone()).child(format!("{pct:.0}%"))),
            )
            .child(charts::meter(pct as f32 / 100.0, c, 6.))
            .child(
                div()
                    .flex()
                    .gap(px(14.))
                    .child(kit::muted(format!("{} usados", fmt::bytes(f.used as f64)), cx))
                    .child(kit::muted(format!("{} libres de {}", fmt::bytes(f.avail as f64), fmt::bytes(f.total as f64)), cx))
                    .when(f.inodes_total > 0, |d| d.child(kit::muted(format!("inodos {:.0}%", (f.inodes_total - f.inodes_free) as f64 / f.inodes_total as f64 * 100.0), cx))),
            )
    }));
    // espacio por disco: cada sistema de ficheros como fracción de la capacidad del disco
    let space = div().flex().flex_col().gap(px(10.)).children(real.iter().map(|d| {
        let fss: Vec<_> = s.disks.filesystems.iter().filter(|f| f.disk.as_deref() == Some(&d.name)).collect();
        let size = d.size.max(1) as f32;
        let parts: Vec<(f32, gpui::Hsla)> = fss.iter().enumerate().flat_map(|(i, f)| [(f.used as f32 / size, th.series(i)), (f.avail as f32 / size, fade(th.series(i), 0.25))]).collect();
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(div().flex().justify_between().child(div().font_weight(FontWeight::BOLD).child(d.name.clone())).child(kit::muted(fmt::bytes(d.size as f64), cx)))
            .child(charts::segments(parts, 12.))
            .child(kit::legend(fss.iter().enumerate().map(|(i, f)| (format!("{} {}", f.mount, fmt::bytes(f.used as f64)), th.series(i))).collect()))
    }));
    g.child(Panel::new("Mapa de actividad", th.violet).span(4).extra("% de tiempo ocupado de cada disco").child(charts::heatmap(rows, labels, (disks.len() as f32 * 22.0).max(90.))))
        .child(Panel::new("Sistemas de ficheros", th.amber).span(2).child(fslist))
        .child(Panel::new("Espacio por disco", th.magenta).span(2).extra("usado (lleno) · libre (tenue)").child(space))
        .child(
            Panel::new("E/S total", th.teal)
                .span(4)
                .extra("todos los discos físicos")
                .child(Plot::new(vec![Line::new(rsum, th.cyan).fill(), Line::new(wsum, th.magenta).fill().negate()], t).y_fmt(y_rate).bytes().height(180.).into_element()),
        )
        .into_any_element()
}
