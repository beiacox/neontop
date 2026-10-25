import * as echarts from "echarts/core";
import { BarChart, GaugeChart, LineChart, PieChart, RadarChart, ScatterChart, SunburstChart, TreemapChart } from "echarts/charts";
import { GraphicComponent, GridComponent, LegendComponent, PolarComponent, RadarComponent, TooltipComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import { C, DISPLAY, MONO, rgba } from "$lib/theme";

echarts.use([
  BarChart,
  GaugeChart,
  LineChart,
  PieChart,
  RadarChart,
  ScatterChart,
  SunburstChart,
  TreemapChart,
  GraphicComponent,
  GridComponent,
  LegendComponent,
  PolarComponent,
  RadarComponent,
  TooltipComponent,
  CanvasRenderer,
]);

export { echarts };
export type { EChartsCoreOption as EOption } from "echarts/core";

/**
 * Estilo común de los tooltips. Son getters para que `{ ...tooltipBase }` copie los colores
 * y la fuente del tema activo en el momento de construir cada gráfica.
 */
export const tooltipBase = {
  get backgroundColor() {
    return rgba(C.panel, 0.96);
  },
  get borderColor() {
    return rgba(C.cyan, 0.4);
  },
  get textStyle() {
    return { color: C.text, fontFamily: MONO, fontSize: 11 };
  },
  get extraCssText() {
    const r = getComputedStyle(document.documentElement).getPropertyValue("--radius-sm") || "0";
    return `box-shadow: 0 4px 16px ${rgba(C.bg, 0.5)}; border-radius: ${r};`;
  },
};

/** Quita las sombras de brillo de una opción (para temas sin neón). */
export function stripGlow<T>(o: T): T {
  if (Array.isArray(o)) return o.map(stripGlow) as T;
  if (o && typeof o === "object" && Object.getPrototypeOf(o) === Object.prototype) {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(o)) {
      if (k === "shadowBlur" || k === "textShadowBlur") continue;
      out[k] = stripGlow(v);
    }
    return out as T;
  }
  return o;
}

/** Velocímetro neón: arco de progreso con brillo y valor grande en el centro. */
export function neonGauge(opts: {
  value: number;
  max?: number;
  color: string;
  label: string;
  text: string;
  sub?: string;
}) {
  const { value, max = 100, color, label, text, sub } = opts;
  return {
    // velocímetros estrechos (dos por fila en el teléfono): letra y arco más finos para que
    // el texto quepa dentro del arco
    media: [
      {
        query: { maxWidth: 240 },
        option: {
          series: [
            {
              progress: { width: 7 },
              axisLine: { lineStyle: { width: 7 } },
              splitLine: { distance: -15, length: 5 },
              axisTick: { distance: -13, length: 2 },
              title: { fontSize: 8 },
              detail: { rich: { v: { fontSize: 16 }, s: { fontSize: 8, padding: [2, 0, 0, 0] } } },
            },
          ],
        },
      },
    ],
    baseOption: gaugeBase(value, max, color, label, text, sub),
  };
}

function gaugeBase(value: number, max: number, color: string, label: string, text: string, sub?: string) {
  return {
    series: [
      {
        type: "gauge",
        startAngle: 220,
        endAngle: -40,
        min: 0,
        max,
        radius: "92%",
        center: ["50%", "56%"],
        progress: {
          show: true,
          width: 10,
          itemStyle: { color, shadowColor: color, shadowBlur: 14 },
        },
        axisLine: { lineStyle: { width: 10, color: [[1, rgba(color, 0.1)]] } },
        axisTick: { distance: -18, length: 3, lineStyle: { color: rgba(color, 0.5), width: 1 } },
        splitLine: { distance: -20, length: 7, lineStyle: { color: rgba(color, 0.8), width: 1.5 } },
        axisLabel: { show: false },
        // marcador triangular sobre el arco (una aguja desde el centro tapaba el número)
        pointer: {
          show: true,
          icon: "path://M0,0 L10,0 L5,-12 Z",
          length: "13%",
          width: 9,
          offsetCenter: [0, "-66%"],
          itemStyle: { color, shadowColor: color, shadowBlur: 8 },
        },
        anchor: { show: false },
        title: { show: true, offsetCenter: [0, "86%"], color: C.muted, fontFamily: DISPLAY, fontSize: 10 },
        detail: {
          offsetCenter: [0, "30%"],
          formatter: () => (sub ? `{v|${text}}\n{s|${sub}}` : `{v|${text}}`),
          rich: {
            v: { color, fontFamily: DISPLAY, fontSize: 20, fontWeight: 700, textShadowColor: color, textShadowBlur: 10 },
            s: { color: C.muted, fontFamily: MONO, fontSize: 10, padding: [4, 0, 0, 0] },
          },
        },
        data: [{ value: Math.min(value, max), name: label }],
        animationDurationUpdate: 400,
      },
    ],
  };
}
