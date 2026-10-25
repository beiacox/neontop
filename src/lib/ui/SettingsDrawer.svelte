<script lang="ts">
  import { settings, fmtInterval, fmtWindow } from "$lib/settings.svelte";
  import { metrics, MAX_POINTS } from "$lib/store.svelte";
  import ThemePicker from "./ThemePicker.svelte";
  import { fmtBytes } from "$lib/format";
  import AlertSettings from "./AlertSettings.svelte";
  import { exportCsv } from "$lib/export";
  import { platform } from "$lib/platform.svelte";
  import SystemSettings from "./SystemSettings.svelte";

  let exporting = $state(false);
  let exportMsg = $state<{ ok: boolean; text: string } | null>(null);
  async function doExport(range: "window" | "all") {
    exporting = true;
    exportMsg = null;
    try {
      const path = await exportCsv(range);
      if (path) exportMsg = { ok: true, text: `Guardado en ${path}` };
    } catch (e) {
      exportMsg = { ok: false, text: String(e) };
    } finally {
      exporting = false;
    }
  }

  let { open = $bindable(false) }: { open?: boolean } = $props();

  const PRESETS = [100, 250, 500, 1000, 2000, 5000, 10_000, 30_000, 60_000];
  const WINDOWS = [60, 120, 300, 900, 1800, 3600, 6 * 3600, 24 * 3600, Infinity];

  // Deslizador logarítmico: 0..1000 ↔ 100 ms..60 s
  const MIN = Math.log(100);
  const MAX = Math.log(60_000);
  const toSlider = (ms: number) => Math.round(((Math.log(ms) - MIN) / (MAX - MIN)) * 1000);
  const fromSlider = (v: number) => {
    const ms = Math.exp(MIN + (v / 1000) * (MAX - MIN));
    // redondeo "amable" según la magnitud
    const step = ms < 1000 ? 10 : ms < 10_000 ? 100 : 1000;
    return Math.round(ms / step) * step;
  };

  let slider = $state(toSlider(settings.intervalMs));
  let preview = $derived(fromSlider(slider));
  $effect(() => {
    slider = toSlider(settings.intervalMs);
  });

  let perMin = $derived(Math.round(60_000 / settings.intervalMs));
  let stored = $derived((metrics.tick, metrics.stored));
  // tiempo que cabe a resolución completa antes de la primera compactación
  let fullRes = $derived((MAX_POINTS * settings.intervalMs) / 1000);

  function fmtSpan(sec: number) {
    if (sec < 120) return `${Math.round(sec)} s`;
    if (sec < 7200) return `${Math.round(sec / 60)} min`;
    if (sec < 172800) return `${(sec / 3600).toFixed(1)} h`;
    return `${(sec / 86400).toFixed(1)} días`;
  }
</script>

<svelte:window onkeydown={(e) => open && e.key === "Escape" && (open = false)} />

