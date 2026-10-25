<script lang="ts">
  /** Medición automática: NeonTop añade `neontop-fps` a los lanzadores de juegos (launchers.rs). */
  import { invoke } from "@tauri-apps/api/core";

  interface Launcher {
    id: string;
    name: string;
    total: number;
    applied: number;
    running: boolean;
    scope: string;
  }
  interface FpsAuto {
    enabled: boolean;
    launchers: Launcher[];
  }

  let st = $state<FpsAuto | null>(null);
  let busy = $state(false);
  let msg = $state<string | null>(null);

  async function refresh() {
    try {
      st = await invoke<FpsAuto>("fps_auto_status");
    } catch {
      /* sin backend */
    }
  }

  $effect(() => {
    refresh();
    const t = setInterval(refresh, 10_000);
    return () => clearInterval(t);
  });

  async function toggle() {
    if (!st) return;
    busy = true;
    msg = null;
    try {
      st = await invoke<FpsAuto>("set_fps_auto", { enabled: !st.enabled });
    } catch (e) {
      msg = String(e);
    }
    busy = false;
  }

  async function restartSteam() {
    busy = true;
    msg = "Cerrando Steam…";
    try {
      st = await invoke<FpsAuto>("fps_auto_restart_steam");
      msg = "Listo: Steam se está abriendo de nuevo con los cambios.";
    } catch (e) {
      msg = String(e);
    }
    busy = false;
  }

  const done = (l: Launcher) => (st?.enabled ? l.applied >= l.total : l.applied === 0);
</script>

{#if st}
  <div class="auto">
    <div class="head">
      <label class="switch">
        <input type="checkbox" checked={st.enabled} disabled={busy} onchange={toggle} />
        <span>Añadir <code>neontop-fps</code> automáticamente a los juegos</span>
      </label>
      <span class="hint">
        {st.enabled
          ? "Cada juego instalado (también los nuevos) arranca con la medición, sin tocar nada."
          : "Desactivado: NeonTop quita lo que añadió."}
      </span>
    </div>
    {#if st.launchers.length}
      <div class="rows">
        {#each st.launchers as l (l.id)}
          <div class="row">
            <b>{l.name}</b>
            <span class="scope">{l.scope}</span>
            <span class="count">{l.total > 1 ? `${l.applied}/${l.total} juegos` : l.applied ? "activo" : "sin activar"}</span>
            <span class="state">
              {#if done(l)}
                <span class="ok">● listo</span>
              {:else if l.running}
                <span class="warn">● pendiente: se aplica al cerrar {l.name}</span>
                {#if l.id === "steam"}
                  <button disabled={busy} onclick={restartSteam} title="Cierra Steam, aplica los cambios y lo vuelve a abrir">Reiniciar Steam y aplicar</button>
                {/if}
              {:else}
                <span class="warn">● aplicando…</span>
              {/if}
            </span>
          </div>
        {/each}
      </div>
    {:else}
      <p class="hint">No se ha encontrado Steam, Heroic, Lutris ni Prism Launcher.</p>
    {/if}
    {#if msg}<p class="msg">{msg}</p>{/if}
  </div>
{/if}

<style>
  .auto {
    display: grid;
    gap: 10px;
  }
  .head {
    display: grid;
    gap: 4px;
  }
  .switch {
    display: flex;
    gap: 8px;
    align-items: center;
    cursor: pointer;
    font-weight: 600;
  }
  .switch input {
    accent-color: var(--lime);
    width: 16px;
    height: 16px;
  }
  .hint,
  .scope,
  .count {
    font-size: 11px;
    color: var(--muted);
  }
  .rows {
    display: grid;
    gap: 6px;
  }
  .row {
    display: grid;
    grid-template-columns: 120px minmax(0, 1fr) 90px minmax(0, 1.4fr);
    gap: 4px 12px;
    align-items: baseline;
    font-size: 12px;
  }
  .row b {
    color: var(--text);
  }
  .state {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 10px;
    align-items: baseline;
  }
  @media (max-width: 700px) {
    .row {
      grid-template-columns: auto 1fr;
    }
    .scope {
      text-align: right;
    }
    .state {
      grid-column: span 2;
    }
  }
  .ok {
    color: var(--lime);
  }
  .warn {
    color: var(--amber);
  }
  button {
    font: inherit;
    font-size: 11px;
    padding: 3px 10px;
    background: transparent;
    color: var(--cyan);
    border: 1px solid var(--cyan);
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .msg {
    margin: 0;
    font-size: 11px;
    color: var(--cyan);
  }
  code {
    color: var(--lime);
  }
</style>
