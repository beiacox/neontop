<script lang="ts" module>
  export interface SeriesDef {
    label: string;
    color: string;
    width?: number;
    /** Relleno con degradado bajo la línea. */
    fill?: boolean;
    dash?: number[];
    /** Dibuja la serie hacia abajo (gráficas espejadas, p. ej. subida de red). */
    negate?: boolean;
  }
</script>

<script lang="ts">
  import uPlot from "uplot";
  import { untrack } from "svelte";
  import "uplot/dist/uPlot.min.css";
  import { metrics } from "$lib/store.svelte";
  import { settings } from "$lib/settings.svelte";
  import { C, MONO, rgba } from "$lib/theme";
  import { watchVisible } from "./visible";

  interface Props {
    series: SeriesDef[];
    /** Devuelve las series ya recortadas a la ventana (sin el eje x). */
    data: () => number[][];
    height?: number;
    yMin?: number;
    yMax?: number;
    yFmt?: (v: number) => string;
    /** Apila las series (áreas acumuladas). */
    stacked?: boolean;
    /** Eje y simétrico alrededor de 0. */
    mirrored?: boolean;
    /** Tope mínimo de la escala y (para que una serie a 0 no dibuje un eje 0–1). */
    yFloor?: number;
  }

  let { series, data, height = 180, yMin, yMax, yFmt = (v) => v.toFixed(0), stacked = false, mirrored = false, yFloor = 1 }: Props = $props();

  let el = $state<HTMLDivElement>()!;
  let plot: uPlot | null = null;
  let visible = $state(true);

  /**
   * Brillo neón dibujado en el propio canvas: un trazo ancho y translúcido bajo la línea.
   * Sustituye al `filter: drop-shadow` de CSS, que WebKitGTK recalculaba en cada repintado.
   */
  function glowHook(u: uPlot, si: number) {
    if (!settings.glowOn) return;
    const s = u.series[si] as uPlot.Series & { _paths?: { stroke?: Path2D } };
    const path = s._paths?.stroke;
    const color = series[si - 1]?.color;
    // los colores ya translúcidos (p. ej. 12 series de frecuencia) no llevan halo
    if (!path || !color?.startsWith("#")) return;
    const { ctx } = u;
    const w = (series[si - 1].width ?? 1.6) * devicePixelRatio;
    ctx.save();
    ctx.beginPath();
    ctx.rect(u.bbox.left, u.bbox.top, u.bbox.width, u.bbox.height);
    ctx.clip();
    ctx.lineJoin = "round";
    ctx.strokeStyle = rgba(color, 0.12);
    ctx.lineWidth = w * 5;
    ctx.stroke(path);
    ctx.strokeStyle = rgba(color, 0.25);
    ctx.lineWidth = w * 2.5;
    ctx.stroke(path);
    // la línea nítida otra vez por encima del halo
    ctx.strokeStyle = color;
    ctx.lineWidth = w;
    ctx.stroke(path);
    ctx.restore();
  }
  let tip = $state<{ x: number; y: number; time: string; rows: { label: string; color: string; value: string }[] } | null>(null);

  /** Valor real (sin acumular ni negar) de la serie `i` en el índice `idx`. */
  function rawValue(i: number, idx: number): number {
    const d = plot!.data;
    const v = d[i + 1][idx] ?? 0;
    const prev = stacked && i > 0 ? (d[i][idx] ?? 0) : 0;
    return Math.abs(v - prev);
  }

  function onCursor(u: uPlot) {
    const idx = u.cursor.idx;
    if (idx == null || u.cursor.left == null || u.cursor.left < 0) {
      tip = null;
      return;
    }
    tip = {
      x: u.cursor.left + u.bbox.left / devicePixelRatio,
      y: u.cursor.top ?? 0,
      time: new Date((u.data[0][idx] as number) * 1000).toLocaleTimeString("es", { hour12: false }),
      rows: series.map((s, i) => ({ label: s.label, color: s.color, value: yFmt(rawValue(i, idx)) })),
    };
  }

  function gradient(color: string, down: boolean) {
    return (u: uPlot) => {
      const { top, height: h } = u.bbox;
      const g = u.ctx.createLinearGradient(0, top, 0, top + h);
      const strong = rgba(color, stacked ? 0.55 : 0.4);
      const weak = rgba(color, stacked ? 0.18 : 0.0);
      if (mirrored) {
        // en espejo el degradado nace en la línea central
        g.addColorStop(0, down ? weak : strong);
        g.addColorStop(0.5, down ? weak : rgba(color, 0.05));
        g.addColorStop(0.5, down ? rgba(color, 0.05) : weak);
        g.addColorStop(1, down ? strong : weak);
      } else {
        g.addColorStop(0, strong);
        g.addColorStop(1, weak);
      }
      return g;
    };
  }

  /** Etiquetas del eje x según el rango: segundos, minutos o fecha. */
  function timeLabel(t: number, ticks: number[]): string {
    const span = ticks.length > 1 ? ticks[ticks.length - 1] - ticks[0] : 0;
    const d = new Date(t * 1000);
    if (span > 2 * 86400) return d.toLocaleDateString("es", { day: "2-digit", month: "2-digit" }) + " " + d.toLocaleTimeString("es", { hour: "2-digit", minute: "2-digit", hour12: false });
    if (span > 1200) return d.toLocaleTimeString("es", { hour: "2-digit", minute: "2-digit", hour12: false });
    return d.toLocaleTimeString("es", { hour12: false });
  }

  function prepare(): uPlot.AlignedData {
    const t = metrics.view(metrics.h.t);
    let ys = data().map((arr, i) => (series[i]?.negate ? arr.map((v) => -v) : arr));
    if (stacked) {
      const acc = new Array(t.length).fill(0);
      ys = ys.map((arr) => arr.map((v, j) => (acc[j] += v)));
    }
    return [t, ...ys];
  }

  // Se construye una sola vez: props y datos se leen sin crear dependencias.
  $effect(() => untrack(mount));

  function mount() {
    const axis = {
      stroke: C.muted,
      grid: { stroke: C.grid, width: 1 },
      ticks: { stroke: C.grid, width: 1, size: 4 },
      font: `10px ${MONO}`,
    };
    const opts: uPlot.Options = {
      width: el.clientWidth || 300,
      height,
      legend: { show: false },
      cursor: { points: { size: 6, fill: C.bg }, y: false, drag: { x: false, y: false } },
      padding: [8, 6, 0, 0],
      scales: {
        x: { time: true },
        y: {
          range: (_u, min, max) => {
            if (mirrored) {
              const m = Math.max(Math.abs(min), Math.abs(max), 1) * 1.1;
              return [-m, m];
            }
            return [yMin ?? Math.min(0, min), yMax ?? Math.max(max * 1.1, yFloor)];
          },
        },
      },
      axes: [
        { ...axis, space: 70, values: (_u, ticks) => ticks.map((t) => timeLabel(t, ticks)) },
        { ...axis, size: 62, values: (_u, ticks) => ticks.map((v) => yFmt(Math.abs(v))) },
      ],
      series: [
        {},
        ...series.map((s) => ({
          label: s.label,
          stroke: s.color,
          width: s.width ?? 1.6,
          dash: s.dash,
          fill: s.fill || stacked ? gradient(s.color, !!s.negate) : undefined,
          points: { show: false },
        })),
      ],
      // Al apilar, cada serie rellena solo hasta la anterior.
      hooks: { setCursor: [onCursor], drawSeries: [glowHook] },
      bands: stacked ? series.slice(1).map((s, i) => ({ series: [i + 2, i + 1] as [number, number], fill: rgba(s.color, 0.35) })) : undefined,
    };
    plot = new uPlot(opts, prepare(), el);
    const ro = new ResizeObserver(() => plot?.setSize({ width: el.clientWidth, height }));
    ro.observe(el);
    const unwatch = watchVisible(el, (v) => (visible = v));
    return () => {
      ro.disconnect();
      unwatch();
      plot?.destroy();
      plot = null;
    };
  }

  $effect(() => {
    metrics.tick;
    settings.windowSec;
    if (visible) plot?.setData(prepare());
  });
