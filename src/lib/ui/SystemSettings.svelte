<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import { renderSVG } from "uqr";
  import { settings } from "$lib/settings.svelte";
  import { metrics } from "$lib/store.svelte";
  import { energy } from "$lib/energy.svelte";

  interface AppConfig {
    webEnabled: boolean;
    webPort: number;
    webToken: string;
    webActions: boolean;
    startHidden: boolean;
    localUrl: string;
    web: { running: boolean; error: string | null; urls: string[]; clients: number };
  }

  let cfg = $state<AppConfig | null>(null);
  let autostart = $state(false);
  let err = $state<string | null>(null);
  let port = $state(7340);
  let url = $state<string | null>(null);
  let showKey = $state(false);

  const load = async () => {
    cfg = await invoke<AppConfig>("get_app_config");
    port = cfg.webPort;
    autostart = await isEnabled().catch(() => false);
  };
  load();
  // el nº de clientes conectados se refresca mientras el panel está abierto
  $effect(() => {
    const id = setInterval(() => invoke<AppConfig>("get_app_config").then((c) => (cfg = c)), 3000);
    return () => clearInterval(id);
  });

  async function run(f: () => Promise<unknown>) {
    err = null;
    try {
      await f();
    } catch (e) {
      err = String(e);
    }
  }

  const setAutostart = (on: boolean) =>
    run(async () => {
      await (on ? enable() : disable());
      autostart = await isEnabled();
    });
  const setHidden = (hidden: boolean) => run(async () => (cfg = await invoke<AppConfig>("set_start_hidden", { hidden })));
  const setWeb = (enabled: boolean, actions: boolean) =>
    run(async () => (cfg = await invoke<AppConfig>("set_web", { enabled, port: Math.min(65535, Math.max(1024, Number(port) || 7340)), actions })));
  const newToken = () => run(async () => (cfg = await invoke<AppConfig>("web_new_token")));

  let rapl = $derived(metrics.snap?.energy.rapl ?? "absent");
  let raplRule = $derived(metrics.snap?.energy.raplRule ?? false);
  let raplBusy = $state(false);
  let membw = $derived(metrics.snap?.membw);
  const membwAccess = async (on: boolean) => {
    raplBusy = true;
    await run(() => invoke("membw_access", { enable: on }));
    raplBusy = false;
  };
  const raplAccess = async (on: boolean, persistent: boolean) => {
    raplBusy = true;
    await run(() => energy.raplAccess(on, persistent));
    raplBusy = false;
  };

  let links = $derived(cfg ? (cfg.web.urls.length ? cfg.web.urls : [cfg.localUrl]) : []);
  let chosen = $derived(url && links.includes(url) ? url : links[0]);
  let fullLink = $derived(cfg && chosen ? `${chosen}/?t=${cfg.webToken}` : "");
  let qr = $derived(fullLink ? renderSVG(fullLink, { border: 2, whiteColor: "#ffffff", blackColor: "#000000" }) : "");
</script>

