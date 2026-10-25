<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    title: string;
    accent?: string;
    /** Texto a la derecha del título (valor actual, unidad…). */
    extra?: Snippet;
    children: Snippet;
    span?: number;
    rows?: number;
    /** En el teléfono ocupa media fila (velocímetros y gráficos circulares, que van en pareja). */
    half?: boolean;
  }
  let { title, accent = "var(--cyan)", extra, children, span = 1, rows = 1, half = false }: Props = $props();
</script>

<section class="panel" data-half={half || undefined} style:--accent={accent} style:grid-column="span {span}" style:grid-row="span {rows}">
  <header>
    <h2 {title}><span class="tick"></span>{title}</h2>
    {#if extra}<div class="extra">{@render extra()}</div>{/if}
  </header>
  <div class="body">{@render children()}</div>
</section>

<style>
  /* La forma del panel depende del tema (html[data-corners] / [data-hud], ver theme.ts).
     Las esquinas "cortadas" se dibujan con pseudo-elementos: más barato que clip-path,
     que en WebKitGTK obliga a pintar el panel en una capa aparte en cada actualización. */
  .panel {
    position: relative;
    min-width: 0;
    background:
      linear-gradient(180deg, color-mix(in srgb, var(--accent) 5%, transparent), transparent 40%),
      var(--panel);
    border: 1px solid color-mix(in srgb, var(--accent) var(--border-mix), transparent);
    border-radius: var(--radius);
    padding: 10px 12px 12px;
  }
  :global(html[data-light="true"]) .panel {
    border-color: color-mix(in srgb, var(--text) 10%, transparent);
    box-shadow:
      0 1px 2px color-mix(in srgb, var(--text) 6%, transparent),
      0 6px 20px color-mix(in srgb, var(--text) 5%, transparent);
  }

  /* ---- esquinas cortadas (Neón) */
  :global(html[data-corners="cut"]) .panel::before,
  :global(html[data-corners="cut"]) .panel::after {
    content: "";
    position: absolute;
    width: 15px;
    height: 15px;
    pointer-events: none;
  }
  :global(html[data-corners="cut"]) .panel::before {
    top: -1px;
    right: -1px;
    background: linear-gradient(225deg, var(--bg) 0 48%, var(--accent) 48% 58%, transparent 58%);
  }
  :global(html[data-corners="cut"]) .panel::after {
    bottom: -1px;
    left: -1px;
    background: linear-gradient(45deg, var(--bg) 0 48%, color-mix(in srgb, var(--accent) 40%, transparent) 48% 56%, transparent 56%);
  }

  header {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 4px 8px;
    margin-bottom: 8px;
  }
  h2 {
    margin: 0;
    /* una sola línea: si no cabe se recorta, así los paneles en pareja quedan alineados */
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
    font-family: var(--display);
    font-size: 13px;
    font-weight: 600;
    letter-spacing: var(--title-spacing);
    text-transform: var(--title-transform);
    color: var(--text);
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .tick {
    width: 4px;
    height: 12px;
    border-radius: 2px;
    background: var(--accent);
  }

  /* ---- títulos HUD (Neón y terminal): color de acento, mayúsculas y brillo */
  :global(html[data-hud="true"]) h2 {
    font-size: 11px;
    font-weight: 700;
    color: var(--accent);
    text-shadow: 0 0 8px color-mix(in srgb, var(--accent) 70%, transparent);
  }
  :global(html[data-hud="true"]) .tick {
    width: 6px;
    height: 6px;
    border-radius: 0;
    transform: rotate(45deg);
    box-shadow: 0 0 8px color-mix(in srgb, var(--accent) var(--glow-a), transparent);
  }
  /* terminal: título entre corchetes, sin rombo */
  :global(html[data-hud="true"][data-corners="square"]) h2 {
    font-size: 17px;
    font-weight: 400;
    letter-spacing: 0.06em;
  }
  :global(html[data-hud="true"][data-corners="square"]) .tick {
    display: none;
  }
  :global(html[data-hud="true"][data-corners="square"]) h2::before {
    content: "[";
    opacity: 0.6;
  }
  :global(html[data-hud="true"][data-corners="square"]) h2::after {
    content: "]";
    opacity: 0.6;
  }

  /* paneles emparejados en el teléfono: título más compacto para que quepa en media fila */
  @media (max-width: 700px) {
    .panel[data-half] h2 {
      font-size: 11px;
      letter-spacing: 0.06em;
    }
    .panel[data-half] .extra {
      display: none;
    }
  }
  .extra {
    font-size: 11px;
    color: var(--muted);
    display: flex;
    flex-wrap: wrap;
    gap: 2px 12px;
    align-items: baseline;
  }
  .body {
    min-width: 0;
  }
</style>
