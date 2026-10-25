<script lang="ts">
  import { onMount } from "svelte";
  import { metrics } from "$lib/store.svelte";
  import { settings, fmtInterval, fmtWindow } from "$lib/settings.svelte";
  import SettingsDrawer from "$lib/ui/SettingsDrawer.svelte";
  import { fmtDuration, fmtPct, fmtRate } from "$lib/format";
  import { memSegments } from "$lib/store.svelte";
  import Overview from "$lib/modules/Overview.svelte";
  import Cpu from "$lib/modules/Cpu.svelte";
  import Memory from "$lib/modules/Memory.svelte";
  import Network from "$lib/modules/Network.svelte";
  import Processes from "$lib/modules/Processes.svelte";
  import Gpu from "$lib/modules/Gpu.svelte";
  import Disks from "$lib/modules/Disks.svelte";
  import Sensors from "$lib/modules/Sensors.svelte";
  import Energy from "$lib/modules/Energy.svelte";
  import Games from "$lib/modules/Games.svelte";
  import { games } from "$lib/games.svelte";
  import { energy } from "$lib/energy.svelte";
  import { procs } from "$lib/procs.svelte";
  import { alerts } from "$lib/alerts.svelte";
  import { platform, initPlatform, platformReady } from "$lib/platform.svelte";
  import { fillRows } from "$lib/ui/fillRows";
  import WindowControls from "$lib/ui/WindowControls.svelte";
  import Placeholder from "$lib/modules/Placeholder.svelte";

  type Tab = "overview" | "cpu" | "mem" | "net" | "procs" | "disks" | "sensors" | "gpu" | "energy" | "games";

  const TABS: { id: Tab; label: string; icon: string; sprint?: number }[] = [
    { id: "overview", label: "Resumen", icon: "◈" },
    { id: "cpu", label: "CPU", icon: "▦" },
    { id: "mem", label: "Memoria", icon: "▤" },
    { id: "net", label: "Red", icon: "⇅" },
    { id: "procs", label: "Procesos", icon: "≣" },
    { id: "disks", label: "Discos", icon: "◎" },
    { id: "sensors", label: "Sensores", icon: "✺" },
    { id: "gpu", label: "GPU", icon: "▣" },
    { id: "energy", label: "Energía", icon: "ϟ" },
    { id: "games", label: "Juegos", icon: "◭" },
  ];
  const INTERVALS = [250, 500, 1000, 2000, 5000];
  const WINDOWS = [60, 300, 900, 3600, 6 * 3600, Infinity];

  // la pestaña se guarda en los ajustes para reabrir la app donde se dejó
  let tab = $derived((TABS.some((t) => t.id === settings.tab) ? settings.tab : "overview") as Tab);
  let now = $state(new Date());
  let error = $state<string | null>(null);
  let showSettings = $state(false);
  let loginKey = $state("");

  $effect(() => {
    document.body.classList.toggle("no-glow", !settings.glowOn);
    document.body.classList.toggle("no-scan", !settings.scanlinesOn);
  });

  onMount(() => {
    metrics.onSample((s) => alerts.check(s));
    energy.init();
    games.init();
    // fuera de Tauri: interfaz web conectada al equipo, o datos simulados sin backend
    initPlatform().then(() => {
      if (platform.needAuth) return;
      Promise.all([metrics.start(), procs.init()])
        .then(platformReady)
        .catch((e) => (error = String(e)));
    });
    const id = setInterval(() => (now = new Date()), 1000);
    return () => clearInterval(id);
  });

  let s = $derived(metrics.snap);
  // mini-indicadores de la barra lateral
  let side = $derived(
    s
      ? {
          cpu: s.cpu.total.usage,
          mem: (memSegments(s.mem).used / s.mem.total) * 100,
          net: s.net.ifaces.filter((i) => i.kind === "ethernet" || i.kind === "wifi").reduce((a, i) => a + i.rxRate + i.txRate, 0),
          disk: s.disks.disks.filter((d) => d.kind !== "virtual").reduce((a, d) => a + d.readRate + d.writeRate, 0),
          temp: Math.max(0, ...s.sensors.chips.flatMap((c) => c.readings.filter((r) => r.kind === "temp").map((r) => r.value))),
        }
      : null,
  );
  let current = $derived(TABS.find((t) => t.id === tab)!);
