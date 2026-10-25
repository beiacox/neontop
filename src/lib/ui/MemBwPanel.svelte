<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import TimeChart from "$lib/charts/TimeChart.svelte";
  import { metrics } from "$lib/store.svelte";
  import { bench } from "$lib/bench.svelte";
  import { platform } from "$lib/platform.svelte";
  import { fmtGBs } from "$lib/format";
  import { C } from "$lib/theme";

  let bw = $derived(metrics.snap!.membw);
  let split = $derived(bw.read != null);
  /** Referencia para el %: lo máximo medido por el test de RAM (lectura con todos los hilos). */
  let peak = $derived(bench.last ? Math.max(bench.last.read, bench.last.copy) : null);

  let win = $derived.by(() => {
    metrics.tick;
    const v = metrics.raw(metrics.h.memBw.total);
    if (!v.length) return { avg: 0, max: 0 };
    let sum = 0;
    let max = 0;
    for (const x of v) {
      sum += x;
      if (x > max) max = x;
    }
    return { avg: sum / v.length, max };
  });

  let busy = $state(false);
  let msg = $state<string | null>(null);
  async function access(enable: boolean) {
    busy = true;
    msg = null;
    try {
      await invoke("membw_access", { enable });
      msg = enable ? "Permiso concedido: la medición empieza en unos segundos." : "Desactivado.";
    } catch (e) {
      msg = String(e);
    } finally {
      busy = false;
    }
  }

  const WHY: Record<string, string> = {
    nomodule: "Falta cargar el módulo del kernel que expone los contadores del controlador de memoria (amd_uncore).",
    noperm: "Leer los contadores de todo el sistema requiere un permiso de administrador (CAP_PERFMON).",
    nopmu: "El controlador de memoria no expone contadores en este sistema.",
    unsupported: "Este procesador no tiene contadores de tráfico de memoria compatibles (soportados: AMD Zen 2/Zen 3 e Intel con uncore_imc).",
    missing: "No se encuentra el lector de contadores (neontop-membw); reinstala el paquete de NeonTop.",
    error: "El lector de contadores falló.",
    starting: "Iniciando…",
  };
  let fixable = $derived(bw.status === "nomodule" || bw.status === "noperm");
  let series = $derived(
    split
      ? [
          { label: "lectura", color: C.cyan, fill: true },
          { label: "escritura", color: C.magenta, fill: true },
        ]
      : [{ label: "lectura + escritura", color: C.cyan, fill: true }],
  );
</script>

{#if bw.status === "ok"}
  <div class="live">
    <div class="nums">
      <div class="now">
        <span>ahora</span>
        <b>{fmtGBs(bw.total ?? 0)}</b>
        {#if split}<small><i style:color={C.cyan}>▼ {fmtGBs(bw.read ?? 0)}</i> <i style:color={C.magenta}>▲ {fmtGBs(bw.write ?? 0)}</i></small>{/if}
      </div>
      <div><span>media</span><b class="sm">{fmtGBs(win.avg)}</b></div>
      <div><span>pico</span><b class="sm">{fmtGBs(win.max)}</b></div>
      <div>
        <span>del máximo</span>
        <b class="sm">{peak ? `${Math.min(100, ((bw.total ?? 0) / peak) * 100).toFixed(0)} %` : "—"}</b>
        {#if peak}<small>test: {fmtGBs(peak)}</small>{:else}<small>ejecuta el test</small>{/if}
      </div>
    </div>
    {#key split}
      <TimeChart
        height={190}
        stacked={split}
        yFloor={1e9}
        yFmt={(v) => fmtGBs(v, 0)}
        {series}
        data={() => (split ? [metrics.view(metrics.h.memBw.read), metrics.view(metrics.h.memBw.write)] : [metrics.view(metrics.h.memBw.total)])}
      />
    {/key}
    <p class="hint">
      {bw.source === "amd_df"
        ? "Contadores del Data Fabric de AMD: tráfico real entre el controlador de memoria y la DRAM (lectura y escritura juntas; este procesador no las separa)."
        : "Contadores del controlador de memoria de Intel (uncore_imc)."}
      {#if platform.desktop && bw.enabled}
        <button class="link" disabled={busy} onclick={() => access(false)}>Desactivar</button>
      {/if}
    </p>
  </div>
{:else}
  <div class="off">
    <p>
      {WHY[bw.status] ?? bw.message ?? bw.status}
      {#if bw.message && bw.status === "error"}<br /><small>{bw.message}</small>{/if}
    </p>
    {#if fixable && platform.desktop}
      <button class="primary" disabled={busy} onclick={() => access(true)}>Activar (pide contraseña)</button>
      <p class="hint">
        Se hará como administrador, una sola vez: cargar el módulo <code>amd_uncore</code> (y dejarlo para cada arranque en
        <code>/etc/modules-load.d/neontop-membw.conf</code>) y dar a <code>/usr/lib/neontop/neontop-membw</code> el permiso
        <code>CAP_PERFMON</code>, que solo permite leer contadores de rendimiento. NeonTop no se ejecuta como root.
      </p>
    {:else if fixable}
      <p class="hint">Actívalo desde la aplicación del equipo (Memoria → Tráfico de RAM en vivo).</p>
    {/if}
  </div>
{/if}
{#if msg}<p class="msg">{msg}</p>{/if}

<style>
  .nums {
    display: grid;
    grid-template-columns: 1.6fr repeat(3, 1fr);
    gap: 12px;
    margin-bottom: 8px;
  }
  .nums > div {
    display: grid;
    align-content: start;
    gap: 2px;
    min-width: 0;
  }
  .nums span {
    font-size: 10px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.12em;
  }
  .nums b {
    font-family: var(--display);
    font-size: 26px;
    color: var(--cyan);
    text-shadow: 0 0 10px color-mix(in srgb, var(--cyan) calc(var(--glow-a) * 0.6), transparent);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .nums b.sm {
    font-size: 17px;
    color: var(--text);
    text-shadow: none;
  }
  .nums small {
    font-size: 10.5px;
    color: var(--muted);
  }
  .nums i {
    font-style: normal;
  }
  .off p {
    margin: 0 0 10px;
    font-size: 12px;
    line-height: 1.6;
  }
  .hint {
    font-size: 11px;
    color: var(--muted);
    line-height: 1.6;
    margin: 8px 0 0;
  }
  code {
    color: var(--cyan);
  }
  button.primary {
    background: var(--cyan);
    color: var(--bg);
    border: none;
    font: inherit;
    font-weight: 700;
    font-size: 12px;
    padding: 8px 16px;
    cursor: pointer;
  }
  .link {
    background: transparent;
    color: var(--cyan);
    border: 1px solid color-mix(in srgb, var(--cyan) 40%, transparent);
    font: inherit;
    font-size: 10px;
    padding: 1px 8px;
    margin-left: 6px;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: wait;
  }
  .msg {
    color: var(--amber);
    font-size: 11px;
    margin: 8px 0 0;
  }
  @media (max-width: 700px) {
    .nums {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
    .nums .now {
      grid-column: 1 / -1;
    }
  }
</style>
