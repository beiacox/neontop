//! Gráficas dibujadas con GPUI (rutas teseladas en la GPU): series en el tiempo, velocímetros,
//! mini-gráficas, mapas de calor, histogramas y barras. Cada una es un `canvas` que recibe los
//! datos ya preparados (copiados del historial) y los pinta en el espacio que le da el diseño.

mod scale;

pub use scale::{nice_max, nice_max_binary, ticks};

use gpui::{
    canvas, div, fill, linear_color_stop, linear_gradient, point, prelude::*, px, size, App, Bounds, Hsla, PathBuilder, Pixels, Point, SharedString, TextRun,
    Window,
};

use crate::ui::theme::{fade, Th};

/// Alineación horizontal de un texto respecto a su punto de origen.
#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[allow(clippy::too_many_arguments)]
/// Pinta una línea de texto con la fuente `family` (la x de `at` según `align`, la y es la parte superior).
pub fn paint_text(window: &mut Window, cx: &mut App, s: &str, at: Point<Pixels>, size: f32, color: Hsla, family: &SharedString, align: Align) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    let run = TextRun { len: s.len(), font: gpui::font(family.clone()), color, background_color: None, underline: None, strikethrough: None };
    let line = window.text_system().shape_line(SharedString::from(s.to_string()), px(size), &[run], None);
    let w = line.width;
    let x = match align {
        Align::Left => at.x,
        Align::Center => at.x - w / 2.0,
        Align::Right => at.x - w,
    };
    let _ = line.paint(point(x, at.y), px(size * 1.25), window, cx);
    f32::from(w)
}

/// Ancho de un texto con la fuente y el tamaño dados.
pub fn text_width(window: &mut Window, s: &str, size: f32, family: &SharedString) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    let run = TextRun { len: s.len(), font: gpui::font(family.clone()), color: gpui::black(), background_color: None, underline: None, strikethrough: None };
    f32::from(window.text_system().shape_line(SharedString::from(s.to_string()), px(size), &[run], None).width)
}

/// Recta poligonal (trazo) por los puntos dados.
fn stroke(window: &mut Window, pts: &[Point<Pixels>], width: f32, color: Hsla, dashed: bool) {
    if pts.len() < 2 {
        return;
    }
    let mut b = PathBuilder::stroke(px(width));
    if dashed {
        b = b.dash_array(&[px(4.), px(3.)]);
    }
    b.move_to(pts[0]);
    for p in &pts[1..] {
        b.line_to(*p);
    }
    if let Ok(path) = b.build() {
        window.paint_path(path, color);
    }
}

/// Área bajo una poligonal hasta `base` (o hasta la poligonal `lower`), con degradado vertical.
fn area(window: &mut Window, top: &[Point<Pixels>], lower: Option<&[Point<Pixels>]>, base: Pixels, color: Hsla, alpha: (f32, f32)) {
    if top.len() < 2 {
        return;
    }
    let mut b = PathBuilder::fill();
    b.move_to(top[0]);
    for p in &top[1..] {
        b.line_to(*p);
    }
    match lower {
        Some(lo) => {
            for p in lo.iter().rev() {
                b.line_to(*p);
            }
        }
        None => {
            b.line_to(point(top[top.len() - 1].x, base));
            b.line_to(point(top[0].x, base));
        }
    }
    b.close();
    if let Ok(path) = b.build() {
        window.paint_path(path, linear_gradient(180., linear_color_stop(fade(color, alpha.0), 0.), linear_color_stop(fade(color, alpha.1), 1.)));
    }
}

/// Una serie de una gráfica.
#[derive(Clone)]
pub struct Line {
    pub ys: Vec<f32>,
    /// Abscisas propias; si está vacío, se usan las de la gráfica.
    pub xs: Vec<f64>,
    pub color: Hsla,
    pub width: f32,
    pub fill: bool,
    pub dashed: bool,
    /// Se dibuja hacia abajo (gráficas espejo: subida bajo el eje, bajada encima).
    pub negate: bool,
}