</script>

<div class="app">
  <aside>
    <!-- logo y cabecera sirven para arrastrar la ventana (data-tauri-drag-region solo actúa
         sobre el elemento que lo lleva, por eso lo llevan también los textos) -->
    <div class="logo" data-tauri-drag-region>
      <span class="n" data-tauri-drag-region>NEON</span><span class="t" data-tauri-drag-region>TOP</span>
    </div>
    <nav>
      {#each TABS as t}
        <button class:active={tab === t.id} class:soon={t.sprint} onclick={() => settings.setTab(t.id)}>
          <span class="icon">{t.icon}</span>
          <span class="label">{t.label}</span>
          {#if side && t.id === "cpu"}<span class="mini">{fmtPct(side.cpu)}</span>{/if}
          {#if side && t.id === "mem"}<span class="mini">{fmtPct(side.mem)}</span>{/if}
          {#if side && t.id === "net"}<span class="mini">{fmtRate(side.net)}</span>{/if}
          {#if side && t.id === "disks"}<span class="mini">{fmtRate(side.disk)}</span>{/if}
          {#if side && t.id === "sensors" && side.temp}<span class="mini">{side.temp.toFixed(0)}°</span>{/if}
          {#if t.id === "gpu" && s?.gpus[0]?.busy != null}<span class="mini">{fmtPct(s.gpus[0].busy)}</span>{/if}
          {#if t.id === "games" && s?.games.games.some((g) => g.active)}<span class="mini">{s.games.games.find((g) => g.active)?.fps} FPS</span>{/if}
          {#if t.id === "energy" && s && s.energy.totalMethod !== "none"}<span class="mini">{s.energy.totalW.toFixed(0)} W</span>{/if}
          {#if t.id === "procs" && procs.list.length}<span class="mini">{procs.list.length}</span>{/if}
          {#if t.sprint}<span class="mini">S{t.sprint}</span>{/if}
        </button>
      {/each}
    </nav>
    <div class="foot">v0.13.0{platform.mode === "demo" ? " · modo demo" : platform.mode === "web" ? " · web" : ""}</div>
  </aside>

  <main>
    <header data-tauri-drag-region>
      <div class="title" data-tauri-drag-region>
        <h1 data-tauri-drag-region>{current.label}</h1>
        {#if metrics.info}
          <span class="host" data-tauri-drag-region>{metrics.info.hostname} · {metrics.info.os} · {metrics.info.kernel}</span>
        {/if}
      </div>
      <!-- los huecos entre controles también arrastran la ventana -->
      <div class="controls" data-tauri-drag-region>
        <button class="icon-btn" class:paused={settings.paused} title={settings.paused ? "Reanudar" : "Pausar"} onclick={() => settings.togglePause()}>
          {settings.paused ? "▶" : "⏸"}
        </button>
        <div class="seg" title="Intervalo de muestreo">
          <span>muestreo</span>
          {#each INTERVALS as ms}
            <button class:on={settings.intervalMs === ms} onclick={() => settings.setInterval(ms)}>{fmtInterval(ms)}</button>
          {/each}
          {#if !INTERVALS.includes(settings.intervalMs)}
            <button class="on" onclick={() => (showSettings = true)}>{fmtInterval(settings.intervalMs)}</button>
          {/if}
        </div>
        <div class="seg" title="Ventana de historial">
          <span>ventana</span>
          {#each WINDOWS as w}
            <button class:on={settings.windowSec === w} onclick={() => settings.setWindow(w)}>{fmtWindow(w)}</button>
          {/each}
          {#if !WINDOWS.includes(settings.windowSec)}
            <button class="on" onclick={() => (showSettings = true)}>{fmtWindow(settings.windowSec)}</button>
          {/if}
        </div>
        <button class="icon-btn" title="Ajustes" onclick={() => (showSettings = true)}>⚙</button>
        <div class="clock" data-tauri-drag-region>
          <b data-tauri-drag-region>{now.toLocaleTimeString("es", { hour12: false })}</b>
          {#if settings.paused}<span class="paused-label">EN PAUSA</span>{:else if s}<span data-tauri-drag-region>up {fmtDuration(s.uptime)}</span>{/if}
        </div>
        {#if platform.desktop}<WindowControls />{/if}
      </div>
    </header>

    {#if platform.offline}<div class="offline">Sin conexión con el equipo · reintentando…</div>{/if}
    <div class="content" {@attach fillRows}>
      {#if platform.needAuth}
        <form class="login" onsubmit={(e) => { e.preventDefault(); platform.login(loginKey); }}>
          <h2>Acceso a NeonTop</h2>
          <p>Abre el enlace o escanea el código QR de <b>Ajustes → Acceso web</b> en el equipo, o escribe aquí la clave de acceso.</p>
          <input bind:value={loginKey} placeholder="clave de acceso" autocomplete="off" autocapitalize="off" spellcheck="false" />
          <button type="submit" disabled={!loginKey.trim()}>Entrar</button>
        </form>
      {:else if error}
        <p class="error">Error al conectar con el backend: {error}</p>
      {:else if !s}
        <div class="boot">INICIALIZANDO SENSORES<span class="dots">...</span></div>
      {:else}
        <!-- al cambiar de tema se remonta todo para que las gráficas recojan la nueva paleta -->
        {#key settings.resolvedTheme}
          {#if tab === "overview"}
            <Overview />
          {:else if tab === "cpu"}
            <Cpu />
          {:else if tab === "mem"}
            <Memory />
          {:else if tab === "net"}
            <Network />
          {:else if tab === "procs"}
            <Processes />
          {:else if tab === "gpu"}
            <Gpu />
          {:else if tab === "disks"}
            <Disks />
          {:else if tab === "sensors"}
            <Sensors />
          {:else if tab === "energy"}
            <Energy />
          {:else if tab === "games"}
            <Games />
          {:else}
            <Placeholder name={current.label.toUpperCase()} sprint={current.sprint ?? 0} />
          {/if}
        {/key}
      {/if}
    </div>
  </main>
</div>

<SettingsDrawer bind:open={showSettings} />

<div class="toasts">
  {#each alerts.toasts as t (t.id)}
    <div class="toast" role="alert">
      <button class="tmain" onclick={() => alerts.dismiss(t.id)} title="Cerrar">
        <b>{t.title}</b>
        <span>{t.body}</span>
      </button>
      <div class="tact">
        <button onclick={() => alerts.mute(t.rule, 60)}>Silenciar 1 h</button>
        {#if t.key}
          <button onclick={() => alerts.ignore(t.rule, t.key)}>Ignorar {t.key}</button>
        {:else}
          <button onclick={() => (alerts.update(t.rule, { enabled: false }), alerts.dismiss(t.id))}>No avisar más</button>
        {/if}
      </div>
    </div>
  {/each}
</div>

<style>
  .app {
    display: grid;
    grid-template-columns: 190px 1fr;
    height: 100vh;
  }
  aside {
    display: flex;
    flex-direction: column;
    border-right: 1px solid color-mix(in srgb, var(--cyan) 15%, transparent);
    background: linear-gradient(180deg, color-mix(in srgb, var(--violet) 8%, transparent), color-mix(in srgb, var(--bg) 60%, transparent));
    padding: 18px 10px;
  }
  .logo {
    font-family: var(--display);
    font-weight: 900;
    font-size: 24px;
    letter-spacing: 0.08em;
    padding: 0 8px 22px;
  }
  .logo .n {
    color: var(--cyan);
    text-shadow:
      0 0 10px var(--cyan),
      0 0 22px color-mix(in srgb, var(--cyan) 50%, transparent);
  }
  .logo .t {
    color: var(--magenta);
    text-shadow:
      0 0 10px var(--magenta),
      0 0 22px color-mix(in srgb, var(--magenta) 50%, transparent);
    /* parpadeo solo al pasar el ratón: una animación infinita obliga a repintar sin parar */
  }
  .logo:hover .t {
    animation: flicker 1.2s 1;
  }
  @keyframes flicker {
    0%, 92%, 94%, 97%, 100% { opacity: 1; }
    93%, 96% { opacity: 0.35; }
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  nav button {
    display: grid;
    grid-template-columns: 20px 1fr auto;
    align-items: center;
    gap: 8px;
    background: none;
    border: 1px solid transparent;
    color: var(--text);
    font: inherit;
    padding: 9px 10px;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s, border-color 0.15s;
  }
  nav button:hover {
    background: color-mix(in srgb, var(--cyan) 6%, transparent);
  }
  nav button.active {
    border-color: color-mix(in srgb, var(--cyan) 50%, transparent);
    background: linear-gradient(90deg, color-mix(in srgb, var(--cyan) 18%, transparent), transparent);
    box-shadow: inset 3px 0 0 var(--cyan), 0 0 14px color-mix(in srgb, var(--cyan) 15%, transparent);
  }
  nav button.soon {
    opacity: 0.45;
  }
  .icon {
    color: var(--cyan);
    font-size: 15px;
  }
  .mini {
    font-size: 10px;
    color: var(--muted);
  }
  .foot {
    margin-top: auto;
    font-size: 10px;
    color: var(--muted);
    padding: 0 8px;
  }
  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    padding: 14px 22px;
    border-bottom: 1px solid color-mix(in srgb, var(--cyan) 12%, transparent);
    background: color-mix(in srgb, var(--bg) 70%, transparent);
  }
  h1 {
    margin: 0;
    font-family: var(--display);
    font-size: 20px;
    font-weight: 700;
    letter-spacing: var(--title-spacing);
    text-transform: var(--title-transform);
    color: var(--text);
    text-shadow: 0 0 12px color-mix(in srgb, var(--cyan) 50%, transparent);
  }
  .host {
    font-size: 11px;
    color: var(--muted);
  }
  .controls {
    display: flex;
    user-select: none;
    flex-wrap: wrap;
    justify-content: flex-end;
    align-items: center;
    gap: 10px;
  }
  .seg {
    display: flex;
    align-items: center;
    border: 1px solid color-mix(in srgb, var(--cyan) 20%, transparent);
  }
  .seg span {
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--muted);
    padding: 0 8px;
  }
  .seg button {
    background: none;
    border: none;
    border-left: 1px solid color-mix(in srgb, var(--cyan) 12%, transparent);
    color: var(--muted);
    font: inherit;
    font-size: 11px;
    padding: 5px 9px;
    cursor: pointer;
  }
  .seg button.on {
    color: var(--bg);
    background: var(--cyan);
    box-shadow: 0 0 12px color-mix(in srgb, var(--cyan) var(--glow-a), transparent);
  }
  .toasts {
    position: fixed;
    right: 18px;
    bottom: 18px;
    z-index: 70;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 380px;
  }
  .tmain {
    display: flex;
    flex-direction: column;
    gap: 3px;
    text-align: left;
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }
  .tact {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 6px;
  }
  .tact button {
    font: inherit;
    font-size: 10.5px;
    padding: 2px 8px;
    background: transparent;
    color: var(--text);
    border: 1px solid color-mix(in srgb, var(--red) 50%, transparent);
    cursor: pointer;
  }
  .tact button:hover {
    background: color-mix(in srgb, var(--red) 20%, transparent);
  }
  .toast {
    text-align: left;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 10px 14px;
    font: inherit;
    font-size: 11px;
    color: var(--text);
    background: color-mix(in srgb, var(--bg) 92%, var(--red));
    border: 1px solid var(--red);
    border-left-width: 3px;
    box-shadow: 0 0 18px color-mix(in srgb, var(--red) 35%, transparent);
    cursor: pointer;
  }
  .toast b {
    color: var(--red);
    font-size: 12px;
  }
  .toast span {
    color: var(--muted);
  }
  .icon-btn {
    width: 32px;
    height: 30px;
    background: none;
    border: 1px solid color-mix(in srgb, var(--cyan) 25%, transparent);
    color: var(--cyan);
    font-size: 14px;
    cursor: pointer;
  }
  .icon-btn:hover {
    background: color-mix(in srgb, var(--cyan) 10%, transparent);
  }
  .icon-btn.paused {
    color: var(--bg);
    background: var(--amber);
    border-color: var(--amber);
    box-shadow: 0 0 12px color-mix(in srgb, var(--amber) var(--glow-a), transparent);
  }
  .paused-label {
    color: var(--amber);
    letter-spacing: 0.15em;
  }
  .clock {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    font-size: 10px;
    color: var(--muted);
  }
  .clock b {
    font-family: var(--display);
    font-size: 16px;
    color: var(--magenta);
    text-shadow: 0 0 10px var(--magenta);
  }
  .content {
    flex: 1;
    overflow-y: auto;
    padding: 18px 22px 28px;
  }
  .boot {
    height: 60vh;
    display: grid;
    place-content: center;
    font-family: var(--display);
    letter-spacing: 0.3em;
    color: var(--cyan);
    text-shadow: 0 0 12px var(--cyan);
  }
  .dots {
    animation: flicker 1s infinite;
  }
  .offline {
    padding: 6px 22px;
    font-size: 11px;
    color: var(--bg);
    background: var(--amber);
  }
  .login {
    max-width: 380px;
    margin: 12vh auto 0;
    display: grid;
    gap: 12px;
  }
  .login h2 {
    font-family: var(--display);
    color: var(--cyan);
    margin: 0;
  }
  .login p {
    color: var(--muted);
    line-height: 1.6;
    margin: 0;
  }
  .login input,
  .login button {
    font: inherit;
    font-size: 15px;
    padding: 10px 12px;
    background: color-mix(in srgb, var(--cyan) 6%, transparent);
    border: 1px solid color-mix(in srgb, var(--cyan) 40%, transparent);
    color: var(--text);
  }
  .login button {
    background: var(--cyan);
    color: var(--bg);
    cursor: pointer;
  }
  /* ---- pantallas estrechas: la barra lateral pasa arriba como pestañas desplazables */
  @media (max-width: 900px) {
    .app {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(0, 1fr);
      height: 100dvh;
    }
    aside {
      flex-direction: row;
      align-items: center;
      gap: 8px;
      padding: 6px 8px;
      border-right: none;
      border-bottom: 1px solid color-mix(in srgb, var(--cyan) 15%, transparent);
      min-width: 0;
    }
    .logo {
      font-size: 15px;
      padding: 0 4px;
      flex-shrink: 0;
    }
    nav {
      flex-direction: row;
      overflow-x: auto;
      scrollbar-width: none;
      min-width: 0;
    }
    nav::-webkit-scrollbar {
      display: none;
    }
    nav button {
      grid-template-columns: auto auto;
      white-space: nowrap;
      padding: 7px 10px;
      gap: 6px;
    }
    nav button.active {
      box-shadow: inset 0 -3px 0 var(--cyan);
    }
    nav .mini,
    .foot {
      display: none;
    }
    header {
      flex-wrap: wrap;
      padding: 8px 12px;
      gap: 8px;
    }
    .host {
      display: none;
    }
    h1 {
      font-size: 16px;
    }
    .controls {
      gap: 6px;
    }
    .content {
      padding: 12px 10px 24px;
    }
  }
  @media (max-width: 600px) {
    .logo {
      display: none;
    }
    .seg span {
      display: none;
    }
    .seg button {
      padding: 6px 7px;
    }
    .clock {
      display: none;
    }
    .toasts {
      left: 10px;
      right: 10px;
      max-width: none;
    }
  }
  .error {
    color: var(--red);
  }
</style>
