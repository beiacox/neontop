<script lang="ts">
  import { echarts, stripGlow, type EOption } from "./echarts";
  import { settings } from "$lib/settings.svelte";
  import { watchVisible } from "./visible";

  interface Props {
    option: EOption;
    height?: number;
    onclick?: (params: any) => void;
    /**
     * Animaciones de transición. Desactivadas por defecto: cada una redibuja el canvas a
     * 60 fps durante ~0,4 s por actualización, que es lo que más CPU gastaba en WebKit.
     */
    animate?: boolean;
  }
  let { option, height = 200, onclick, animate = false }: Props = $props();

  let el: HTMLDivElement;
  let chart: echarts.ECharts | null = null;
  let visible = $state(true);
  /** Ancho del contenedor, para las variantes por tamaño (`media`). */
  let width = $state(0);

  /**
   * Fusión profunda para las variantes por tamaño: los arrays (p. ej. `series`) se fusionan
   * elemento a elemento, como hace ECharts.
   */
  function merge(a: any, b: any): any {
    if (Array.isArray(a) && Array.isArray(b)) return a.map((x, i) => (i < b.length ? merge(x, b[i]) : x));
    if (a && b && typeof a === "object" && typeof b === "object" && !Array.isArray(b)) {
      const out = { ...a };
      for (const k of Object.keys(b)) out[k] = k in a ? merge(a[k], b[k]) : b[k];
      return out;
    }
    return b;
  }

  /**
   * Opciones con variantes por ancho: `{ baseOption, media: [{ query: { maxWidth }, option }] }`.
   * Se resuelven aquí y no con el `media` de ECharts, que en la compilación modular exige
   * importar el componente de línea temporal.
   */
  function resolve(o: Record<string, any>, w: number): Record<string, any> {
    if (!o.baseOption) return o;
    let base = o.baseOption;
    for (const m of o.media ?? []) if (!m.query?.maxWidth || (w > 0 && w <= m.query.maxWidth)) base = merge(base, m.option);
    return base;
  }

  $effect(() => {
    const c = echarts.init(el, null, { renderer: "canvas" });
    chart = c;
    c.on("click", (params) => onclick?.(params));
    const ro = new ResizeObserver(() => {
      width = el.clientWidth;
      c.resize();
    });
    ro.observe(el);
    const unwatch = watchVisible(el, (v) => (visible = v));
    return () => {
      ro.disconnect();
      unwatch();
      c.dispose();
      chart = null;
    };
  });

  $effect(() => {
    if (!visible) return;
    let o = resolve(option as Record<string, any>, width);
    if (!settings.glowOn) o = stripGlow(o);
    if (!animate) o = { ...o, animation: false };
    chart?.setOption(o, { lazyUpdate: true });
  });
</script>

<div class="echart" bind:this={el} style:height="{height}px"></div>

<style>
  .echart {
    width: 100%;
    min-width: 0;
  }
</style>