impl Line {
    pub fn new(ys: Vec<f32>, color: Hsla) -> Self {
        Line { ys, xs: Vec::new(), color, width: 1.6, fill: false, dashed: false, negate: false }
    }
    pub fn negate(mut self) -> Self {
        self.negate = true;
        self
    }
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }
    pub fn dashed(mut self) -> Self {
        self.dashed = true;
        self
    }
}

/// Cómo se rotulan las abscisas.
#[derive(Clone, Copy, PartialEq)]
pub enum XAxis {
    /// Segundos desde epoch → hora local.
    Clock,
    /// Segundos relativos (`-5.0 s`).
    Secs,
    /// Valor tal cual con una unidad corta (ms en histogramas…).
    Value(&'static str),
    None,
}

/// Gráfica general: series en el tiempo (líneas o áreas, apiladas o no), barras, líneas de
/// referencia horizontales y marcas puntuales.
#[derive(Clone)]
pub struct Plot {
    pub lines: Vec<Line>,
    pub xs: Vec<f64>,
    pub x_range: Option<(f64, f64)>,
    pub y_min: f32,
    pub y_max: Option<f32>,
    pub stacked: bool,
    /// La primera serie se dibuja como barras (histogramas).
    pub bars: bool,
    pub height: f32,
    pub y_fmt: fn(f32) -> String,
    pub x_axis: XAxis,
    /// Líneas de referencia: (valor, color, rótulo).
    pub hlines: Vec<(f32, Hsla, String)>,
    /// Marcas verticales: (x, color).
    pub vlines: Vec<(f64, Hsla)>,
    /// Puntos destacados: (x, y, color).
    pub dots: Vec<(f64, f32, Hsla)>,
    /// Sin ejes ni rejilla (mini-gráficas).
    pub bare: bool,
    /// Eje en bytes: máximo redondeado en unidades binarias.
    pub binary: bool,
}

fn fmt_plain(v: f32) -> String {
    if v.abs() >= 100.0 || v == 0.0 {
        format!("{v:.0}")
    } else if v.abs() >= 10.0 {
        format!("{v:.1}").trim_end_matches(".0").to_string()
    } else {
        format!("{v:.2}").trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

impl Plot {
    pub fn new(lines: Vec<Line>, xs: Vec<f64>) -> Self {
        Plot {
            lines,
            xs,
            x_range: None,
            y_min: 0.0,
            y_max: None,
            stacked: false,
            bars: false,
            height: 160.0,
            y_fmt: fmt_plain,
            x_axis: XAxis::Clock,
            hlines: Vec::new(),
            vlines: Vec::new(),
            dots: Vec::new(),
            bare: false,
            binary: false,
        }
    }
    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }
    pub fn y_max(mut self, m: f32) -> Self {
        self.y_max = Some(m);
        self
    }
    pub fn y_fmt(mut self, f: fn(f32) -> String) -> Self {
        self.y_fmt = f;
        self
    }
    pub fn stacked(mut self) -> Self {
        self.stacked = true;
        self
    }
    pub fn x_axis(mut self, a: XAxis) -> Self {
        self.x_axis = a;
        self
    }
    pub fn x_range(mut self, a: f64, b: f64) -> Self {
        self.x_range = Some((a, b));
        self
    }
    pub fn hline(mut self, v: f32, color: Hsla, label: impl Into<String>) -> Self {
        self.hlines.push((v, color, label.into()));
        self
    }
    pub fn dots(mut self, d: Vec<(f64, f32, Hsla)>) -> Self {
        self.dots = d;
        self
    }
    /// Eje de bytes (máximos en potencias de dos y rótulos en KiB/MiB/GiB).
    pub fn bytes(mut self) -> Self {
        self.binary = true;
        self
    }
    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }

    /// Máximo del eje Y (redondeado) y series acumuladas si es apilada.
    fn prepare(&self) -> (f32, Vec<Vec<f32>>) {
        let mut ys: Vec<Vec<f32>> = self.lines.iter().map(|l| if l.negate { l.ys.iter().map(|v| -v).collect() } else { l.ys.clone() }).collect();
        if self.stacked {
            for i in 1..ys.len() {
                let (prev, cur) = ys.split_at_mut(i);
                for (j, v) in cur[0].iter_mut().enumerate() {
                    *v += prev[i - 1].get(j).copied().unwrap_or(0.0);
                }
            }
        }
        let data_max = ys.iter().flatten().copied().filter(|v| v.is_finite()).fold(0.0f32, |a, v| a.max(v.abs()));
        let ref_max = self.hlines.iter().map(|h| h.0).fold(0.0f32, f32::max);
        let top = data_max.max(ref_max * 1.05);
        let max = self.y_max.unwrap_or_else(|| if self.binary { nice_max_binary(top) } else { nice_max(top) });
        (max.max(self.y_min + 1e-6), ys)
    }

    pub fn into_element(self) -> impl IntoElement {
        let h = self.height;
        canvas(|_, _, _| (), move |bounds, _, window, cx| self.paint(bounds, window, cx)).w_full().h(px(h))
    }

    fn paint(self, b: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let th = Th::get(cx).clone();
        let (max, ys) = self.prepare();
        // el margen izquierdo se ajusta al rótulo más ancho del eje Y ("979.8 MiB")
        let tick_vals = ticks(if self.lines.iter().any(|l| l.negate) { -max } else { self.y_min }, max, 4);
        let label_w = if self.bare { 0.0 } else { tick_vals.iter().map(|v| text_width(window, &(self.y_fmt)(v.abs()), 9.5, &th.mono)).fold(0.0, f32::max) };
        let (left, bottom, top, right) = if self.bare { (0.0, 0.0, 1.0, 0.0) } else { (label_w.max(20.0) + 12.0, 16.0, 6.0, 6.0) };
        let x0 = b.origin.x + px(left);
        let x1 = b.origin.x + b.size.width - px(right);
        let y0 = b.origin.y + px(top);
        let y1 = b.origin.y + b.size.height - px(bottom);
        let (w, hgt) = (f32::from(x1 - x0), f32::from(y1 - y0));
        if w <= 2.0 || hgt <= 2.0 {
            return;
        }
        // gráfica espejo: eje simétrico, el cero en medio
        let mirrored = self.lines.iter().any(|l| l.negate);
        let min = if mirrored { -max } else { self.y_min };
        let ymap = |v: f32| y1 - px(((v - min) / (max - min)).clamp(0.0, 1.0) * hgt);

        // abscisas: las de la gráfica o, si no hay, el índice
        let longest = self.lines.iter().map(|l| l.ys.len()).max().unwrap_or(0);
        let (xa, xb) = self.x_range.unwrap_or_else(|| {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for l in &self.lines {
                let xs = if l.xs.is_empty() { &self.xs } else { &l.xs };
                if let (Some(a), Some(z)) = (xs.first(), xs.last()) {
                    lo = lo.min(*a);
                    hi = hi.max(*z);
                }
            }
            if lo.is_finite() && hi > lo { (lo, hi) } else { (0.0, (longest.max(2) - 1) as f64) }
        });
        let span = (xb - xa).max(1e-9);
        let xmap = |x: f64| x0 + px(((x - xa) / span).clamp(0.0, 1.0) as f32 * w);

        if !self.bare {
            // rejilla y rótulos del eje Y
            let mono = th.mono.clone();
            for v in ticks(min, max, 4) {
                let y = ymap(v);
                window.paint_quad(fill(Bounds::new(point(x0, y), size(x1 - x0, px(1.))), th.grid));
                paint_text(window, cx, &(self.y_fmt)(v.abs()), point(x0 - px(6.), y - px(6.)), 9.5, th.muted, &mono, Align::Right);
            }
            // rótulos del eje X
            let n_labels = ((w / 110.0) as usize).clamp(2, 8);
            for i in 0..=n_labels {
                let x = xa + span * i as f64 / n_labels as f64;
                let label = match self.x_axis {
                    XAxis::Clock => neontop_core::format::clock((x * 1000.0) as u64),
                    XAxis::Secs => format!("{x:.1} s"),
                    XAxis::Value(u) => format!("{}{u}", fmt_plain(x as f32)),
                    XAxis::None => String::new(),
                };
                let align = if i == 0 { Align::Left } else if i == n_labels { Align::Right } else { Align::Center };
                paint_text(window, cx, &label, point(xmap(x), y1 + px(3.)), 9.5, th.muted, &mono, align);
            }
        }

        let pts_of = |i: usize, vals: &[f32]| -> Vec<Point<Pixels>> {
            let l = &self.lines[i];
            let xs = if l.xs.is_empty() { &self.xs } else { &l.xs };
            let n = vals.len();
            vals.iter()
                .enumerate()
                .map(|(j, &v)| {
                    let x = xs.get(j).copied().unwrap_or_else(|| if xs.is_empty() { j as f64 } else { xa + span * j as f64 / (n.max(2) - 1) as f64 });
                    point(xmap(x), ymap(if v.is_finite() { v } else { 0.0 }))
                })
                .collect()
        };

        if self.bars {
            if let Some(l) = self.lines.first() {
                let n = l.ys.len().max(1);
                let bw = w / n as f32;
                for (j, &v) in l.ys.iter().enumerate() {
                    if v <= 0.0 {
                        continue;
                    }
                    let y = ymap(v);
                    let x = x0 + px(bw * j as f32);
                    window.paint_quad(fill(Bounds::new(point(x, y), size(px((bw - 1.0).max(1.0)), y1 - y)), fade(l.color, 0.85)));
                }
            }
        } else {
            let all: Vec<Vec<Point<Pixels>>> = ys.iter().enumerate().map(|(i, v)| pts_of(i, v)).collect();
            for (i, l) in self.lines.iter().enumerate() {
                if l.fill {
                    let lower = if self.stacked && i > 0 { Some(all[i - 1].as_slice()) } else { None };
                    let alpha = if self.stacked { (0.55, 0.35) } else { (0.32, 0.02) };
                    let base = if mirrored { ymap(0.0) } else { y1 };
                    area(window, &all[i], lower, base, l.color, alpha);
                }
            }
            for (i, l) in self.lines.iter().enumerate() {
                if th.glow && l.width >= 1.4 {
                    stroke(window, &all[i], l.width + 3.0, fade(l.color, 0.12), false);
                }
                stroke(window, &all[i], l.width, l.color, l.dashed);
            }
        }

        for (v, color, label) in &self.hlines {
            let y = ymap(*v);
            stroke(window, &[point(x0, y), point(x1, y)], 1.0, fade(*color, 0.7), true);
            paint_text(window, cx, label, point(x1 - px(2.), y - px(13.)), 9.5, *color, &th.mono, Align::Right);
        }
        for (x, color) in &self.vlines {
            let x = xmap(*x);
            window.paint_quad(fill(Bounds::new(point(x, y0), size(px(1.), y1 - y0)), fade(*color, 0.6)));
        }
        for (x, y, color) in &self.dots {
            let c = point(xmap(*x), ymap(*y));
            window.paint_quad(fill(Bounds::new(point(c.x - px(2.5), c.y - px(2.5)), size(px(5.), px(5.))), *color).corner_radii(px(2.5)));
        }
    }
}

/// Mini-gráfica (sin ejes) con área suave.
pub fn sparkline(ys: Vec<f32>, color: Hsla, height: f32, max: Option<f32>) -> impl IntoElement {
    let mut p = Plot::new(vec![Line::new(ys, color).fill().width(1.3)], Vec::new()).bare().height(height);
    p.y_max = max;
    p.into_element()
}

/// Puntos de un arco (ángulos en grados, 0 = derecha, sentido horario en pantalla).
fn arc_points(c: Point<Pixels>, r: f32, from: f32, to: f32) -> Vec<Point<Pixels>> {
    let steps = ((to - from).abs() / 4.0).ceil().max(2.0) as usize;
    (0..=steps)
        .map(|i| {
            let a = (from + (to - from) * i as f32 / steps as f32).to_radians();
            point(c.x + px(r * a.cos()), c.y + px(r * a.sin()))
        })
        .collect()
}

/// Velocímetro de 270°: valor 0–100 (`frac` = fracción llena), cifra y textos en el centro.
pub fn gauge(frac: f32, color: Hsla, value: String, sub: String, label: String, height: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, cx| {
            let th = Th::get(cx).clone();
            let r = (f32::from(b.size.width.min(b.size.height)) / 2.0 - 10.0).max(10.0);
            let c = point(b.origin.x + b.size.width / 2.0, b.origin.y + b.size.height / 2.0 + px(r * 0.08));
            let (start, end) = (135.0f32, 405.0f32);
            let width = (r * 0.11).max(4.0);
            // marcas
            for i in 0..=27 {
                let a = start + (end - start) * i as f32 / 27.0;
                let big = i % 9 == 0;
                let (r0, r1) = (r + 4.0, r + if big { 9.0 } else { 6.0 });
                let ar = a.to_radians();
                stroke(window, &[point(c.x + px(r0 * ar.cos()), c.y + px(r0 * ar.sin())), point(c.x + px(r1 * ar.cos()), c.y + px(r1 * ar.sin()))], 1.0, fade(color, if big { 0.7 } else { 0.35 }), false);
            }
            stroke(window, &arc_points(c, r - width, start, end), width, fade(color, 0.13), false);
            let f = frac.clamp(0.0, 1.0);
            if f > 0.002 {
                let pts = arc_points(c, r - width, start, start + (end - start) * f);
                if th.glow {
                    stroke(window, &pts, width * 2.2, fade(color, 0.12), false);
                }
                stroke(window, &pts, width, color, false);
            }
            let big = (r * 0.36).clamp(14.0, 40.0);
            paint_text(window, cx, &value, point(c.x, c.y - px(th.dsize(big) * 0.75)), th.dsize(big), color, &th.display, Align::Center);
            paint_text(window, cx, &sub, point(c.x, c.y + px(big * 0.55)), 10.5, th.muted, &th.mono, Align::Center);
            paint_text(window, cx, &label, point(c.x, c.y + px(r * 0.72)), 10.5, th.muted, &th.display, Align::Center);
        },
    )
    .w_full()
    .h(px(height))
}

