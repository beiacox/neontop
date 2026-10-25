<script lang="ts">
  import { metrics } from "$lib/store.svelte";
  import { rgba } from "$lib/theme";
  import { settings } from "$lib/settings.svelte";
  import type { SeriesLike } from "$lib/series";

  interface Props {
    values: SeriesLike;
    color: string;
    height?: number;
    max?: number;
    /** Nº de puntos a mostrar (los últimos). */
    points?: number;
    /** Contador externo que fuerza el redibujado (p. ej. `procs.tick`). */
    version?: number;
  }
  let { values, color, height = 36, max, points = 60, version }: Props = $props();

  const id = `sp${Math.random().toString(36).slice(2, 8)}`;

  let path = $derived.by(() => {
    // `values` suele ser un array de la historia que se muta en sitio
    metrics.tick;
    version;
    const v = values.slice(-points);
    if (v.length < 2) return { line: "", area: "" };
    const top = max ?? Math.max(...v, 1e-9);
    const step = 100 / (points - 1);
    const off = (points - v.length) * step;
    const pts = v.map((y, i) => `${(off + i * step).toFixed(2)},${(100 - (Math.min(y, top) / top) * 92).toFixed(2)}`);
    const line = `M${pts.join("L")}`;
    return { line, area: `${line}L100,100L${off.toFixed(2)},100Z` };
  });
</script>

<svg class="spark" viewBox="0 0 100 100" preserveAspectRatio="none" style:height="{height}px">
  <defs>
    <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color={rgba(color, 0.45)} />
      <stop offset="1" stop-color={rgba(color, 0)} />
    </linearGradient>
  </defs>
  <path d={path.area} fill="url(#{id})" />
  <!-- halo como trazo ancho translúcido: mucho más barato que un filtro drop-shadow -->
  {#if settings.glowOn}<path d={path.line} fill="none" stroke={rgba(color, 0.25)} stroke-width="4" vector-effect="non-scaling-stroke" />{/if}
  <path d={path.line} fill="none" stroke={color} stroke-width="1.5" vector-effect="non-scaling-stroke" />
</svg>

<style>
  .spark {
    width: 100%;
    display: block;
  }
</style>
