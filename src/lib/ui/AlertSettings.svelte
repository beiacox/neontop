<script lang="ts">
  import { alerts, MULTI, type Channel } from "$lib/alerts.svelte";
  import { metrics } from "$lib/store.svelte";

  const CHANNELS: { v: Channel; label: string }[] = [
    { v: "both", label: "Escritorio y app" },
    { v: "app", label: "Solo en la app" },
    { v: "system", label: "Solo escritorio" },
  ];
  const REPEATS = [
    { v: 0, label: "una vez" },
    { v: 5, label: "cada 5 min" },
    { v: 15, label: "cada 15 min" },
    { v: 30, label: "cada 30 min" },
    { v: 60, label: "cada hora" },
  ];

  // se refresca cada pocos segundos para mostrar los silencios que caducan
  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 5000);
    return () => clearInterval(t);
  });
  const hhmm = (ts: number) => new Date(ts).toLocaleTimeString("es", { hour: "2-digit", minute: "2-digit", hour12: false });
  /** Mañana a las 8:00. */
  const tomorrow = () => {
    const d = new Date();
    d.setDate(d.getDate() + 1);
    d.setHours(8, 0, 0, 0);
    return d.getTime();
  };
  let open = $state<string | null>(null);
</script>

<div class="global">
  <label class="toggle">
    <input type="checkbox" checked={alerts.opts.desktop} onchange={(e) => alerts.setOpts({ desktop: e.currentTarget.checked })} />
    <span>Notificaciones del escritorio</span>
  </label>
  <label class="toggle">
    <input type="checkbox" checked={alerts.opts.toasts} onchange={(e) => alerts.setOpts({ toasts: e.currentTarget.checked })} />
    <span>Avisos dentro de NeonTop</span>
  </label>
  <div class="mute">
    {#if alerts.opts.muteAll > now}
      <span class="muted">Todo silenciado hasta las {hhmm(alerts.opts.muteAll)}</span>
      <button onclick={() => alerts.muteAll(0)}>Reactivar</button>
    {:else}
      <span>Silenciar todo:</span>
      <button onclick={() => alerts.muteAll(Date.now() + 3600_000)}>1 hora</button>
      <button onclick={() => alerts.muteAll(tomorrow())}>hasta mañana</button>
    {/if}
  </div>
</div>

<div class="rules">
  {#each alerts.rules as r (r.id)}
    {@const muted = r.mutedUntil > now}
    <div class="rule" class:off={!r.enabled}>
      <div class="row">
        <label class="toggle">
          <input type="checkbox" checked={r.enabled} onchange={(e) => alerts.update(r.id, { enabled: e.currentTarget.checked })} />
          <span>{r.label}</span>
        </label>
        <button class="more" onclick={() => (open = open === r.id ? null : r.id)} aria-expanded={open === r.id}>{open === r.id ? "▴" : "▾"}</button>
      </div>
      <span class="when">
        {#if !r.flag}
          {r.below ? "<" : ">"}
          <input type="number" value={r.threshold} min="0" max="200" onchange={(e) => alerts.update(r.id, { threshold: +e.currentTarget.value })} aria-label="Umbral" />
          {r.unit} durante
          <input type="number" value={r.sustain} min="0" max="3600" onchange={(e) => alerts.update(r.id, { sustain: +e.currentTarget.value })} aria-label="Segundos" />
          s ·
        {:else}
          al ocurrir ·
        {/if}
        {REPEATS.find((x) => x.v === r.repeat)?.label ?? `cada ${r.repeat} min`}
        {#if muted}<b class="muted">· silenciada hasta {hhmm(r.mutedUntil)}</b>{/if}
        {#if r.ignore.length}<b class="muted">· {r.ignore.length} ignorado{r.ignore.length > 1 ? "s" : ""}</b>{/if}
      </span>

      {#if open === r.id}
        <div class="detail">
          <label>
            Avisar en
            <select value={r.channel} onchange={(e) => alerts.update(r.id, { channel: e.currentTarget.value as Channel })}>
              {#each CHANNELS as c}<option value={c.v}>{c.label}</option>{/each}
            </select>
          </label>
          <label>
            Repetir
            <select value={r.repeat} onchange={(e) => alerts.update(r.id, { repeat: +e.currentTarget.value })}>
              {#each REPEATS as x}<option value={x.v}>{x.v ? `${x.label} mientras siga` : "una vez hasta que se resuelva"}</option>{/each}
            </select>
          </label>
          <div class="mute">
            {#if muted}
              <button onclick={() => alerts.mute(r.id, 0)}>Reactivar</button>
            {:else}
              <span>Silenciar:</span>
              <button onclick={() => alerts.mute(r.id, 60)}>1 h</button>
              <button onclick={() => alerts.mute(r.id, 8 * 60)}>8 h</button>
              <button onclick={() => alerts.mute(r.id, 7 * 24 * 60)}>1 semana</button>
            {/if}
          </div>
          {#if MULTI[r.id]}
            {@const items = alerts.items(r.id, metrics.snap)}
            {@const keys = [...new Set([...items.map((i) => i.key), ...r.ignore])]}
            <div class="items">
              <span class="cap">Vigilar estos {MULTI[r.id]}:</span>
              {#each keys as k (k)}
                {@const it = items.find((i) => i.key === k)}
                <label class="item" class:over={it && it.value >= r.threshold}>
                  <input type="checkbox" checked={!r.ignore.includes(k)} onchange={(e) => (e.currentTarget.checked ? alerts.unignore(r.id, k) : alerts.ignore(r.id, k))} />
                  <code>{k}</code>
                  <span>{it ? `${it.value.toFixed(r.unit === "%" ? 0 : 1)}${r.unit}` : "no conectado"}</span>
                </label>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/each}
</div>
<p class="hint">
  Pulsa ▾ en una regla para elegir dónde avisa, cada cuánto se repite, silenciarla o (en discos) cuáles vigilar. Los avisos emergentes también tienen
  botones para silenciar o ignorar.
</p>

{#if alerts.log.length}
  <div class="log">
    {#each alerts.log.slice(0, 5) as f (f.id)}
      <div><b>{hhmm(f.ts)}</b> {f.title} <span>{f.body}</span></div>
    {/each}
  </div>
{/if}
<button class="link" onclick={() => alerts.reset()}>Restaurar valores por defecto</button>

<style>
  .global {
    display: grid;
    gap: 6px;
    margin-bottom: 12px;
  }
  .mute {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--muted);
  }
  .muted {
    color: var(--amber);
    font-weight: normal;
  }
  button {
    font: inherit;
    font-size: 11px;
    padding: 2px 8px;
    background: transparent;
    color: var(--cyan);
    border: 1px solid color-mix(in srgb, var(--cyan) 35%, transparent);
    cursor: pointer;
  }
  .rules {
    display: grid;
    gap: 8px;
  }
  .rule {
    display: grid;
    gap: 4px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--cyan) 14%, transparent);
    border-radius: var(--radius-sm);
  }
  .rule.off .when,
  .rule.off .detail {
    opacity: 0.45;
  }
  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .more {
    border: none;
    font-size: 13px;
    padding: 0 6px;
  }
  .when {
    font-size: 11px;
    color: var(--muted);
  }
  .when input {
    width: 54px;
    font: inherit;
    font-size: 11px;
    padding: 1px 4px;
    background: color-mix(in srgb, var(--cyan) 6%, transparent);
    border: 1px solid color-mix(in srgb, var(--cyan) 25%, transparent);
    color: var(--text);
  }
  .detail {
    display: grid;
    gap: 8px;
    margin-top: 6px;
    padding-top: 8px;
    border-top: 1px dashed color-mix(in srgb, var(--cyan) 18%, transparent);
    font-size: 11px;
  }
  .detail label {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    color: var(--muted);
  }
  select {
    font: inherit;
    font-size: 11px;
    padding: 2px 6px;
    background: var(--bg);
    color: var(--text);
    border: 1px solid color-mix(in srgb, var(--cyan) 30%, transparent);
  }
  .items {
    display: grid;
    gap: 3px;
  }
  .cap {
    color: var(--muted);
  }
  .item {
    display: grid !important;
    grid-template-columns: auto minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    cursor: pointer;
  }
  .item code {
    color: var(--cyan);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item.over span {
    color: var(--red);
  }
  .log {
    display: grid;
    gap: 3px;
    margin-top: 10px;
    font-size: 10.5px;
  }
  .log b {
    color: var(--amber);
    font-weight: normal;
  }
  .log span {
    color: var(--muted);
  }
  .link {
    margin-top: 10px;
  }
</style>
