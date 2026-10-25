<script lang="ts">
  import { settings } from "$lib/settings.svelte";
  import { CATEGORIES, SYSTEM_THEME, THEMES, rgba, type Theme } from "$lib/theme";

  const FONT_NAMES: [string, string][] = [
    ["Orbitron", "Orbitron"],
    ["VT323", "VT323"],
    ["Lora", "Lora"],
    ["Space Grotesk", "Space Grotesk"],
    ["Inter", "Inter"],
    ["IBM Plex", "IBM Plex Mono"],
    ["JetBrains", "JetBrains Mono"],
  ];
  const fontName = (f: string) => FONT_NAMES.find(([k]) => f.includes(k))?.[1] ?? f;
  const byId = (id: string) => THEMES.find((t) => t.id === id)!;

  /** Radio de la miniatura según la forma del tema (escalado). */
  const miniRadius = (t: Theme) => (t.style.corners === "round" ? Math.max(3, Math.round(t.style.radius / 2.5)) : 0);
  // una misma curva para todas las miniaturas, así se comparan los estilos y no los datos
  const LINE = "M0,26 L10,22 L18,24 L26,12 L34,17 L42,8 L50,14 L58,6 L66,11 L74,4";
</script>

{#snippet mini(t: Theme)}
  {@const c = t.colors}
  {@const hud = t.style.titleCase === "upper"}
  <div
    class="mock"
    style:background={c.bg}
    style:font-family={t.style.body}
    class:grid={t.style.backdrop === "grid"}
    class:lines={t.style.backdrop === "lines"}
    style:--g={rgba(c.cyan, 0.1)}
  >
    <div class="side" style:background={rgba(c.panel, 0.7)} style:border-color={rgba(c.cyan, 0.2)}>
      <i style:background={c.cyan}></i><i style:background={c.muted}></i><i style:background={c.muted}></i>
    </div>
    <div
      class="panel"
      class:cut={t.style.corners === "cut"}
      style:background={c.panel}
      style:border-color={t.light ? rgba(c.text, 0.12) : rgba(c.cyan, Math.min(1, t.style.border / 100 + 0.1))}
      style:border-radius="{miniRadius(t)}px"
      style:box-shadow={t.light ? `0 2px 6px ${rgba(c.text, 0.1)}` : "none"}
      style:--acc={c.cyan}
      style:--bgc={c.bg}
    >
      <span
        class="title"
        style:font-family={t.style.display}
        style:color={hud ? c.cyan : c.text}
        style:text-transform={hud ? "uppercase" : "none"}
        style:letter-spacing={hud ? "0.12em" : "0"}
        style:text-shadow={t.style.glow ? `0 0 5px ${c.cyan}` : "none"}
      >
        {t.style.corners === "square" && hud ? "[CPU]" : "CPU"}
      </span>
      <svg viewBox="0 0 74 30" preserveAspectRatio="none">
        <path d="{LINE} L74,30 L0,30 Z" fill={rgba(c.cyan, 0.22)} />
        <path d={LINE} fill="none" stroke={c.cyan} stroke-width="1.6" vector-effect="non-scaling-stroke" style:filter={t.style.glow ? `drop-shadow(0 0 2px ${c.cyan})` : "none"} />
      </svg>
      <div class="bars">
        {#each [c.cyan, c.magenta, c.lime, c.amber] as col, i}
          <i style:background={col} style:width="{[80, 55, 35, 65][i]}%" style:border-radius="{miniRadius(t) ? 2 : 0}px"></i>
        {/each}
      </div>
    </div>
  </div>
{/snippet}

<div class="picker">
  {#each CATEGORIES as cat (cat.id)}
    <div class="cat">
      <h4>{cat.label}</h4>
      <div class="cards">
        {#if cat.id === "clasico"}
          <!-- tema automático: mitad claro, mitad oscuro -->
          <button class="card" class:on={settings.theme === SYSTEM_THEME} onclick={() => settings.setTheme(SYSTEM_THEME)}>
            <div class="split">
              <div class="half">{@render mini(byId("claro"))}</div>
              <div class="half dark">{@render mini(byId("oscuro"))}</div>
            </div>
            <div class="meta"><b>Sistema</b><small>sigue el modo claro/oscuro del escritorio</small></div>
          </button>
        {/if}
        {#each THEMES.filter((t) => t.category === cat.id) as t (t.id)}
          <button class="card" class:on={settings.theme === t.id} onclick={() => settings.setTheme(t.id)}>
            {@render mini(t)}
            <div class="meta">
              <b>{t.name}</b>
              <small>{t.light ? "claro" : "oscuro"} · {fontName(t.style.display)}</small>
            </div>
          </button>
        {/each}
      </div>
    </div>
  {/each}
</div>

<style>
  .picker {
    display: grid;
    gap: 14px;
  }
  h4 {
    margin: 0 0 7px;
    font-size: 10px;
    font-weight: 600;
    color: var(--muted);
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 5px;
    background: none;
    border: 1px solid color-mix(in srgb, var(--text) 12%, transparent);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font: inherit;
    color: var(--text);
    text-align: left;
    transition: border-color 0.15s, transform 0.15s;
  }
  .card:hover {
    border-color: color-mix(in srgb, var(--cyan) 50%, transparent);
    transform: translateY(-1px);
  }
  .card.on {
    border-color: var(--cyan);
    box-shadow: 0 0 0 1px var(--cyan);
  }
  .meta {
    display: flex;
    flex-direction: column;
    padding: 0 2px 1px;
    min-width: 0;
  }
  .meta b {
    font-size: 11px;
    font-weight: 600;
  }
  .meta small {
    font-size: 9px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* ---- miniatura */
  .mock {
    position: relative;
    display: grid;
    grid-template-columns: 14px 1fr;
    gap: 5px;
    padding: 6px;
    height: 72px;
    border-radius: 3px;
    overflow: hidden;
  }
  .mock.grid {
    background-image:
      linear-gradient(var(--g) 1px, transparent 1px),
      linear-gradient(90deg, var(--g) 1px, transparent 1px) !important;
    background-size: 8px 8px !important;
  }
  .mock.lines {
    background-image: repeating-linear-gradient(0deg, rgba(0, 0, 0, 0.35) 0 1px, transparent 1px 3px) !important;
  }
  .side {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 3px 2px;
    border: 1px solid;
    border-radius: 2px;
  }
  .side i {
    height: 3px;
    border-radius: 1px;
    opacity: 0.8;
  }
  .panel {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 4px 5px;
    border: 1px solid;
    min-width: 0;
  }
  /* esquina cortada del estilo Neón */
  .panel.cut::before {
    content: "";
    position: absolute;
    top: -1px;
    right: -1px;
    width: 7px;
    height: 7px;
    background: linear-gradient(225deg, var(--bgc) 0 48%, var(--acc) 48% 62%, transparent 62%);
  }
  .title {
    font-size: 9px;
    font-weight: 700;
    line-height: 1;
  }
  svg {
    width: 100%;
    height: 22px;
    display: block;
  }
  .bars {
    display: grid;
    gap: 2px;
  }
  .bars i {
    height: 2px;
  }

  /* tarjeta "Sistema": dos miniaturas cortadas en diagonal */
  .split {
    position: relative;
    height: 72px;
    border-radius: 3px;
    overflow: hidden;
  }
  .half {
    position: absolute;
    inset: 0;
  }
  .half.dark {
    clip-path: polygon(62% 0, 100% 0, 100% 100%, 38% 100%);
  }
</style>