{#if cfg}
  <section>
    <h3>Inicio con el sistema</h3>
    <label class="toggle">
      <input type="checkbox" checked={autostart} onchange={(e) => setAutostart(e.currentTarget.checked)} />
      <span>Abrir NeonTop al iniciar sesión</span>
    </label>
    <label class="toggle" class:off={!autostart}>
      <input type="checkbox" checked={cfg.startHidden} disabled={!autostart} onchange={(e) => setHidden(e.currentTarget.checked)} />
      <span>Arrancar oculto en la bandeja <small>(sigue midiendo, con alertas e interfaz web)</small></span>
    </label>
    <p class="hint">Si abres NeonTop cuando ya está en marcha, se muestra la ventana existente en lugar de abrir otra.</p>
  </section>

  <section>
    <h3>Permisos de administrador</h3>
    <label class="toggle">
      <input type="checkbox" checked={settings.adminActions} onchange={(e) => settings.setAdminActions(e.currentTarget.checked)} />
      <span>Acciones de administrador <small>(terminar o cambiar la prioridad de procesos de otros usuarios o del sistema, subir prioridades)</small></span>
    </label>
    <p class="hint">
      Cuando una acción necesite root, el sistema pedirá tu contraseña en su propia ventana (polkit) y la recordará unos minutos. NeonTop nunca la ve
      ni se ejecuta como root: solo lanza la orden concreta.
    </p>
    <div class="row">
      <span class="lbl">Consumo de CPU (RAPL)</span>
      {#if rapl === "ok"}
        <span class="ok">{raplRule ? "permitido siempre" : "permitido hasta reiniciar"}</span>
        {#if !raplRule}<button disabled={raplBusy} onclick={() => raplAccess(true, true)}>Hacer permanente</button>{/if}
        <button disabled={raplBusy} onclick={() => raplAccess(false, false)}>Retirar</button>
      {:else if rapl === "denied"}
        <span class="warn">bloqueado</span>
        <button disabled={raplBusy} onclick={() => raplAccess(true, false)}>Permitir hasta reiniciar</button>
        <button disabled={raplBusy} onclick={() => raplAccess(true, true)}>Permitir siempre</button>
      {:else}
        <span class="muted">no disponible en este equipo</span>
      {/if}
    </div>
    <div class="row">
      <span class="lbl">Tráfico de RAM en vivo</span>
      {#if membw?.status === "ok"}
        <span class="ok">activo</span>
        {#if membw.enabled}<button disabled={raplBusy} onclick={() => membwAccess(false)}>Desactivar</button>{/if}
      {:else if membw?.status === "nomodule" || membw?.status === "noperm"}
        <span class="warn">sin permiso</span>
        <button disabled={raplBusy} onclick={() => membwAccess(true)}>Activar</button>
      {:else}
        <span class="muted">{membw?.status === "unsupported" ? "no compatible con este procesador" : (membw?.message ?? "no disponible")}</span>
      {/if}
    </div>
  </section>

  <section>
    <h3>Acceso web <small>(teléfono y otros equipos)</small></h3>
    <label class="toggle">
      <input type="checkbox" checked={cfg.webEnabled} onchange={(e) => setWeb(e.currentTarget.checked, cfg!.webActions)} />
      <span>Servir la interfaz en la red local</span>
    </label>
    <div class="row">
      <span class="lbl">Puerto</span>
      <input class="port" type="number" min="1024" max="65535" bind:value={port} onchange={() => cfg!.webEnabled && setWeb(true, cfg!.webActions)} />
      {#if cfg.web.running}<span class="ok">activo · {cfg.web.clients} {cfg.web.clients === 1 ? "cliente" : "clientes"}</span>{/if}
    </div>
    <label class="toggle">
      <input type="checkbox" checked={cfg.webActions} onchange={(e) => setWeb(cfg!.webEnabled, e.currentTarget.checked)} />
      <span>Permitir acciones desde la web <small>(pausar, cambiar el muestreo, terminar procesos)</small></span>
    </label>
    {#if cfg.web.error}<p class="msg err">{cfg.web.error}</p>{/if}

    {#if cfg.webEnabled && cfg.web.running}
      <div class="web">
        <div class="qr">{@html qr}</div>
        <div class="links">
          <p class="hint">Escanea el código con el teléfono (conectado a la misma red) o abre uno de estos enlaces:</p>
          {#each links as l}
            <label class="link"><input type="radio" name="weburl" checked={l === chosen} onchange={() => (url = l)} /><code>{l}</code></label>
          {/each}
          <div class="row">
            <span class="lbl">Clave</span>
            <code class="key">{showKey ? cfg.webToken : "•".repeat(12)}</code>
            <button onclick={() => (showKey = !showKey)}>{showKey ? "Ocultar" : "Mostrar"}</button>
            <button onclick={newToken} title="Los enlaces y teléfonos anteriores dejarán de tener acceso">Nueva clave</button>
          </div>
        </div>
      </div>
      <p class="hint">
        Quien tenga el enlace con la clave puede ver los datos del equipo. La conexión no va cifrada (HTTP): úsalo en tu red de casa, no en redes
        públicas. Si el teléfono no conecta, puede que el cortafuegos bloquee el puerto {cfg.webPort}.
      </p>
    {/if}
  </section>
  {#if err}<p class="msg err">{err}</p>{/if}
{/if}

<style>
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 8px 0;
    font-size: 12px;
  }
  .lbl {
    color: var(--muted);
    font-size: 11px;
    min-width: 60px;
  }
  .row button {
    font: inherit;
    font-size: 11px;
    background: transparent;
    color: var(--cyan);
    border: 1px solid color-mix(in srgb, var(--cyan) 40%, transparent);
    padding: 3px 9px;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: wait;
  }
  .ok {
    color: var(--lime);
  }
  .warn {
    color: var(--amber);
  }
  .muted {
    color: var(--muted);
  }
  .off {
    opacity: 0.5;
  }
  .port {
    width: 90px;
    font: inherit;
    font-size: 12px;
    padding: 3px 8px;
    background: color-mix(in srgb, var(--cyan) 6%, transparent);
    border: 1px solid color-mix(in srgb, var(--cyan) 30%, transparent);
    color: var(--text);
  }
  .web {
    display: grid;
    grid-template-columns: 150px 1fr;
    gap: 14px;
    align-items: start;
    margin-top: 10px;
  }
  .qr :global(svg) {
    width: 150px;
    height: 150px;
    display: block;
    border-radius: 4px;
  }
  .links .hint {
    margin-top: 0;
  }
  .link {
    display: flex;
    gap: 6px;
    align-items: center;
    font-size: 12px;
    margin: 4px 0;
    cursor: pointer;
  }
  code {
    color: var(--cyan);
    word-break: break-all;
  }
  .key {
    font-size: 11px;
  }
  @media (max-width: 520px) {
    .web {
      grid-template-columns: 1fr;
    }
  }
</style>
