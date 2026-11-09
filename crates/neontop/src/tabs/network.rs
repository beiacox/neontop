//! Red: interfaces (elegir una), velocidades, picos, tráfico de la sesión, paquetes, uso del
//! enlace, reparto del tráfico y sockets.

use neontop_core::t;
use gpui::{div, prelude::*, px, AnyElement, Context, SharedString, Window};
use neontop_core::format as fmt;

use super::{ht, hv, y_num, y_rate};
use crate::app::Root;
use crate::charts::{self, Line, Plot};
use crate::ui::kit::{self, Panel};
use crate::ui::theme::{fade, Th};

fn kind_icon(k: &str) -> &'static str {
    match k {
        "ethernet" => "⌬",
        "wifi" => "◉",
        "loopback" => "↻",
        _ => "◇",
    }
}

fn kind_order(k: &str) -> u8 {
    match k {
        "ethernet" => 0,
        "wifi" => 1,
        "loopback" => 3,
        _ => 2,
    }
}

pub fn render(root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    let th = Th::get(cx).clone();
    let s = root.state.read(cx).model.snap.clone();
    for i in &s.net.ifaces {
        root.net_base.entry(i.name.clone()).or_insert((i.rx_total, i.tx_total));
    }
    let show_virtual = root.net_virtual;
    let mut ifaces: Vec<_> = s.net.ifaces.iter().filter(|i| show_virtual || i.kind == "ethernet" || i.kind == "wifi").collect();
    ifaces.sort_by(|a, b| kind_order(&a.kind).cmp(&kind_order(&b.kind)).then((b.state == "up").cmp(&(a.state == "up"))).then(a.name.cmp(&b.name)));
    let sel = ifaces
        .iter()
        .find(|i| Some(&i.name) == root.net_sel.as_ref())
        .or_else(|| ifaces.iter().max_by_key(|i| i.rx_total + i.tx_total))
        .map(|i| (*i).clone());
    let st = root.state.read(cx);
    let t = ht(st);

    let chips = div()
        .col_span(kit::cols(cx) as u16)
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.))
        .children(ifaces.iter().map(|i| {
            let on = sel.as_ref().is_some_and(|s| s.name == i.name);
            let name = i.name.clone();
            let down = i.state != "up" && i.state != "unknown";
            div()
                .id(SharedString::from(format!("if-{}", i.name)))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(5.))
                .cursor_pointer()
                .border_1()
                .border_color(if on { th.cyan } else { th.border(th.cyan) })
                .rounded(px(th.radius.min(6.)))
                .when(on, |d| d.bg(fade(th.cyan, 0.12)))
                .when(down, |d| d.opacity(0.5))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.net_sel = Some(name.clone());
                    cx.notify();
                }))
                .child(div().text_color(th.cyan).child(kind_icon(&i.kind)))
                .child(div().font_weight(gpui::FontWeight::BOLD).child(i.name.clone()))
                .child(div().font_family(th.mono.clone()).text_size(px(10.5)).text_color(th.cyan).child(format!("▼{}", fmt::rate(i.rx_rate))))
                .child(div().font_family(th.mono.clone()).text_size(px(10.5)).text_color(th.magenta).child(format!("▲{}", fmt::rate(i.tx_rate))))
        }))
        .child(kit::toggle("net-virtual", show_virtual, t!("mostrar virtuales"), cx, cx.listener(|this, _, _, cx| {
            this.net_virtual = !this.net_virtual;
            cx.notify();
        })));

    let mut g = kit::grid(cx).child(chips);
    if let Some(i) = sel.clone() {
        let k = |n: &str| format!("net.{}.{}", i.name, n);
        let w = st.window();
        let peak = |key: &str| st.model.history.raw(key, w).iter().copied().fold(0.0f32, f32::max) as f64;
        let (brx, btx) = root.net_base.get(&i.name).copied().unwrap_or((i.rx_total, i.tx_total));
        let errs = i.rx_errs + i.tx_errs;
        let stats = vec![
            kit::stat(t!("Bajada"), fmt::rate(i.rx_rate), fmt::bits(i.rx_rate), th.cyan, Some(hv(st, &k("rx"))), cx).into_any_element(),
            kit::stat(t!("Subida"), fmt::rate(i.tx_rate), fmt::bits(i.tx_rate), th.magenta, Some(hv(st, &k("tx"))), cx).into_any_element(),
            kit::stat(t!("Pico bajada"), fmt::rate(peak(&k("rx"))), t!("en la ventana"), th.cyan, None, cx).into_any_element(),
            kit::stat(t!("Pico subida"), fmt::rate(peak(&k("tx"))), t!("en la ventana"), th.magenta, None, cx).into_any_element(),
            kit::stat(t!("Sesión ▼"), fmt::bytes(i.rx_total.saturating_sub(brx) as f64), t!("desde arranque {}", fmt::bytes(i.rx_total as f64)), th.teal, None, cx).into_any_element(),
            kit::stat(t!("Sesión ▲"), fmt::bytes(i.tx_total.saturating_sub(btx) as f64), t!("desde arranque {}", fmt::bytes(i.tx_total as f64)), th.violet, None, cx).into_any_element(),
            kit::stat(t!("Paquetes/s"), format!("{} / {}", fmt::num(i.rx_pkt_rate), fmt::num(i.tx_pkt_rate)), t!("entrada / salida"), th.lime, Some(hv(st, &k("rxp"))), cx).into_any_element(),
            kit::stat(t!("Errores"), errs.to_string(), t!("descartes {}", i.rx_drop + i.tx_drop), if errs > 0 { th.red } else { th.muted }, None, cx).into_any_element(),
        ];
        let bw = vec![Line::new(hv(st, &k("rx")), th.cyan).fill().width(2.), Line::new(hv(st, &k("tx")), th.magenta).fill().width(2.).negate()];
        let pk = vec![Line::new(hv(st, &k("rxp")), th.lime).fill(), Line::new(hv(st, &k("txp")), th.amber).fill().negate()];
        let link_frac = i.speed.map(|sp| ((i.rx_rate.max(i.tx_rate) * 8.0) / (sp as f64 * 1e6)) as f32).unwrap_or(0.0);
        // tráfico de la sesión por interfaz
        let traffic: Vec<(String, f64)> = s
            .net
            .ifaces
            .iter()
            .filter(|x| x.kind != "loopback")
            .map(|x| {
                let (a, b) = root.net_base.get(&x.name).copied().unwrap_or((x.rx_total, x.tx_total));
                (x.name.clone(), (x.rx_total.saturating_sub(a) + x.tx_total.saturating_sub(b)) as f64)
            })
            .filter(|x| x.1 > 0.0)
            .collect();
        let ttotal: f64 = traffic.iter().map(|x| x.1).sum();
        g = g
            .child(kit::stats_row(stats, cx))
            .child(
                Panel::new(t!("Ancho de banda · {}", i.name), th.cyan)
                    .span(3)
                    .extra(kit::legend(vec![(t!("▼ bajada").into(), th.cyan), (t!("▲ subida").into(), th.magenta)]))
                    .child(Plot::new(bw, t.clone()).y_fmt(y_rate).bytes().height(260.).into_element()),
            )
            .child(Panel::new(t!("Interfaz"), th.blue).child(kit::kv_grid(
                vec![
                    (t!("Tipo").into(), i.kind.clone()),
                    (t!("Estado").into(), i.state.clone()),
                    (t!("MAC").into(), if i.mac.is_empty() { "—".into() } else { i.mac.clone() }),
                    (t!("MTU").into(), i.mtu.to_string()),
                    (t!("Enlace").into(), i.speed.map(|s| format!("{s} Mbps")).unwrap_or("—".into())),
                    (t!("IPs").into(), if i.addrs.is_empty() { "—".into() } else { i.addrs.join(", ") }),
                ],
                false,
                cx,
            )))
            .child(Panel::new(t!("Paquetes por segundo"), th.lime).span(2).child(Plot::new(pk, t.clone()).y_fmt(y_num).height(200.).into_element()))
            .child(Panel::new(t!("Uso del enlace"), th.cyan).child(charts::gauge(
                link_frac,
                th.cyan,
                if i.speed.is_some() { fmt::pct(link_frac as f64 * 100.0, 1) } else { t!("N/D").into() },
                i.speed.map(|s| t!("de {} Mbps", s)).unwrap_or(t!("velocidad desconocida").into()),
                t!("ENLACE").into(),
                210.,
            )))
            .child(
                Panel::new(t!("Tráfico"), th.magenta).extra(t!("de la sesión, por interfaz")).child(charts::ring(
                    traffic.iter().enumerate().map(|(k, x)| ((x.1 / ttotal.max(1.0)) as f32, th.series(k))).collect(),
                    fmt::bytes(ttotal),
                    "total".into(),
                    170.,
                ))
                .child(kit::legend(traffic.iter().enumerate().map(|(k, x)| (x.0.clone(), th.series(k))).collect())),
            );
    }
    let cards = div().grid().grid_cols(if kit::cols(cx) >= 4 { 3 } else { 1 }).gap(px(10.)).children(ifaces.iter().map(|i| {
        let name = i.name.clone();
        let on = sel.as_ref().is_some_and(|s| s.name == i.name);
        div()
            .id(SharedString::from(format!("card-{}", i.name)))
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(8.))
            .cursor_pointer()
            .border_1()
            .border_color(if on { th.cyan } else { th.border(th.teal) })
            .rounded(px(th.radius.min(6.)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.net_sel = Some(name.clone());
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(div().font_weight(gpui::FontWeight::BOLD).child(format!("{} {}", kind_icon(&i.kind), i.name)))
                    .child(div().text_color(if i.state == "up" { th.lime } else { th.muted }).child(i.state.clone())),
            )
            .child(charts::sparkline(hv(st, &format!("net.{}.rx", i.name)), th.cyan, 24., None))
            .child(charts::sparkline(hv(st, &format!("net.{}.tx", i.name)), th.magenta, 24., None))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .font_family(th.mono.clone())
                    .text_size(px(10.5))
                    .child(div().text_color(th.cyan).child(format!("▼{}", fmt::rate(i.rx_rate))))
                    .child(div().text_color(th.magenta).child(format!("▲{}", fmt::rate(i.tx_rate)))),
            )
    }));
    let sockets = div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(kit::stat(t!("TCP establecidos"), s.net.tcp_established.to_string(), "", th.cyan, None, cx))
        .child(kit::stat(t!("TCP escuchando"), s.net.tcp_listen.to_string(), "", th.lime, None, cx))
        .child(kit::stat(t!("UDP"), s.net.udp_sockets.to_string(), "", th.amber, None, cx));
    g.child(Panel::new(t!("Todas las interfaces"), th.teal).span(3).child(cards)).child(Panel::new(t!("Sockets"), th.violet).child(sockets)).into_any_element()
}
