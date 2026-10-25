<script lang="ts">
  /** Barra horizontal segmentada estilo btop. */
  interface Props {
    label: string;
    pct: number;
    color: string;
    right?: string;
    segments?: number;
  }
  let { label, pct, color, right, segments = 40 }: Props = $props();
  // se redondea a segmentos enteros para que la barra "salte" como en btop
  let fill = $derived((Math.round((Math.max(0, Math.min(100, pct)) / 100) * segments) / segments) * 100);
</script>

<!-- Un único relleno + una capa de rayas del color de fondo que hace de separación entre
     segmentos: antes eran `segments` nodos con sombra cada uno. -->
<div class="meter" style:--c={color}>
  <span class="label">{label}</span>
  <div class="track" style:--seg="{100 / segments}%">
    <i style:width="{fill}%"></i>
  </div>
  <span class="right">{right ?? `${pct.toFixed(0)}%`}</span>
</div>

<style>
  .meter {
    display: grid;
    grid-template-columns: 44px 1fr 86px;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    height: 16px;
  }
  .label {
    color: var(--muted);
  }
  .track {
    position: relative;
    height: 9px;
    background: color-mix(in srgb, var(--text) 7%, transparent);
  }
  .track::after {
    content: "";
    position: absolute;
    inset: 0;
    background: repeating-linear-gradient(90deg, transparent 0 calc(var(--seg) - 2px), var(--bg) calc(var(--seg) - 2px) var(--seg));
  }
  i {
    position: absolute;
    inset: 0 auto 0 0;
    background: linear-gradient(90deg, color-mix(in srgb, var(--c) 45%, transparent), var(--c));
  }
  .right {
    text-align: right;
    color: var(--c);
    font-variant-numeric: tabular-nums;
  }
</style>