/// Anillo de proporciones (memoria, discos…): segmentos (fracción, color) y texto central.
pub fn ring(parts: Vec<(f32, Hsla)>, center: String, sub: String, height: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, cx| {
            let th = Th::get(cx).clone();
            let r = (f32::from(b.size.width.min(b.size.height)) / 2.0 - 6.0).max(8.0);
            let c = point(b.origin.x + b.size.width / 2.0, b.origin.y + b.size.height / 2.0);
            let width = (r * 0.18).max(5.0);
            stroke(window, &arc_points(c, r - width, 0.0, 360.0), width, fade(th.dim, 0.6), false);
            let mut a = -90.0f32;
            for (f, color) in parts {
                let sweep = 360.0 * f.clamp(0.0, 1.0);
                if sweep > 0.5 {
                    stroke(window, &arc_points(c, r - width, a + 0.6, a + sweep - 0.6), width, color, false);
                }
                a += sweep;
            }
            let big = (r * 0.32).clamp(12.0, 30.0);
            paint_text(window, cx, &center, point(c.x, c.y - px(th.dsize(big) * 0.8)), th.dsize(big), th.text, &th.display, Align::Center);
            paint_text(window, cx, &sub, point(c.x, c.y + px(big * 0.45)), 10.0, th.muted, &th.mono, Align::Center);
        },
    )
    .w_full()
    .h(px(height))
}

