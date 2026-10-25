<script lang="ts">
  /** Gráfico waffle: una rejilla de celdas repartidas proporcionalmente entre segmentos. */
  interface Seg {
    label: string;
    value: number;
    color: string;
  }
  let { segments, cols = 20, rows = 10, fmt }: { segments: Seg[]; cols?: number; rows?: number; fmt: (v: number) => string } = $props();

  let cells = $derived.by(() => {
    const total = segments.reduce((a, s) => a + s.value, 0) || 1;
    const n = cols * rows;
    // reparto por mayor resto para que siempre sumen exactamente n celdas
    const raw = segments.map((s) => (s.value / total) * n);
    const counts = raw.map(Math.floor);
    let left = n - counts.reduce((a, b) => a + b, 0);
    raw
      .map((r, i) => [r - Math.floor(r), i] as const)
      .sort((a, b) => b[0] - a[0])
      .forEach(([, i]) => left-- > 0 && counts[i]++);
    return segments.flatMap((s, i) => Array.from({ length: counts[i] }, () => s));
  });
  let hover = $state<string | null>(null);
</script>

<div class="waffle">
  <div class="cells" style:grid-template-columns="repeat({cols}, 1fr)">
    {#each cells as c, i (i)}
      <i
        style:--c={c.color}
        class:dim={hover && hover !== c.label}
        onmouseenter={() => (hover = c.label)}
        onmouseleave={() => (hover = null)}
        role="presentation"
      ></i>
    {/each}
  </div>
  <ul>
    {#each segments as s}
      <li class:active={hover === s.label} style:--c={s.color}>
        <i></i><span>{s.label}</span><b>{fmt(s.value)}</b>
      </li>
    {/each}
  </ul>
</div>

<style>
  .waffle {
    display: grid;
    grid-template-columns: 1fr 170px;
    gap: 14px;
    align-items: center;
  }
  .cells {
    display: grid;
    gap: 3px;
  }
  /* estrecho: la leyenda pasa debajo */
  @media (max-width: 700px) {
    .waffle {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .cells i {
    aspect-ratio: 1;
    background: var(--c);
    transition: opacity 0.15s;
  }
  .cells i.dim {
    opacity: 0.18;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: 11px;
  }
  li {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 4px;
  }
  li.active {
    background: color-mix(in srgb, var(--c) 15%, transparent);
  }
  li i {
    width: 9px;
    height: 9px;
    background: var(--c);
    box-shadow: 0 0 6px color-mix(in srgb, var(--c) var(--glow-a), transparent);
  }
  li b {
    margin-left: auto;
    /* mezclado con el texto para que los segmentos oscuros (libre) sigan siendo legibles */
    color: color-mix(in srgb, var(--c) 75%, var(--text));
    font-weight: 500;
  }
</style>
