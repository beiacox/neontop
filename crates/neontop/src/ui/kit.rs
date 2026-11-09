//! Componentes básicos de la interfaz: paneles, cifras, botones, interruptores y filas de datos.
//! Toman el tema activo del global `Th`, así el estilo cambia con el tema sin tocar las vistas.

use gpui::{
    canvas, div, point, prelude::*, px, AnyElement, App, ClickEvent, ElementId, FontWeight, Hsla, IntoElement, ParentElement, PathBuilder, SharedString, Stateful, Styled,
    Window,
};
use neontop_core::theme::Corners;

use crate::charts;
use crate::ui::theme::{fade, Th};

/// Un control que se pulsa: el clic no llega a la cabecera (que arrastra la ventana al
/// pulsarla; si el arrastre empieza, el control nunca recibe el clic).
pub fn no_drag<E: InteractiveElement>(e: E) -> E {
    e.on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// Columnas del diseño actual (las calcula la ventana según su ancho).
#[derive(Clone, Copy)]
pub struct Cols(pub usize);
impl gpui::Global for Cols {}

pub fn cols(cx: &App) -> usize {
    cx.try_global::<Cols>().map(|c| c.0).unwrap_or(4)
}

/// Rejilla de paneles con el número de columnas actual.
pub fn grid(cx: &App) -> gpui::Div {
    div().w_full().grid().grid_cols(cols(cx) as u16).gap(px(12.))
}

/// Texto pequeño apagado (notas, unidades).
pub fn muted(s: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let th = Th::get(cx);
    div().text_size(px(11.)).text_color(th.muted).child(s.into())
}


/// Panel con título, color de acento y contenido.
#[derive(IntoElement)]
pub struct Panel {
    title: SharedString,
    accent: Hsla,
    span: usize,
    extra: Option<AnyElement>,
    children: Vec<AnyElement>,
    min_h: Option<f32>,
    pad: bool,
}

impl Panel {
    pub fn new(title: impl Into<SharedString>, accent: Hsla) -> Self {
        Panel { title: title.into(), accent, span: 1, extra: None, children: Vec::new(), min_h: None, pad: true }
    }
    /// Columnas que ocupa (se limita a las que haya).
    pub fn span(mut self, n: usize) -> Self {
        self.span = n;
        self
    }
    /// Contenido a la derecha del título (valor actual, leyenda…).
    pub fn extra(mut self, e: impl IntoElement) -> Self {
        self.extra = Some(e.into_any_element());
        self
    }
}

impl ParentElement for Panel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Panel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let th = Th::get(cx).clone();
        let span = self.span.clamp(1, cols(cx));
        let accent = self.accent;
        let cut = th.corners() == Corners::Cut;
        let mut p = div()
            .col_span(span as u16)
            .relative()
            .flex()
            .flex_col()
            .min_w_0()
            .bg(th.panel)
            .border_1()
            .border_color(th.border(accent))
            .rounded(px(th.radius))
            .text_color(th.text)
            .font_family(th.body.clone())
            .text_size(px(12.));
        if th.glow {
            p = p.shadow(vec![gpui::BoxShadow { color: fade(accent, 0.10), offset: point(px(0.), px(0.)), blur_radius: px(14.), spread_radius: px(0.) }]);
        }
        if let Some(h) = self.min_h {
            p = p.min_h(px(h));
        }
        let title = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_w_0()
            .child(div().size(px(6.)).bg(accent).rounded(px(if cut { 0. } else { 3. })))
            .child(
                div()
                    .font_family(th.display.clone())
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(th.dsize(if th.upper() { 11.5 } else { 13. })))
                    .text_color(accent)
                    .truncate()
                    .child(th.title(&self.title)),
            );
        // si lo de la derecha no cabe junto al título, pasa a la línea siguiente (el título no se pierde)
        let head = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .px(px(12.))
            .pt(px(10.))
            .pb(px(6.))
            .child(title)
            .children(self.extra.map(|e| div().max_w_full().overflow_hidden().whitespace_nowrap().text_size(px(10.5)).text_color(th.muted).font_family(th.mono.clone()).child(e)));
        let body = div().flex().flex_col().gap(px(8.)).flex_1().min_w_0().when(self.pad, |d| d.px(px(12.)).pb(px(12.))).children(self.children);
        p = p.child(head).child(body);
        if cut {
            // esquinas cortadas en diagonal (estilo HUD): un trazo de acento en dos esquinas
            p = p.child(
                canvas(
                    |_, _, _| (),
                    move |b, _, window, _| {
                        let k = px(12.);
                        let tr = point(b.origin.x + b.size.width, b.origin.y);
                        let bl = point(b.origin.x, b.origin.y + b.size.height);
                        for (a, z) in [(point(tr.x - k, tr.y), point(tr.x, tr.y + k)), (point(bl.x, bl.y - k), point(bl.x + k, bl.y))] {
                            let mut pb = PathBuilder::stroke(px(1.5));
                            pb.move_to(a);
                            pb.line_to(z);
                            if let Ok(path) = pb.build() {
                                window.paint_path(path, accent);
                            }
                        }
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        }
        p
    }
}

/// Cifra destacada: etiqueta, valor grande, subtítulo y mini-gráfica opcional.
pub fn stat(label: impl Into<SharedString>, value: impl Into<SharedString>, sub: impl Into<SharedString>, color: Hsla, spark: Option<Vec<f32>>, cx: &App) -> impl IntoElement {
    let th = Th::get(cx).clone();
    let has_spark = spark.as_ref().is_some_and(|s| s.len() > 1);
    div()
        .relative()
        .flex()
        .flex_col()
        .w_full()
        .min_w_0()
        .h(px(78.))
        .px(px(10.))
        .py(px(7.))
        .bg(th.panel)
        .border_l_2()
        .border_color(color)
        .rounded(px(th.radius.min(6.)))
        .child(div().text_size(px(9.5)).text_color(th.muted).font_family(th.mono.clone()).truncate().child(th.title(&label.into())))
        .child(div().text_size(px(th.dsize(21.))).font_family(th.display.clone()).font_weight(FontWeight::BOLD).text_color(color).truncate().child(value.into()))
        .child(div().text_size(px(10.)).text_color(th.muted).font_family(th.mono.clone()).truncate().when(has_spark, |d| d.mr(px(66.))).child(sub.into()))
        .children(spark.filter(|s| s.len() > 1).map(|s| div().absolute().right(px(6.)).bottom(px(6.)).w(px(60.)).h(px(24.)).child(charts::sparkline(s, color, 24., None))))
}

/// Fila de cifras (tira superior de cada pestaña): se reparte en tantas columnas como quepan.
pub fn stats_row(items: Vec<AnyElement>, cx: &App) -> impl IntoElement {
    let per_row = match cols(cx) {
        1 => 2,
        2 => 4,
        _ => items.len().clamp(1, 8),
    };
    div().col_span(cols(cx) as u16).grid().grid_cols(per_row as u16).gap(px(10.)).children(items)
}

/// Botón de texto.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, color: Hsla, cx: &App) -> Stateful<gpui::Div> {
    let th = Th::get(cx);
    no_drag(div().id(id))
        .flex()
        .items_center()
        .justify_center()
        .px(px(10.))
        .py(px(4.))
        .text_size(px(11.5))
        .font_family(th.body.clone())
        .text_color(color)
        .border_1()
        .border_color(fade(color, 0.6))
        .rounded(px(th.radius.min(6.)))
        .cursor_pointer()
        .hover(move |s| s.bg(fade(color, 0.14)))
        .active(move |s| s.bg(fade(color, 0.25)))
        .child(label.into())
}