/// Mapa de calor: filas (núcleos…) × columnas (tiempo), valores 0–100.
pub fn heatmap(rows: Vec<Vec<f32>>, labels: Vec<String>, height: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, cx| {
            let th = Th::get(cx).clone();
            let n = rows.len().max(1);
            let left = 34.0;
            let rh = f32::from(b.size.height) / n as f32;
            let w = f32::from(b.size.width) - left;
            for (i, row) in rows.iter().enumerate() {
                let y = b.origin.y + px(rh * i as f32);
                if let Some(l) = labels.get(i) {
                    paint_text(window, cx, l, point(b.origin.x, y + px((rh - 11.0) / 2.0)), 9.5, th.muted, &th.mono, Align::Left);
                }
                let cols = row.len().max(1);
                let cw = w / cols as f32;
                for (j, &v) in row.iter().enumerate() {
                    let x = b.origin.x + px(left + cw * j as f32);
                    window.paint_quad(fill(Bounds::new(point(x, y + px(1.)), size(px(cw + 0.5), px((rh - 2.0).max(1.0)))), th.heat(v)));
                }
            }
        },
    )
    .w_full()
    .h(px(height))
}

/// Medidor por bloques al estilo btop: `n` celdas, encendidas hasta `frac`.
pub fn blocks(frac: f32, color: Hsla, n: usize, height: f32) -> impl IntoElement {
    let on = (frac.clamp(0.0, 1.0) * n as f32).round() as usize;
    div().flex_1().flex().gap(px(2.)).h(px(height)).children((0..n).map(move |i| div().flex_1().h_full().bg(if i < on { color } else { fade(color, 0.12) })))
}

