<script lang="ts" module>
  import type { ProcInfo } from "$lib/types";

  export interface Row {
    p: ProcInfo;
    depth: number;
    hasChildren: boolean;
    collapsed: boolean;
    /** Prefijo de guías del árbol (│ ├ └). */
    guide: string;
  }

  export type SortKey = "pid" | "name" | "user" | "state" | "cpu" | "mem" | "threads" | "nice" | "read" | "write" | "cpuTime" | "age";
</script>

<script lang="ts">
  import Sparkline from "$lib/charts/Sparkline.svelte";
  import { procs, STATES } from "$lib/procs.svelte";
  import { C, heatColor } from "$lib/theme";
  import { fmtBytes, fmtDuration, fmtRate } from "$lib/format";

  interface Props {
    rows: Row[];
    sortKey: SortKey;
    sortDesc: boolean;
    selected: number | null;
    memTotal: number;
    /** Divisor del % de CPU (1 = % de un núcleo; nº de hilos = % del total). */
    cpuDiv: number;
    uptime: number;
    height?: number;
    onsort: (k: SortKey) => void;
    onselect: (pid: number) => void;
    ontoggle: (pid: number) => void;
  }
  let { rows, sortKey, sortDesc, selected, memTotal, cpuDiv, uptime, height = 560, onsort, onselect, ontoggle }: Props = $props();

  const ROW_H = 24;
  const OVERSCAN = 8;

  const COLS: { key: SortKey | null; label: string; w: string; align?: "right" | "center" }[] = [
    { key: "name", label: "Proceso", w: "minmax(180px, 2.2fr)" },
    { key: "pid", label: "PID", w: "64px", align: "right" },
    { key: "user", label: "Usuario", w: "86px" },
    { key: "state", label: "E", w: "26px", align: "center" },
    { key: "cpu", label: "CPU", w: "120px" },
    { key: "mem", label: "Memoria", w: "130px" },
    { key: null, label: "Historial CPU", w: "minmax(80px, 1fr)" },
    { key: "threads", label: "Hilos", w: "48px", align: "right" },
    { key: "nice", label: "NI", w: "34px", align: "right" },
    { key: "read", label: "Lect/s", w: "78px", align: "right" },
    { key: "write", label: "Escr/s", w: "78px", align: "right" },
    { key: "cpuTime", label: "T. CPU", w: "76px", align: "right" },
    { key: "age", label: "Edad", w: "86px", align: "right" },
  ];
  const template = COLS.map((c) => c.w).join(" ");
  /** Pantallas estrechas (teléfono): solo nombre, PID, CPU y memoria (columnas 1, 2, 5 y 6). */
  const compactTemplate = "minmax(70px, 1fr) 44px 74px 78px";
  let tableW = $state(1200);
  let compact = $derived(tableW < 820);

  let scrollTop = $state(0);
  let viewport: HTMLDivElement;

  let first = $derived(Math.max(0, Math.floor(scrollTop / ROW_H) - OVERSCAN));
  let visible = $derived(rows.slice(first, first + Math.ceil(height / ROW_H) + OVERSCAN * 2));

  /** Mantiene visible la fila seleccionada al moverse con el teclado. */
  export function reveal(pid: number) {
    const i = rows.findIndex((r) => r.p.pid === pid);
    if (i < 0 || !viewport) return;
    const top = i * ROW_H;
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (top + ROW_H > viewport.scrollTop + height - ROW_H) viewport.scrollTop = top - height + ROW_H * 2;
  }

  const shortAge = (s: number) => (s < 3600 ? `${Math.floor(s / 60)}m ${Math.floor(s % 60)}s` : s < 86400 ? `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m` : `${Math.floor(s / 86400)}d ${Math.floor((s % 86400) / 3600)}h`);
  const stateColor = (s: string) => {
    const c = STATES[s]?.color ?? "muted";
    return c === "muted" ? C.muted : C[c];
  };
</script>