/// Botón relleno (acción principal).
pub fn button_solid(id: impl Into<ElementId>, label: impl Into<SharedString>, color: Hsla, cx: &App) -> Stateful<gpui::Div> {
    button(id, label, color, cx).bg(fade(color, 0.2)).border_color(color)
}

/// Grupo de opciones excluyentes (muestreo, ventana…). `on` marca la elegida.
pub fn segmented<T: Clone + PartialEq + 'static>(
    id: &str,
    options: Vec<(T, String)>,
    current: &T,
    cx: &App,
    on_pick: impl Fn(&T, &mut Window, &mut App) + Clone + 'static,
) -> impl IntoElement {
    let th = Th::get(cx).clone();
    let mut row = div().flex().border_1().border_color(th.border(th.cyan)).rounded(px(th.radius.min(6.))).overflow_hidden();
    for (i, (val, label)) in options.into_iter().enumerate() {
        let on = &val == current;
        let f = on_pick.clone();
        row = row.child(
            no_drag(div().id(SharedString::from(format!("{id}-{i}"))))
                .px(px(8.))
                .py(px(3.))
                .text_size(px(11.))
                .font_family(th.mono.clone())
                .cursor_pointer()
                .when(on, |d| d.bg(th.cyan).text_color(th.bg))
                .when(!on, |d| d.text_color(th.muted).hover(|s| s.bg(fade(th.cyan, 0.12))))
                .on_click(move |_, w, cx| f(&val, w, cx))
                .child(label),
        );
    }
    row
}