/// Barra horizontal de porcentaje (medidores).
pub fn meter(frac: f32, color: Hsla, height: f32) -> impl IntoElement {
    let f = frac.clamp(0.0, 1.0);
    div()
        .w_full()
        .h(px(height))
        .rounded(px(height / 2.0))
        .bg(fade(color, 0.14))
        .child(div().h_full().w(gpui::relative(f)).rounded(px(height / 2.0)).bg(color))
}

/// Barra horizontal dividida en segmentos (fracción, color), p. ej. reparto de la RAM.
pub fn segments(parts: Vec<(f32, Hsla)>, height: f32) -> impl IntoElement {
    let mut row = div().w_full().h(px(height)).flex().overflow_hidden().rounded(px(2.));
    for (f, c) in parts {
        if f > 0.0 {
            row = row.child(div().h_full().w(gpui::relative(f.clamp(0.0, 1.0))).bg(c));
        }
    }
    row
}

/// Radar (perfil): ejes con su rótulo y valor 0–100.
pub fn radar(axes: Vec<(String, f32)>, color: Hsla, height: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, cx| {
            let th = Th::get(cx).clone();
            let n = axes.len().max(3);
            let r = (f32::from(b.size.width.min(b.size.height)) / 2.0 - 26.0).max(10.0);
            let c = point(b.origin.x + b.size.width / 2.0, b.origin.y + b.size.height / 2.0 + px(4.));
            let at = |i: usize, f: f32| {
                let a = (-90.0 + 360.0 * i as f32 / n as f32).to_radians();
                point(c.x + px(r * f * a.cos()), c.y + px(r * f * a.sin()))
            };
            for k in 1..=4 {
                let f = k as f32 / 4.0;
                let mut ring: Vec<Point<Pixels>> = (0..n).map(|i| at(i, f)).collect();
                ring.push(ring[0]);
                stroke(window, &ring, 1.0, fade(th.cyan, 0.15), false);
            }
            for (i, (label, _)) in axes.iter().enumerate() {
                stroke(window, &[c, at(i, 1.0)], 1.0, fade(th.cyan, 0.12), false);
                // el rótulo se mantiene dentro del lienzo aunque sea largo
                let p = at(i, 1.16);
                let half = text_width(window, label, 10.0, &th.mono) / 2.0;
                let x = f32::from(p.x).clamp(f32::from(b.origin.x) + half, f32::from(b.origin.x + b.size.width) - half);
                paint_text(window, cx, label, point(px(x), p.y - px(6.)), 10.0, th.muted, &th.mono, Align::Center);
            }
            let mut poly: Vec<Point<Pixels>> = axes.iter().enumerate().map(|(i, (_, v))| at(i, (v / 100.0).clamp(0.0, 1.0))).collect();
            if poly.len() >= 3 {
                let mut pb = PathBuilder::fill();
                pb.move_to(poly[0]);
                for p in &poly[1..] {
                    pb.line_to(*p);
                }
                pb.close();
                if let Ok(path) = pb.build() {
                    window.paint_path(path, fade(color, 0.25));
                }
                poly.push(poly[0]);
                stroke(window, &poly, 2.0, color, false);
            }
        },
    )
    .w_full()
    .h(px(height))
}

