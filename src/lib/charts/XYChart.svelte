<script lang="ts">
  /**
   * Gráfica de líneas con datos propios (no ligada al historial en vivo): para partidas
   * guardadas. `x` son segundos desde el inicio; el eje muestra mm:ss o h:mm.
   */
  import uPlot from "uplot";
  import { untrack } from "svelte";
  import "uplot/dist/uPlot.min.css";
  import { C, MONO, rgba } from "$lib/theme";
  import type { SeriesDef } from "./TimeChart.svelte";

  interface Props {
    x: number[];
    ys: number[][];
    series: SeriesDef[];
    height?: number;
    yFmt?: (v: number) => string;
    yMin?: number;
    yMax?: number;
    /** Tope mínimo de la escala y. */
    yFloor?: number;
  }
  let { x, ys, series, height = 200, yFmt = (v) => v.toFixed(0), yMin, yMax, yFloor = 1 }: Props = $props();

  let el = $state<HTMLDivElement>()!;
  let tip = $state<{ left: number; time: string; rows: { label: string; color: string; value: string }[] } | null>(null);

  const clock = (s: number) => {
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = Math.floor(s % 60);
    return h ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}` : `${m}:${String(sec).padStart(2, "0")}`;
  };

  $effect(() =>
    untrack(() => {
      const axis = { stroke: C.muted, grid: { stroke: C.grid, width: 1 }, ticks: { stroke: C.grid, width: 1, size: 4 }, font: `10px ${MONO}` };
      const gradient = (color: string) => (u: uPlot) => {
        const g = u.ctx.createLinearGradient(0, u.bbox.top, 0, u.bbox.top + u.bbox.height);
        g.addColorStop(0, rgba(color, 0.35));
        g.addColorStop(1, rgba(color, 0.02));
        return g;
      };
      const plot = new uPlot(
        {
          width: el.clientWidth || 300,
          height,
          legend: { show: false },
          cursor: { points: { size: 5, fill: C.bg }, y: false, drag: { x: false, y: false } },
          padding: [8, 6, 0, 0],
          scales: { x: { time: false }, y: { range: (_u, min, max) => [yMin ?? Math.min(0, min), yMax ?? Math.max(max * 1.1, yFloor)] } },
          axes: [
            { ...axis, space: 70, values: (_u, t) => t.map(clock) },
            { ...axis, size: 56, values: (_u, t) => t.map((v) => yFmt(v)) },
          ],
          series: [{}, ...series.map((s) => ({ label: s.label, stroke: s.color, width: s.width ?? 1.5, dash: s.dash, fill: s.fill ? gradient(s.color) : undefined, points: { show: false } }))],
          hooks: {
            setCursor: [
              (u) => {
                const i = u.cursor.idx;
                if (i == null || u.cursor.left == null || u.cursor.left < 0) return (tip = null);
                tip = {
                  left: u.cursor.left + u.bbox.left / devicePixelRatio,
                  time: clock(x[i]),
                  rows: series.map((s, k) => ({ label: s.label, color: s.color, value: yFmt(ys[k][i] ?? 0) })),
                };
              },
            ],
          },
        },
        [x, ...ys],
        el,
      );
      const ro = new ResizeObserver(() => plot.setSize({ width: el.clientWidth, height }));
      ro.observe(el);
      return () => {
        ro.disconnect();
        plot.destroy();
      };
    }),
  );
</script>

<div class="xy" bind:this={el} onmouseleave={() => (tip = null)} role="img">
  {#if tip}
    <div class="tip" style:left="{tip.left}px">
      <b>{tip.time}</b>
      {#each tip.rows as r}<span style:color={r.color}>{r.label}: {r.value}</span>{/each}
    </div>
  {/if}
</div>

<style>
  .xy {
    position: relative;
    width: 100%;
    min-width: 0;
  }
  .tip {
    position: absolute;
    top: 4px;
    transform: translateX(8px);
    z-index: 5;
    pointer-events: none;
    display: grid;
    gap: 2px;
    padding: 6px 8px;
    font-size: 10.5px;
    background: color-mix(in srgb, var(--panel) 96%, transparent);
    border: 1px solid color-mix(in srgb, var(--cyan) 40%, transparent);
    border-radius: var(--radius-sm);
    white-space: nowrap;
  }
</style>