/// Casilla con texto (interruptor).
pub fn toggle(id: impl Into<ElementId>, on: bool, label: impl Into<SharedString>, cx: &App, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    let th = Th::get(cx).clone();
    no_drag(div().id(id))
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .on_click(f)
        .child(
            div()
                .size(px(14.))
                .flex_shrink_0()
                .border_1()
                .border_color(if on { th.lime } else { th.muted })
                .rounded(px(3.))
                .bg(if on { fade(th.lime, 0.85) } else { gpui::transparent_black() })
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.))
                .text_color(th.bg)
                .child(if on { "✓" } else { "" }),
        )
        .child(div().text_size(px(12.)).child(label.into()))
}

/// Valor numérico con botones − / + (umbrales, puertos, precio…).
pub fn stepper(
    id: &str,
    value: String,
    cx: &App,
    on_dec: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_inc: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let th = Th::get(cx).clone();
    let b = |sid: String, s: &'static str| {
        no_drag(div().id(SharedString::from(sid)))
            .w(px(20.))
            .h(px(20.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(th.cyan)
            .hover(|s| s.bg(fade(th.cyan, 0.15)))
            .child(s)
    };
    div()
        .flex()
        .items_center()
        .border_1()
        .border_color(th.border(th.cyan))
        .rounded(px(4.))
        .child(b(format!("{}-dec", id), "−").on_click(on_dec))
        .child(div().min_w(px(52.)).px(px(4.)).text_size(px(11.5)).font_family(th.mono.clone()).flex().justify_center().child(value))
        .child(b(format!("{}-inc", id), "+").on_click(on_inc))
}

/// Fila "etiqueta … valor" de las listas de detalles.
pub fn kv(label: impl Into<SharedString>, value: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    let th = Th::get(cx).clone();
    div()
        .flex()
        .items_baseline()
        .gap(px(12.))
        .min_w_0()
        .text_size(px(11.5))
        .child(div().w(px(96.)).text_color(th.muted).flex_shrink_0().child(th.title(&label.into())).text_size(px(10.)).font_family(th.mono.clone()))
        .child(div().flex_1().min_w_0().font_family(th.mono.clone()).truncate().child(value.into()))
}

/// Lista de pares en dos columnas.
pub fn kv_grid(items: Vec<(String, String)>, two: bool, cx: &App) -> impl IntoElement {
    let two = two && cols(cx) > 1;
    div().grid().grid_cols(if two { 2 } else { 1 }).gap_x(px(18.)).gap_y(px(4.)).children(items.into_iter().map(|(k, v)| kv(k, v, cx)))
}

/// Etiqueta pequeña con borde (estado, tipo…).
pub fn badge(s: impl Into<SharedString>, color: Hsla) -> impl IntoElement {
    div().px(px(6.)).py(px(1.)).text_size(px(10.)).text_color(color).border_1().border_color(fade(color, 0.6)).rounded(px(3.)).child(s.into())
}

/// Leyenda: punto de color + texto.
pub fn legend(items: Vec<(String, Hsla)>) -> impl IntoElement {
    // una sola línea que nunca se parte (GPUI mide mal el alto de contenidos que se parten dentro de
    // filas flexibles): si no cabe junto al título, el panel la pasa debajo y se recorta en el borde
    div().flex().overflow_hidden().gap_x(px(10.)).children(items.into_iter().map(|(s, c)| {
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(4.))
            .child(div().flex_shrink_0().size(px(7.)).rounded(px(2.)).bg(c))
            .child(div().whitespace_nowrap().text_color(c).child(s))
    }))
}