/// Barras radiales: un sector por elemento (hilos de la CPU…) con longitud según su valor 0–100.
pub fn polar(values: Vec<f32>, center: String, height: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, cx| {
            let th = Th::get(cx).clone();
            let n = values.len().max(1);
            let r = (f32::from(b.size.width.min(b.size.height)) / 2.0 - 16.0).max(10.0);
            let r0 = r * 0.32;
            let c = point(b.origin.x + b.size.width / 2.0, b.origin.y + b.size.height / 2.0);
            let step = 360.0 / n as f32;
            for (i, v) in values.iter().enumerate() {
                let a0 = -90.0 + step * i as f32 + 1.5;
                let a1 = a0 + step - 3.0;
                let rr = r0 + (r - r0) * (v / 100.0).clamp(0.0, 1.0);
                let mut ring = arc_points(c, r, a0, a1);
                ring.extend(arc_points(c, r0, a1, a0));
                let mut pb = PathBuilder::fill();
                pb.add_polygon(&ring, true);
                if let Ok(path) = pb.build() {
                    window.paint_path(path, fade(th.cyan, 0.07));
                }
                let mut seg = arc_points(c, rr, a0, a1);
                seg.extend(arc_points(c, r0, a1, a0));
                let mut pb = PathBuilder::fill();
                pb.add_polygon(&seg, true);
                if let Ok(path) = pb.build() {
                    window.paint_path(path, th.load_color(*v));
                }
                let lp = {
                    let a = ((a0 + a1) / 2.0).to_radians();
                    point(c.x + px((r + 8.0) * a.cos()), c.y + px((r + 8.0) * a.sin()) - px(5.))
                };
                paint_text(window, cx, &format!("C{i}"), lp, 8.5, th.muted, &th.mono, Align::Center);
            }
            paint_text(window, cx, &center, point(c.x, c.y - px(10.)), 16.0, th.text, &th.display, Align::Center);
        },
    )
    .w_full()
    .h(px(height))
}

