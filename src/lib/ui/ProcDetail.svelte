<script lang="ts">
  import Sparkline from "$lib/charts/Sparkline.svelte";
  import { procs, STATES } from "$lib/procs.svelte";
  import { C, heatColor } from "$lib/theme";
  import { fmtBytes, fmtDuration, fmtRate } from "$lib/format";
  import type { ProcInfo } from "$lib/types";

  interface Props {
    p: ProcInfo;
    /** false si el proceso ya terminó (se muestra su último estado). */
    alive: boolean;
    cpuDiv: number;
    uptime: number;
    parent?: ProcInfo;
    childCount: number;
    onselect: (pid: number) => void;
  }
  let { p, alive, cpuDiv, uptime, parent, childCount, onselect }: Props = $props();

  type Sig = "TERM" | "KILL" | "INT" | "HUP" | "STOP" | "CONT";
  let confirming = $state<Sig | null>(null);
  let msg = $state<{ ok: boolean; text: string } | null>(null);
  let nice = $state(0);
  let lastPid = 0;

  $effect(() => {
    // al cambiar de proceso se reinician acciones pendientes y el valor de nice
    if (p.pid !== lastPid) {
      lastPid = p.pid;
      nice = p.nice;
      confirming = null;
      msg = null;
    }
  });

  let h = $derived((procs.tick, procs.hist.get(p.pid)));
  let cpu = $derived(p.cpu / cpuDiv);

  const SIGNALS: { sig: Sig; label: string; danger?: boolean; hint: string }[] = [
    { sig: "TERM", label: "Terminar", danger: true, hint: "SIGTERM: pide al proceso que cierre limpiamente" },
    { sig: "KILL", label: "Matar", danger: true, hint: "SIGKILL: lo detiene al instante, sin guardar nada" },
    { sig: "STOP", label: "Congelar", hint: "SIGSTOP: lo suspende hasta reanudarlo" },
    { sig: "CONT", label: "Reanudar", hint: "SIGCONT: reanuda un proceso congelado" },
    { sig: "INT", label: "Interrumpir", hint: "SIGINT: como pulsar Ctrl+C" },
    { sig: "HUP", label: "Recargar", hint: "SIGHUP: muchos demonios recargan su configuración" },
  ];

  async function send(sig: Sig) {
    // las señales que terminan el proceso piden confirmación
    if ((sig === "TERM" || sig === "KILL") && confirming !== sig) {
      confirming = sig;
      return;
    }
    confirming = null;
    try {
      await procs.signal(p.pid, sig);
      msg = { ok: true, text: `SIG${sig} enviada a ${p.name} (${p.pid})` };
    } catch (e) {
      msg = { ok: false, text: String(e) };
    }
  }

  async function applyNice() {
    try {
      await procs.renice(p.pid, nice);
      msg = { ok: true, text: `Prioridad de ${p.name} → nice ${nice}` };
    } catch (e) {
      msg = { ok: false, text: String(e) };
    }
  }
</script>