/// Fila con medidor: etiqueta, barra y valor.
pub fn meter_row(label: impl Into<SharedString>, frac: f32, value: impl Into<SharedString>, color: Hsla, cx: &App) -> impl IntoElement {
    meter_row_w(label, 110., frac, value, color, cx)
}

/// Como `meter_row` con el ancho de la etiqueta a elegir (etiquetas largas).
pub fn meter_row_w(label: impl Into<SharedString>, label_w: f32, frac: f32, value: impl Into<SharedString>, color: Hsla, cx: &App) -> impl IntoElement {
    let th = Th::get(cx).clone();
    let label: SharedString = label.into();
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(11.))
        .child(div().id(SharedString::from(format!("mr-{label}"))).w(px(label_w)).flex_shrink_0().truncate().text_color(th.muted).tooltip(tooltip_text(label.clone())).child(label))
        .child(div().flex_1().child(charts::meter(frac, color, 6.)))
        .child(div().w(px(92.)).flex_shrink_0().flex().justify_end().font_family(th.mono.clone()).child(value.into()))
}

/// Texto emergente simple (para etiquetas recortadas).
pub fn tooltip_text(text: SharedString) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView {
    move |_, cx| cx.new(|_| Tip(text.clone())).into()
}

struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let th = Th::get(cx);
        div().px(px(8.)).py(px(4.)).bg(th.bg).border_1().border_color(th.cyan).rounded(px(4.)).text_size(px(11.)).text_color(th.text).child(self.0.clone())
    }
}

/// Mensaje centrado para paneles sin datos.
pub fn empty(s: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    let th = Th::get(cx);
    div().py(px(18.)).flex().justify_center().text_size(px(11.5)).text_color(th.muted).child(s.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, Context, Modifiers, TestAppContext};

    /// Imita la cabecera: el contenedor arrastra la ventana al pulsarlo y dentro hay controles.
    struct Header {
        dragged: u32,
        clicked: u32,
        toggled: u32,
        picked: u32,
    }

    impl Render for Header {
        fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, _| this.dragged += 1))
                .child(button("b", "Pulsar", gpui::red(), cx).w(px(100.)).h(px(40.)).on_click(cx.listener(|this, _, _, _| this.clicked += 1)))
                .child(div().w(px(100.)).h(px(40.)).child(toggle("t", false, "Casilla", cx, cx.listener(|this, _, _, _| this.toggled += 1))))
                .child(div().w(px(100.)).h(px(40.)).child(segmented("s", vec![(1, "uno".into())], &0, cx, {
                    let me = cx.entity().downgrade();
                    move |_, _, cx| {
                        let _ = me.update(cx, |this, _| this.picked += 1);
                    }
                })))
                .child(div().w(px(100.)).h(px(40.)))
        }
    }

    #[gpui::test]
    fn controls_receive_clicks_inside_the_drag_area(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(crate::ui::theme::Th::new(neontop_core::theme::resolve("neon", true), true, true)));
        let (view, cx) = cx.add_window_view(|_, _| Header { dragged: 0, clicked: 0, toggled: 0, picked: 0 });
        cx.run_until_parked();
        // botón, casilla y grupo de opciones: el clic llega y la cabecera no empieza a arrastrar
        cx.simulate_click(point(px(20.), px(20.)), Modifiers::none());
        cx.simulate_click(point(px(110.), px(8.)), Modifiers::none());
        cx.simulate_click(point(px(215.), px(8.)), Modifiers::none());
        // una zona vacía de la cabecera sí arrastra
        cx.simulate_click(point(px(350.), px(20.)), Modifiers::none());
        view.read_with(cx, |h, _| {
            assert_eq!((h.clicked, h.toggled, h.picked), (1, 1, 1), "cada control recibe su clic");
            assert_eq!(h.dragged, 1, "solo la zona vacía empieza a arrastrar");
        });
    }
}