/// Barras verticales con rótulo y, opcionalmente, una marca de máximo por barra.
pub fn columns(values: Vec<(String, f32, Hsla)>, max: f32, marks: Vec<f32>, fmt: fn(f32) -> String, height: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, cx| {
            let th = Th::get(cx).clone();
            let max = max.max(1e-6);
            let left = ticks(0.0, max, 4).iter().map(|v| text_width(window, &fmt(*v), 9.5, &th.mono)).fold(0.0, f32::max).max(20.0) + 12.0;
            let bottom = 16.0;
            let x0 = b.origin.x + px(left);
            let y1 = b.origin.y + b.size.height - px(bottom);
            let y0 = b.origin.y + px(6.);
            let w = f32::from(b.size.width) - left;
            let hgt = f32::from(y1 - y0);
            for v in ticks(0.0, max, 4) {
                let y = y1 - px(v / max * hgt);
                window.paint_quad(fill(Bounds::new(point(x0, y), size(px(w), px(1.))), th.grid));
                paint_text(window, cx, &fmt(v), point(x0 - px(6.), y - px(6.)), 9.5, th.muted, &th.mono, Align::Right);
            }
            let n = values.len().max(1);
            let bw = w / n as f32;
            for (i, (label, v, color)) in values.iter().enumerate() {
                let x = x0 + px(bw * i as f32 + bw * 0.18);
                let h = (v / max).clamp(0.0, 1.0) * hgt;
                let bar = Bounds::new(point(x, y1 - px(h)), size(px(bw * 0.64), px(h)));
                window.paint_quad(fill(Bounds::new(point(x, y0), size(px(bw * 0.64), px(hgt))), fade(*color, 0.07)));
                window.paint_quad(fill(bar, gpui::linear_gradient(180., gpui::linear_color_stop(*color, 0.), gpui::linear_color_stop(fade(*color, 0.45), 1.))));
                if let Some(m) = marks.get(i) {
                    let y = y1 - px((m / max).clamp(0.0, 1.0) * hgt);
                    window.paint_quad(fill(Bounds::new(point(x, y), size(px(bw * 0.64), px(2.))), th.amber));
                }
                paint_text(window, cx, label, point(x + px(bw * 0.32), y1 + px(3.)), 9.0, th.muted, &th.mono, Align::Center);
            }
        },
    )
    .w_full()
    .h(px(height))
}

