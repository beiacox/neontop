<script lang="ts">
  import { metrics } from "$lib/store.svelte";
  import { C, HEAT_LUT, MONO } from "$lib/theme";
  import { watchVisible } from "./visible";
  import type { SeriesLike } from "$lib/series";

  /**
   * Mapa de calor: una fila por serie (núcleos, discos…), una columna por muestra dentro de
   * la ventana. Los valores se interpretan en 0–100.
   */
  interface Props {
    height?: number;
    /** Series de la historia completa (se recortan a la ventana aquí). */
    rows: () => { label: string; data: SeriesLike }[];
    labelWidth?: number;
    unit?: string;
  }
  let { height = 220, rows, labelWidth = 34, unit = "%" }: Props = $props();

  let canvas: HTMLCanvasElement;
  let wrap: HTMLDivElement;
  let width = $state(600);
  let hover = $state<{ x: number; y: number; label: string; v: number } | null>(null);
  const LABEL_W = $derived(labelWidth);

  let visible = $state(true);

  $effect(() => {
    const ro = new ResizeObserver(() => (width = wrap.clientWidth));
    ro.observe(wrap);
    const unwatch = watchVisible(wrap, (v) => (visible = v));
    return () => {
      ro.disconnect();
      unwatch();
    };
  });

  function draw() {
    const dpr = devicePixelRatio || 1;
    // cambiar width/height reserva un búfer nuevo: solo cuando cambia el tamaño
    if (canvas.width !== Math.round(width * dpr) || canvas.height !== Math.round(height * dpr)) {
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
    }
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);

    // filas ya recortadas a la ventana y reducidas (con ventana ilimitada puede haber decenas de miles de muestras)
    const src = rows();
    const cores = src.map((r) => metrics.view(r.data));
    if (!cores.length) return;
    const n = cores[0].length;
    const rowH = height / cores.length;
    const plotW = width - LABEL_W;
    const colW = plotW / Math.max(n, 1);

    ctx.font = `10px ${MONO}`;
    ctx.textBaseline = "middle";
    for (let r = 0; r < cores.length; r++) {
      const row = cores[r];
      const y = r * rowH;
      ctx.fillStyle = C.muted;
      ctx.fillText(src[r].label, 4, y + rowH / 2, LABEL_W - 8);
      for (let i = 0; i < n; i++) {
        const v = row[i];
        ctx.fillStyle = HEAT_LUT[Math.max(0, Math.min(100, Math.round(v)))];
        ctx.fillRect(LABEL_W + i * colW, y + 1, Math.ceil(colW), rowH - 2);
      }
    }
  }

  $effect(() => {
    metrics.tick;
    width;
    if (visible) draw();
  });

  function onMove(e: MouseEvent) {
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const src = rows();
    if (!src.length || x < LABEL_W) return (hover = null);
    const row = src[Math.floor((y / height) * src.length)];
    const view = row ? metrics.view(row.data) : [];
    const i = Math.floor(((x - LABEL_W) / (width - LABEL_W)) * view.length);
    const v = view[i];
    hover = v == null ? null : { x, y, label: row.label, v };
  }
</script>

<div class="heat" bind:this={wrap} style:--lw="{LABEL_W}px">
  <canvas bind:this={canvas} style:width="{width}px" style:height="{height}px" onmousemove={onMove} onmouseleave={() => (hover = null)}></canvas>
  <div class="scale">
    <span>0%</span>
    <div class="bar" style:background="linear-gradient(90deg, {HEAT_LUT[0]}, {HEAT_LUT[25]}, {HEAT_LUT[50]}, {HEAT_LUT[75]}, {HEAT_LUT[100]})"></div>
    <span>100%</span>
  </div>
  {#if hover}
    <div class="tip" style:left="{hover.x + 12}px" style:top="{hover.y}px">{hover.label} · <b>{hover.v.toFixed(1)}{unit}</b></div>
  {/if}
</div>

<style>
  .heat {
    position: relative;
    width: 100%;
    min-width: 0;
  }
  canvas {
    display: block;
    /* sin esto el ancho inicial (600 px) impide que el contenedor encoja en pantallas estrechas */
    max-width: 100%;
  }
  .scale {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 10px;
    color: var(--muted);
    margin: 6px 0 0 var(--lw);
  }
  .bar {
    flex: 1;
    max-width: 240px;
    height: 6px;
  }
  .tip {
    position: absolute;
    transform: translateY(-50%);
    pointer-events: none;
    background: color-mix(in srgb, var(--bg) 92%, transparent);
    border: 1px solid color-mix(in srgb, var(--cyan) 40%, transparent);
    padding: 3px 7px;
    font-size: 11px;
    white-space: nowrap;
  }
</style>