<div class="table" class:compact style:--cols={compact ? compactTemplate : template} bind:clientWidth={tableW}>
  <div class="head">
    {#each COLS as c}
      <button
        class:sortable={c.key}
        class:active={c.key === sortKey}
        style:justify-content={c.align === "right" ? "flex-end" : c.align === "center" ? "center" : "flex-start"}
        disabled={!c.key}
        onclick={() => c.key && onsort(c.key)}
      >
        {c.label}
        {#if c.key === sortKey}<span class="arrow">{sortDesc ? "▼" : "▲"}</span>{/if}
      </button>
    {/each}
  </div>

  <div class="viewport" bind:this={viewport} style:height="{height}px" onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}>
    <div class="spacer" style:height="{rows.length * ROW_H}px">
      {#each visible as r, i (r.p.pid)}
        {@const p = r.p}
        {@const cpu = p.cpu / cpuDiv}
        {@const memPct = (p.mem / memTotal) * 100}
        {@const h = procs.hist.get(p.pid)}
        <div
          class="row"
          class:sel={selected === p.pid}
          class:kernel={p.kernel}
          style:top="{(first + i) * ROW_H}px"
          onclick={() => onselect(p.pid)}
          role="row"
          tabindex="-1"
          aria-selected={selected === p.pid}
          onkeydown={() => {}}
        >
          <div class="name" title={p.cmd}>
            {#if r.guide}<span class="guide">{r.guide}</span>{/if}
            {#if r.hasChildren}
              <button class="twisty" onclick={(e) => (e.stopPropagation(), ontoggle(p.pid))}>{r.collapsed ? "▸" : "▾"}</button>
            {/if}
            <span class="pname">{p.name}</span>
            <span class="cmd">{p.cmd.startsWith("[") ? "" : p.cmd}</span>
          </div>
          <div class="r num">{p.pid}</div>
          <div class="user" class:root={p.uid === 0}>{p.user}</div>
          <div class="c" style:color={stateColor(p.state)}>{p.state}</div>
          <div class="bar">
            <i style:width="{Math.min(100, cpu)}%" style:background={heatColor(cpu)}></i>
            <span>{cpu.toFixed(1)}%</span>
          </div>
          <div class="bar">
            <i style:width="{Math.min(100, memPct * 4)}%" style:background={C.magenta}></i>
            <span>{fmtBytes(p.mem)}</span>
          </div>
          <div class="spark">
            {#if h}<Sparkline values={h.cpu} color={heatColor(cpu)} height={18} points={60} max={Math.max(10, ...h.cpu)} version={procs.tick} />{/if}
          </div>
          <div class="r num">{p.threads}</div>
          <div class="r num" class:hi={p.nice < 0} class:lo={p.nice > 0}>{p.nice}</div>
          <div class="r num io">{p.readRate == null ? "—" : p.readRate > 0 ? fmtRate(p.readRate) : "0"}</div>
          <div class="r num io">{p.writeRate == null ? "—" : p.writeRate > 0 ? fmtRate(p.writeRate) : "0"}</div>
          <div class="r num">{fmtDuration(p.cpuTime)}</div>
          <div class="r num dim">{shortAge(Math.max(0, uptime - p.start))}</div>
        </div>
      {/each}
    </div>
  </div>
  <div class="foot">{rows.length} procesos visibles</div>
</div>

<style>
  /* modo compacto: solo las columnas 1, 2, 5 y 6 */
  .compact .head > :not(:nth-child(1), :nth-child(2), :nth-child(5), :nth-child(6)),
  .compact .row > :not(:nth-child(1), :nth-child(2), :nth-child(5), :nth-child(6)) {
    display: none;
  }
  .compact .row .cmd {
    display: none;
  }
  .compact .head,
  .compact .row {
    gap: 5px;
    padding: 0 4px;
  }
  .table {
    font-size: 11.5px;
    min-width: 0;
  }
  .head,
  .row {
    display: grid;
    grid-template-columns: var(--cols);
    gap: 8px;
    align-items: center;
    padding: 0 8px;
  }
  .head {
    border-bottom: 1px solid color-mix(in srgb, var(--cyan) 30%, transparent);
    padding-bottom: 5px;
    margin-bottom: 2px;
  }
  .head button {
    display: flex;
    align-items: center;
    gap: 4px;
    background: none;
    border: none;
    padding: 2px 0;
    font: inherit;
    font-size: 9.5px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--muted);
    white-space: nowrap;
  }
  .head button.sortable {
    cursor: pointer;
  }
  .head button.sortable:hover,
  .head button.active {
    color: var(--cyan);
  }
  .arrow {
    font-size: 8px;
  }
  .viewport {
    overflow-y: auto;
    position: relative;
  }
  .spacer {
    position: relative;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    height: 24px;
    cursor: pointer;
    border-left: 2px solid transparent;
  }
  .row:hover {
    background: color-mix(in srgb, var(--cyan) 6%, transparent);
  }
  .row.sel {
    background: linear-gradient(90deg, color-mix(in srgb, var(--cyan) 20%, transparent), color-mix(in srgb, var(--cyan) 4%, transparent));
    border-left-color: var(--cyan);
  }
  .row.kernel .pname {
    color: var(--muted);
  }
  .name {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
  }
  .guide {
    color: color-mix(in srgb, var(--cyan) 35%, transparent);
    white-space: pre;
  }
  .twisty {
    background: none;
    border: none;
    color: var(--cyan);
    padding: 0;
    width: 12px;
    cursor: pointer;
    font-size: 10px;
  }
  .pname {
    color: var(--text);
    font-weight: 500;
  }
  .cmd {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 10px;
  }
  .num {
    font-variant-numeric: tabular-nums;
  }
  .r {
    text-align: right;
  }
  .c {
    text-align: center;
    font-weight: 700;
  }
  .user {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--teal);
  }
  .user.root {
    color: var(--red);
  }
  .bar {
    position: relative;
    height: 16px;
    background: color-mix(in srgb, var(--cyan) 5%, transparent);
  }
  .bar i {
    position: absolute;
    inset: 0 auto 0 0;
    opacity: 0.55;
  }
  .bar span {
    position: relative;
    display: block;
    text-align: right;
    padding-right: 5px;
    line-height: 16px;
    font-variant-numeric: tabular-nums;
  }
  .spark {
    min-width: 0;
  }
  .hi {
    color: var(--magenta);
  }
  .lo {
    color: var(--teal);
  }
  .io {
    color: var(--amber);
  }
  .dim {
    color: var(--muted);
  }
  .foot {
    font-size: 10px;
    color: var(--muted);
    padding: 6px 8px 0;
  }
</style>