/// Mapa de celdas (10 × 20): cada celda es 1/200 del total, coloreada por segmento.
pub fn waffle(parts: Vec<(f32, Hsla)>, height: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, cx| {
            let th = Th::get(cx).clone();
            let (cols, rows) = (20usize, 10usize);
            let gap = 3.0;
            let cw = (f32::from(b.size.width) - gap * (cols as f32 - 1.0)) / cols as f32;
            let ch = (f32::from(b.size.height) - gap * (rows as f32 - 1.0)) / rows as f32;
            let mut colors = Vec::with_capacity(cols * rows);
            for (f, c) in &parts {
                let n = (f.clamp(0.0, 1.0) * (cols * rows) as f32).round() as usize;
                colors.extend(std::iter::repeat_n(*c, n));
            }
            for i in 0..cols * rows {
                let (r, c) = (i / cols, i % cols);
                let color = colors.get(i).copied().unwrap_or(fade(th.dim, 0.7));
                let at = point(b.origin.x + px(c as f32 * (cw + gap)), b.origin.y + px(r as f32 * (ch + gap)));
                window.paint_quad(fill(Bounds::new(at, size(px(cw), px(ch))), color).corner_radii(px(2.)));
            }
        },
    )
    .w_full()
    .h(px(height))
}

/// Histograma con eje Y logarítmico: `bins` (cuenta, color) de ancho `step` (rótulos en el eje X).
pub fn histogram(bins: Vec<(f32, Hsla)>, step: f32, unit: &'static str, height: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, cx| {
            let th = Th::get(cx).clone();
            let max = bins.iter().map(|x| x.0).fold(1.0f32, f32::max);
            let left = text_width(window, &crate::tabs::y_num(10f32.powi((max + 1.0).log10().ceil() as i32)), 9.5, &th.mono).max(20.0) + 12.0;
            let bottom = 16.0;
            let x0 = b.origin.x + px(left);
            let y1 = b.origin.y + b.size.height - px(bottom);
            let y0 = b.origin.y + px(6.);
            let w = f32::from(b.size.width) - left;
            let hgt = f32::from(y1 - y0);
            let lmax = (max + 1.0).log10().max(0.1);
            for k in 0..=(lmax.ceil() as i32) {
                let v = 10f32.powi(k);
                let y = y1 - px(((v + 1.0).log10() / lmax).min(1.0) * hgt);
                window.paint_quad(fill(Bounds::new(point(x0, y), size(px(w), px(1.))), th.grid));
                paint_text(window, cx, &crate::tabs::y_num(v), point(x0 - px(6.), y - px(6.)), 9.5, th.muted, &th.mono, Align::Right);
            }
            let n = bins.len().max(1);
            let bw = w / n as f32;
            for (i, (v, c)) in bins.iter().enumerate() {
                if *v <= 0.0 {
                    continue;
                }
                let h = ((v + 1.0).log10() / lmax).min(1.0) * hgt;
                window.paint_quad(fill(Bounds::new(point(x0 + px(bw * i as f32), y1 - px(h)), size(px((bw * 0.9).max(1.0)), px(h))), *c));
            }
            let every = (n / 8).max(1);
            for i in (0..n).step_by(every) {
                let label = format!("{}{unit}", fmt_plain(step * i as f32));
                paint_text(window, cx, &label, point(x0 + px(bw * i as f32), y1 + px(3.)), 9.0, th.muted, &th.mono, Align::Left);
            }
        },
    )
    .w_full()
    .h(px(height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacked_prepare() {
        let red = gpui::red();
        let p = Plot::new(vec![Line::new(vec![1.0, 2.0], red), Line::new(vec![3.0, 4.0], red)], vec![0.0, 1.0]).stacked();
        let (max, ys) = p.prepare();
        assert_eq!(ys[1], vec![4.0, 6.0]);
        assert!(max >= 6.0);
        let p = Plot::new(vec![Line::new(vec![5.0], red)], vec![]).y_max(100.0);
        assert_eq!(p.prepare().0, 100.0);
    }

    #[test]
    fn plain_format() {
        assert_eq!(fmt_plain(0.0), "0");
        assert_eq!(fmt_plain(250.0), "250");
        assert_eq!(fmt_plain(12.5), "12.5");
        assert_eq!(fmt_plain(20.0), "20");
        assert_eq!(fmt_plain(0.25), "0.25");
        assert_eq!(fmt_plain(1.5), "1.5");
    }
}