</script>

<div class="wrap">
  <div class="timechart" bind:this={el}></div>
  {#if tip}
    <div class="tip" style:left="{tip.x}px" style:top="{tip.y}px" class:flip={tip.x > (el?.clientWidth ?? 0) * 0.6}>
      <div class="time">{tip.time}</div>
      {#each tip.rows as r}
        <div class="row"><i style:background={r.color}></i>{r.label}<b>{r.value}</b></div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
  }
  .tip {
    position: absolute;
    transform: translate(14px, -50%);
    pointer-events: none;
    z-index: 10;
    background: color-mix(in srgb, var(--bg) 92%, transparent);
    border: 1px solid color-mix(in srgb, var(--cyan) 40%, transparent);
    box-shadow: 0 0 12px color-mix(in srgb, var(--cyan) 25%, transparent);
    padding: 6px 8px;
    font-size: 11px;
    white-space: nowrap;
  }
  .tip.flip {
    transform: translate(calc(-100% - 14px), -50%);
  }
  .tip .time {
    color: var(--muted);
    margin-bottom: 3px;
  }
  .tip .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .tip i {
    width: 8px;
    height: 8px;
  }
  .tip b {
    margin-left: auto;
    padding-left: 12px;
    color: var(--text);
  }
  .timechart {
    width: 100%;
    min-width: 0;
  }
  .timechart :global(.u-cursor-x) {
    border-right: 1px dashed color-mix(in srgb, var(--cyan) 40%, transparent);
  }
</style>
