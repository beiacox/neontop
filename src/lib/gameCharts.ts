/**
 * Gráficas de la pestaña Juegos que comparten la vista en vivo y las partidas guardadas.
 */
import { tooltipBase } from "./charts/echarts";
import { C, MONO, rgba } from "./theme";

/**
 * Distribución de tiempos de fotograma. `counts` son cubos de `step` ms (el último recoge el
 * resto); se reagrupan para que el eje llegue hasta 1,5× el percentil 99,9.
 */
export function histOption(counts: number[], step: number, p999: number, median: number) {
  const total = counts.reduce((a, b) => a + b, 0) || 1;
  const upTo = Math.min(counts.length * step, Math.max(20, Math.ceil((p999 * 1.5) / 5) * 5));
  const bin = upTo <= 40 ? 0.5 : 1;
  const per = Math.max(1, Math.round(bin / step));
  const n = Math.ceil(upTo / bin);
  const bins = new Array(n).fill(0);
  counts.forEach((c, i) => (bins[Math.min(n - 1, Math.floor(i / per))] += c));
  return {
    tooltip: {
      ...tooltipBase,
      trigger: "axis",
      axisPointer: { type: "shadow" },
      formatter: (p: any) => `${p[0].name} ms (${(1000 / Math.max(0.1, Number(p[0].name))).toFixed(0)} FPS)<br><b>${(((p[0].value ?? 0) / total) * 100).toFixed(2)} %</b> de los fotogramas`,
    },
    grid: { left: 44, right: 8, top: 10, bottom: 24 },
    xAxis: {
      type: "category",
      data: bins.map((_, i) => (i * bin).toFixed(bin < 1 ? 1 : 0)),
      axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9, interval: Math.ceil(bins.length / 10) },
      axisTick: { show: false },
      axisLine: { lineStyle: { color: C.grid } },
    },
    yAxis: { type: "log", min: 1, axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9 }, splitLine: { lineStyle: { color: C.grid } } },
    series: [
      {
        type: "bar",
        barWidth: "90%",
        data: bins.map((v, i) => ({ value: v || null, itemStyle: { color: i * bin > median * 2 && median > 0 ? C.red : C.lime } })),
      },
    ],
  };
}

/** Percentiles del tiempo de fotograma (ms) y FPS medios de una partida. */
export interface Percentiles {
  avg: number;
  p50: number;
  p90: number;
  p95: number;
  p99: number;
  p999: number;
  max: number;
}

/** FPS por percentil: media, mediana, 10/5/1/0,1 % bajos y mínimo. */
export function pctOption(p: Percentiles) {
  const fps = (ms: number) => (ms > 0 ? 1000 / ms : 0);
  const rows = [
    { name: "Media", v: p.avg, c: C.cyan },
    { name: "Mediana", v: fps(p.p50), c: C.teal },
    { name: "10 % bajo", v: fps(p.p90), c: C.lime },
    { name: "5 % bajo", v: fps(p.p95), c: C.lime },
    { name: "1 % bajo", v: fps(p.p99), c: C.amber },
    { name: "0,1 % bajo", v: fps(p.p999), c: C.orange },
    { name: "Mínimo", v: fps(p.max), c: C.red },
  ];
  return {
    tooltip: { ...tooltipBase, trigger: "axis", axisPointer: { type: "shadow" }, valueFormatter: (v: number) => `${v.toFixed(1)} FPS` },
    grid: { left: 80, right: 50, top: 4, bottom: 4 },
    xAxis: { type: "value", show: false, max: Math.max(1, ...rows.map((r) => r.v)) * 1.05 },
    yAxis: { type: "category", inverse: true, data: rows.map((r) => r.name), axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 10 }, axisLine: { show: false }, axisTick: { show: false } },
    series: [
      {
        type: "bar",
        barWidth: 12,
        showBackground: true,
        backgroundStyle: { color: rgba(C.cyan, 0.04) },
        label: { show: true, position: "right", color: C.text, fontFamily: MONO, fontSize: 10, formatter: (q: any) => q.value.toFixed(1) },
        data: rows.map((r) => ({ value: r.v, itemStyle: { color: r.c, shadowColor: r.c, shadowBlur: 6 } })),
      },
    ],
  };
}
