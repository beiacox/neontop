<script lang="ts">
  import uPlot from "uplot";
  import { untrack } from "svelte";
  import "uplot/dist/uPlot.min.css";
  import { metrics } from "$lib/store.svelte";
  import { settings } from "$lib/settings.svelte";
  import { C, MONO, rgba } from "$lib/theme";
  import { watchVisible } from "./visible";

  interface Props {
    /** Devuelve [x (s, relativos al último fotograma), y (ms)]. */
    data: () => { x: number[]; y: number[] };
    height?: number;
    color?: string;
    /** Tiempo de referencia (ms) para marcar tirones: mediana reciente. */
    median?: () => number;
  }
  let { data, height = 220, color = C.lime, median }: Props = $props();

  let el = $state<HTMLDivElement>()!;
  let plot: uPlot | null = null;
  let visible = $state(true);

  // líneas de referencia: 60 FPS (16,7 ms) y 30 FPS (33,3 ms)
  const REFS = [
    { ms: 1000 / 60, label: "60", color: C.cyan },
    { ms: 1000 / 30, label: "30", color: C.amber },
  ];

  function drawRefs(u: uPlot) {
    const { ctx, bbox } = u;
    ctx.save();
    ctx.font = `${10 * devicePixelRatio}px ${MONO}`;
    for (const r of REFS) {
      const y = u.valToPos(r.ms, "y", true);
      if (y < bbox.top || y > bbox.top + bbox.height) continue;
      ctx.strokeStyle = rgba(r.color, 0.45);
      ctx.setLineDash([4 * devicePixelRatio, 4 * devicePixelRatio]);
      ctx.lineWidth = devicePixelRatio;
      ctx.beginPath();
      ctx.moveTo(bbox.left, y);
      ctx.lineTo(bbox.left + bbox.width, y);
      ctx.stroke();
      // la etiqueta a la derecha: a la izquierda se pisaba con los números del eje
      ctx.fillStyle = rgba(r.color, 0.8);
      ctx.textAlign = "right";
      ctx.fillText(`${r.label} FPS`, bbox.left + bbox.width - 4 * devicePixelRatio, y - 3 * devicePixelRatio);
    }
    ctx.restore();
    // tirones: puntos sobre los fotogramas de más del doble de la mediana
    const med = median?.() ?? 0;
    if (!med) return;
    const xs = u.data[0] as number[];
    const ys = u.data[1] as number[];
    ctx.save();
    ctx.fillStyle = C.red;
    for (let i = 0; i < ys.length; i++) {
      if (ys[i] > med * 2 && ys[i] - med >= 4) {
        const x = u.valToPos(xs[i], "x", true);
        const y = u.valToPos(ys[i], "y", true);
        ctx.beginPath();
        ctx.arc(x, y, 2.5 * devicePixelRatio, 0, Math.PI * 2);
        ctx.fill();
      }
    }
    ctx.restore();
  }

  function prepare(): uPlot.AlignedData {
    const d = data();
    return [d.x, d.y];
  }

  $effect(() => untrack(mount));

  function mount() {
    const axis = { stroke: C.muted, grid: { stroke: C.grid, width: 1 }, ticks: { stroke: C.grid, width: 1, size: 4 }, font: `10px ${MONO}` };
    plot = new uPlot(
      {
        width: el.clientWidth || 300,
        height,
        legend: { show: false },
        cursor: { points: { size: 5, fill: C.bg }, y: false, drag: { x: false, y: false } },
        padding: [8, 6, 0, 0],
        scales: {
          x: { time: false },
          y: { range: (_u, _min, max) => [0, Math.max(20, Math.min(max * 1.15, 200))] },
        },
        axes: [
          { ...axis, space: 60, values: (_u, t) => t.map((v) => `${v.toFixed(v > -10 ? 1 : 0)} s`) },
          { ...axis, size: 52, values: (_u, t) => t.map((v) => `${v.toFixed(0)} ms`) },
        ],
        series: [
          {},
          {
            label: "ms",
            stroke: color,
            width: 1.3,
            fill: (u: uPlot) => {
              const g = u.ctx.createLinearGradient(0, u.bbox.top, 0, u.bbox.top + u.bbox.height);
              g.addColorStop(0, rgba(color, 0.35));
              g.addColorStop(1, rgba(color, 0.02));
              return g;
            },
            points: { show: false },
          },
        ],
        hooks: { draw: [drawRefs] },
      },
      prepare(),
      el,
    );
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
    settings.glowOn;
    if (visible) plot?.setData(prepare());
  });
</script>

<div class="frames" bind:this={el}></div>

<style>
  .frames {
    width: 100%;
    min-width: 0;
  }
</style>