{#if open}
  <button class="scrim" aria-label="Cerrar ajustes" onclick={() => (open = false)}></button>
{/if}
<aside class="drawer" class:open aria-hidden={!open}>
  <header>
    <h2>Ajustes</h2>
    <button class="close" onclick={() => (open = false)} aria-label="Cerrar">✕</button>
  </header>

  <section>
    <h3>Tema</h3>
    <ThemePicker />
  </section>

  <section>
    <h3>Velocidad de actualización</h3>
    <div class="big">
      <b>{fmtInterval(preview)}</b>
      <span>{Math.round(60_000 / preview)} muestras/min</span>
      <button class="pause" class:on={settings.paused} onclick={() => settings.togglePause()}>
        {settings.paused ? "▶ Reanudar" : "⏸ Pausar"}
      </button>
    </div>
    <input
      class="range"
      type="range"
      min="0"
      max="1000"
      bind:value={slider}
      onchange={() => settings.setInterval(fromSlider(slider))}
      aria-label="Intervalo de muestreo"
    />
    <div class="ticks">
      {#each [100, 1000, 10_000, 60_000] as ms}
        <span style:left="{toSlider(ms) / 10}%">{fmtInterval(ms)}</span>
      {/each}
    </div>
    <div class="chips">
      {#each PRESETS as ms}
        <button class:on={settings.intervalMs === ms} onclick={() => settings.setInterval(ms)}>{fmtInterval(ms)}</button>
      {/each}
      <label class="custom">
        <input
          type="number"
          min="100"
          max="60000"
          step="50"
          value={settings.intervalMs}
          onchange={(e) => settings.setInterval(+(e.currentTarget as HTMLInputElement).value)}
        />
        ms
      </label>
    </div>
    <p class="hint">
      Intervalos cortos (&lt; 500 ms) muestran picos breves pero consumen más CPU. Ahora: {perMin} muestras/min.
    </p>
  </section>

  <section>
    <h3>Ventana de historial</h3>
    <div class="chips">
      {#each WINDOWS as w}
        <button class:on={settings.windowSec === w} onclick={() => settings.setWindow(w)}>{fmtWindow(w)}</button>
      {/each}
    </div>
    <dl class="store">
      <dt>Guardado</dt>
      <dd>{stored.points.toLocaleString("es")} muestras · desde {stored.since ? new Date(stored.since * 1000).toLocaleString("es", { hour12: false }) : "—"}</dd>
      <dt>Memoria</dt>
      <dd>{fmtBytes(stored.bytes)} en {stored.series} series (arrays compactos de 32 bits)</dd>
      <dt>Resolución</dt>
      <dd>
        completa durante ~{fmtSpan(fullRes)} a este intervalo;
        {#if metrics.compactions > 0}
          lo anterior ya está compactado ({metrics.compactions}×).
        {:else}
          después, lo más antiguo se compacta y el historial sigue creciendo sin límite.
        {/if}
      </dd>
    </dl>
    <button class="danger" onclick={() => metrics.clearHistory()}>Borrar historial</button>
  </section>

  <section>
    <h3>Alertas</h3>
    <AlertSettings />
  </section>

  <section>
    <h3>Exportar</h3>
    <div class="chips">
      <button disabled={exporting} onclick={() => doExport("window")}>CSV · ventana visible</button>
      <button disabled={exporting} onclick={() => doExport("all")}>CSV · todo el historial</button>
    </div>
    <p class="hint">Valores sin reducir en unidades base (%, bytes, bytes/s, °C, MHz, W): CPU, núcleos, memoria, red, discos, GPU y sensores.</p>
    {#if exportMsg}<p class="msg" class:err={!exportMsg.ok}>{exportMsg.text}</p>{/if}
  </section>

  {#if platform.desktop}
    <SystemSettings />
  {:else if platform.mode === "web"}
    <section>
      <h3>Conexión web</h3>
      <p class="hint">
        Estás viendo NeonTop desde el navegador. Las acciones (pausar, cambiar el muestreo, terminar procesos) están
        <b>{platform.webActions ? "permitidas" : "desactivadas"}</b> en los ajustes del equipo. Los temas y la ventana de historial son propios de
        este navegador.
      </p>
    </section>
  {/if}

  {#if platform.desktop}
  <section>
    <h3>Bandeja del sistema</h3>
    <label class="toggle">
      <input type="checkbox" checked={settings.closeToTray} onchange={(e) => settings.setCloseToTray(e.currentTarget.checked)} />
      <span>Al cerrar la ventana, seguir en la bandeja</span>
    </label>
    <p class="hint">El icono de la bandeja muestra CPU, RAM, GPU y temperatura, y permite mostrar la ventana, pausar el muestreo o salir. Las alertas siguen funcionando con la ventana oculta.</p>
  </section>
  {/if}

  <section>
    <h3>Efectos</h3>
    <label class="toggle">
      <input type="checkbox" checked={settings.glow} onchange={(e) => settings.setEffect("glow", e.currentTarget.checked)} />
      <span>Brillo neón <small>(en los temas que lo usan)</small></span>
    </label>
    <label class="toggle">
      <input type="checkbox" checked={settings.scanlines} onchange={(e) => settings.setEffect("scanlines", e.currentTarget.checked)} />
      <span>Líneas de escaneo CRT <small>(en los temas que las usan)</small></span>
    </label>
  </section>
</aside>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: color-mix(in srgb, var(--bg) 55%, transparent);
    border: none;
    z-index: 50;
  }
  .drawer {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(500px, 100vw);
    z-index: 60;
    overflow-y: auto;
    padding: 18px 20px 30px;
    background: color-mix(in srgb, var(--bg) 94%, var(--violet));
    border-left: 1px solid color-mix(in srgb, var(--cyan) 35%, transparent);
    box-shadow: -10px 0 40px color-mix(in srgb, var(--cyan) 12%, transparent);
    transform: translateX(105%);
    transition: transform 0.22s ease;
  }
  .drawer.open {
    transform: none;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 8px;
  }
  h2 {
    margin: 0;
    font-family: var(--display);
    letter-spacing: 0.2em;
    text-transform: uppercase;
    font-size: 16px;
    color: var(--cyan);
    text-shadow: 0 0 10px var(--cyan);
  }
  .close {
    background: none;
    border: 1px solid color-mix(in srgb, var(--cyan) 30%, transparent);
    color: var(--text);
    width: 30px;
    height: 30px;
    cursor: pointer;
  }
  .drawer :global(section) {
    padding: 16px 0;
    border-bottom: 1px solid color-mix(in srgb, var(--cyan) 10%, transparent);
  }
  .drawer :global(h3) {
    margin: 0 0 12px;
    font-size: 10px;
    font-weight: 500;
    letter-spacing: 0.18em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .big {
    display: flex;
    align-items: baseline;
    gap: 12px;
  }
  .big b {
    font-family: var(--display);
    font-size: 30px;
    color: var(--cyan);
    text-shadow: 0 0 12px var(--cyan);
  }
  .big span {
    color: var(--muted);
    font-size: 11px;
  }
  .pause {
    margin-left: auto;
    background: none;
    border: 1px solid color-mix(in srgb, var(--amber) 50%, transparent);
    color: var(--amber);
    font: inherit;
    font-size: 11px;
    padding: 5px 10px;
    cursor: pointer;
  }
  .pause.on {
    background: var(--amber);
    color: var(--bg);
    box-shadow: 0 0 12px color-mix(in srgb, var(--amber) var(--glow-a), transparent);
  }
  .range {
    width: 100%;
    margin: 14px 0 2px;
    accent-color: var(--cyan);
  }
  /* marcas colocadas sobre la escala logarítmica del deslizador */
  .ticks {
    position: relative;
    height: 14px;
    font-size: 9px;
    color: var(--muted);
    margin-bottom: 10px;
  }
  .ticks span {
    position: absolute;
    transform: translateX(-50%);
  }
  .ticks span:first-child {
    transform: none;
  }
  .ticks span:last-child {
    transform: translateX(-100%);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  .chips button {
    background: none;
    border: 1px solid color-mix(in srgb, var(--cyan) 22%, transparent);
    color: var(--muted);
    font: inherit;
    font-size: 11px;
    padding: 5px 9px;
    cursor: pointer;
  }
  .chips button.on {
    color: var(--bg);
    background: var(--cyan);
    border-color: var(--cyan);
    box-shadow: 0 0 10px color-mix(in srgb, var(--cyan) var(--glow-a), transparent);
  }
  .custom {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--muted);
  }
  .custom input {
    width: 76px;
    background: var(--bg);
    border: 1px solid color-mix(in srgb, var(--cyan) 30%, transparent);
    color: var(--text);
    font: inherit;
    padding: 4px 6px;
  }
  .drawer :global(.hint) {
    font-size: 10px;
    color: var(--muted);
    margin: 10px 0 0;
  }
  .store {
    display: grid;
    grid-template-columns: 80px 1fr;
    gap: 6px 10px;
    font-size: 11px;
    margin: 14px 0;
  }
  .store dt {
    color: var(--muted);
    text-transform: uppercase;
    font-size: 9px;
    letter-spacing: 0.1em;
    padding-top: 2px;
  }
  .store dd {
    margin: 0;
  }
  .danger {
    background: none;
    border: 1px solid color-mix(in srgb, var(--red) 50%, transparent);
    color: var(--red);
    font: inherit;
    font-size: 11px;
    padding: 6px 12px;
    cursor: pointer;
  }
  .danger:hover {
    background: color-mix(in srgb, var(--red) 15%, transparent);
  }
  .drawer :global(.msg) {
    font-size: 10.5px;
    color: var(--lime);
    word-break: break-all;
  }
  .drawer :global(.msg.err) {
    color: var(--red);
  }
  .drawer :global(.toggle) {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
    cursor: pointer;
  }
  .drawer :global(.toggle small) {
    color: var(--muted);
    font-size: 10px;
  }
  .drawer :global(.toggle input) {
    accent-color: var(--cyan);
    width: 16px;
    height: 16px;
  }
</style>