<div class="detail">
  <div class="title">
    <h3 style:color={heatColor(cpu)}>{p.name}</h3>
    <span class="pid">PID {p.pid}</span>
    {#if !alive}<span class="dead">TERMINADO</span>{/if}
  </div>
  <code class="cmd">{p.cmd}</code>

  <div class="metrics">
    <div>
      <div class="lbl">CPU <b style:color={heatColor(cpu)}>{cpu.toFixed(1)}%</b></div>
      {#if h}<Sparkline values={h.cpu} color={heatColor(cpu)} height={42} points={120} max={Math.max(10, ...h.cpu)} version={procs.tick} />{/if}
    </div>
    <div>
      <div class="lbl">Memoria <b style:color={C.magenta}>{fmtBytes(p.mem)}</b></div>
      {#if h}<Sparkline values={h.mem} color={C.magenta} height={42} points={120} version={procs.tick} />{/if}
    </div>
    <div>
      <div class="lbl">E/S disco <b style:color={C.amber}>{p.readRate == null ? "sin permiso" : fmtRate((p.readRate ?? 0) + (p.writeRate ?? 0))}</b></div>
      {#if h && p.readRate != null}<Sparkline values={h.io} color={C.amber} height={42} points={120} version={procs.tick} />{/if}
    </div>
  </div>

  <dl>
    <dt>Estado</dt><dd>{p.state} · {STATES[p.state]?.label ?? "desconocido"}</dd>
    <dt>Usuario</dt><dd>{p.user} (uid {p.uid})</dd>
    <dt>Padre</dt>
    <dd>
      {#if parent}<button class="link" onclick={() => onselect(parent.pid)}>{parent.name} ({parent.pid})</button>{:else}{p.ppid}{/if}
      {#if childCount}· {childCount} hijos{/if}
    </dd>
    <dt>Hilos</dt><dd>{p.threads}</dd>
    <dt>Prioridad</dt><dd>{p.priority} · nice {p.nice}</dd>
    <dt>Virtual</dt><dd>{fmtBytes(p.virt)}</dd>
    <dt>T. CPU</dt><dd>{fmtDuration(p.cpuTime)}</dd>
    <dt>Iniciado</dt><dd>hace {fmtDuration(Math.max(0, uptime - p.start))}</dd>
  </dl>

  {#if alive}
    <div class="section">Señales</div>
    <div class="signals">
      {#each SIGNALS as s}
        <button class:danger={s.danger} class:confirm={confirming === s.sig} title={s.hint} onclick={() => send(s.sig)}>
          {confirming === s.sig ? `¿${s.label}? Confirmar` : s.label}
        </button>
      {/each}
    </div>

    <div class="section">Prioridad (nice)</div>
    <div class="nice">
      <span class="hi">-20</span>
      <input type="range" min="-20" max="19" bind:value={nice} aria-label="Nice" />
      <span class="lo">19</span>
      <b>{nice}</b>
      <button onclick={applyNice} disabled={nice === p.nice}>Aplicar</button>
    </div>
    <p class="hint">Valores más bajos = más prioridad. Bajar el nice requiere privilegios de root.</p>
  {/if}

  {#if msg}<div class="msg" class:err={!msg.ok}>{msg.text}</div>{/if}
</div>

<style>
  .detail {
    font-size: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  h3 {
    margin: 0;
    font-family: var(--display);
    font-size: 16px;
    word-break: break-all;
    text-shadow: 0 0 10px currentColor;
  }
  .pid {
    color: var(--muted);
    font-size: 11px;
    white-space: nowrap;
  }
  .dead {
    color: var(--red);
    font-size: 10px;
    letter-spacing: 0.15em;
  }
  .cmd {
    display: block;
    font-size: 10.5px;
    color: var(--muted);
    background: color-mix(in srgb, var(--cyan) 5%, transparent);
    border-left: 2px solid color-mix(in srgb, var(--cyan) 40%, transparent);
    padding: 6px 8px;
    word-break: break-all;
    max-height: 70px;
    overflow-y: auto;
  }
  .metrics {
    display: grid;
    gap: 6px;
  }
  .lbl {
    display: flex;
    justify-content: space-between;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--muted);
  }
  .lbl b {
    font-size: 12px;
    letter-spacing: 0;
    text-transform: none;
  }
  dl {
    display: grid;
    grid-template-columns: 74px 1fr;
    gap: 4px 10px;
    margin: 4px 0;
  }
  dt {
    color: var(--muted);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    align-self: center;
  }
  dd {
    margin: 0;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--cyan);
    font: inherit;
    cursor: pointer;
    text-decoration: underline dotted;
  }
  .section {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.15em;
    color: var(--muted);
    border-top: 1px solid color-mix(in srgb, var(--cyan) 12%, transparent);
    padding-top: 8px;
  }
  .signals {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 5px;
  }
  .signals button,
  .nice button {
    background: none;
    border: 1px solid color-mix(in srgb, var(--cyan) 30%, transparent);
    color: var(--text);
    font: inherit;
    font-size: 11px;
    padding: 5px;
    cursor: pointer;
  }
  .signals button:hover,
  .nice button:hover:not(:disabled) {
    background: color-mix(in srgb, var(--cyan) 12%, transparent);
  }
  .signals button.danger {
    border-color: color-mix(in srgb, var(--red) 50%, transparent);
    color: var(--red);
  }
  .signals button.confirm {
    background: var(--red);
    color: var(--bg);
    box-shadow: 0 0 12px color-mix(in srgb, var(--red) var(--glow-a), transparent);
  }
  .nice {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10px;
  }
  .nice input {
    flex: 1;
    accent-color: var(--cyan);
  }
  .nice b {
    width: 26px;
    text-align: right;
    font-size: 13px;
    color: var(--cyan);
  }
  .nice button:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .hi {
    color: var(--magenta);
  }
  .lo {
    color: var(--teal);
  }
  .hint {
    font-size: 10px;
    color: var(--muted);
    margin: 0;
  }
  .msg {
    font-size: 11px;
    padding: 6px 8px;
    border: 1px solid color-mix(in srgb, var(--lime) 50%, transparent);
    color: var(--lime);
  }
  .msg.err {
    border-color: color-mix(in srgb, var(--red) 50%, transparent);
    color: var(--red);
  }
</style>
