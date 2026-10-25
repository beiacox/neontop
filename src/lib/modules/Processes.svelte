<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import EChart from "$lib/charts/EChart.svelte";
  import { tooltipBase } from "$lib/charts/echarts";
  import Panel from "$lib/ui/Panel.svelte";
  import Stat from "$lib/ui/Stat.svelte";
  import ProcTable, { type Row, type SortKey } from "$lib/ui/ProcTable.svelte";
  import ProcDetail from "$lib/ui/ProcDetail.svelte";
  import { metrics } from "$lib/store.svelte";
  import { procs, STATES } from "$lib/procs.svelte";
  import { C, MONO, heatColor, rgba } from "$lib/theme";
  import { fmtBytes, fmtPct } from "$lib/format";
  import type { ProcInfo } from "$lib/types";

  $effect(() => {
    procs.want("processes", true);
    return () => procs.want("processes", false);
  });

  let s = $derived(metrics.snap!);
  let ncores = $derived(metrics.info?.logicalCores ?? 1);

  let filter = $state("");
  let tree = $state(false);
  let showKernel = $state(false);
  /** % de un núcleo (como top) o % del total de la CPU (como el Monitor del sistema). */
  let perCore = $state(true);
  let cpuDiv = $derived(perCore ? 1 : ncores);
  let sortKey = $state<SortKey>("cpu");
  let sortDesc = $state(true);
  let selected = $state<number | null>(null);
  let lastSeen = $state.raw<ProcInfo | null>(null);
  const collapsed = new SvelteSet<number>();

  let filterInput: HTMLInputElement;
  let table = $state<ReturnType<typeof ProcTable>>();

  let all = $derived(procs.list);
  let list = $derived(showKernel ? all : all.filter((p) => !p.kernel));
  let byPid = $derived(new Map(all.map((p) => [p.pid, p])));

  function value(p: ProcInfo, k: SortKey): number | string {
    switch (k) {
      case "pid": return p.pid;
      case "name": return p.name.toLowerCase();
      case "user": return p.user;
      case "state": return p.state;
      case "cpu": return p.cpu;
      case "mem": return p.mem;
      case "threads": return p.threads;
      case "nice": return p.nice;
      case "read": return p.readRate ?? -1;
      case "write": return p.writeRate ?? -1;
      case "cpuTime": return p.cpuTime;
      // más antiguo = empezó antes
      case "age": return -p.start;
    }
  }

  function cmp(a: ProcInfo, b: ProcInfo) {
    const va = value(a, sortKey);
    const vb = value(b, sortKey);
    const r = va < vb ? -1 : va > vb ? 1 : a.pid - b.pid;
    return sortDesc ? -r : r;
  }

  function matches(p: ProcInfo, q: string) {
    return p.name.toLowerCase().includes(q) || p.cmd.toLowerCase().includes(q) || p.user.toLowerCase().includes(q) || String(p.pid) === q;
  }

  let rows = $derived.by<Row[]>(() => {
    const q = filter.trim().toLowerCase();
    if (!tree) {
      return list
        .filter((p) => !q || matches(p, q))
        .sort(cmp)
        .map((p) => ({ p, depth: 0, hasChildren: false, collapsed: false, guide: "" }));
    }

    // En árbol, un filtro muestra las coincidencias junto con sus antepasados.
    let keep: Set<number> | null = null;
    if (q) {
      keep = new Set();
      for (const p of list) {
        if (!matches(p, q)) continue;
        for (let cur: ProcInfo | undefined = p; cur && !keep.has(cur.pid); cur = byPid.get(cur.ppid)) keep.add(cur.pid);
      }
    }
    const visible = keep ? list.filter((p) => keep!.has(p.pid)) : list;
    const pids = new Set(visible.map((p) => p.pid));
    const children = new Map<number, ProcInfo[]>();
    const roots: ProcInfo[] = [];
    for (const p of visible) {
      if (pids.has(p.ppid) && p.ppid !== p.pid) {
        if (!children.has(p.ppid)) children.set(p.ppid, []);
        children.get(p.ppid)!.push(p);
      } else roots.push(p);
    }
    const out: Row[] = [];
    const walk = (p: ProcInfo, depth: number, prefix: string, last: boolean) => {
      const kids = (children.get(p.pid) ?? []).sort(cmp);
      const isCollapsed = collapsed.has(p.pid) && !q;
      out.push({ p, depth, hasChildren: kids.length > 0, collapsed: isCollapsed, guide: depth === 0 ? "" : prefix + (last ? "└─" : "├─") });
      if (isCollapsed) return;
      const next = depth === 0 ? "" : prefix + (last ? "  " : "│ ");
      kids.forEach((k, i) => walk(k, depth + 1, next, i === kids.length - 1));
    };
    roots.sort(cmp).forEach((r, i) => walk(r, 0, "", i === roots.length - 1));
    return out;
  });

  let current = $derived(selected != null ? byPid.get(selected) : undefined);
  $effect(() => {
    if (current) lastSeen = current;
  });
  let shown = $derived(current ?? (lastSeen?.pid === selected ? lastSeen : null));

  function sortBy(k: SortKey) {
    if (k === sortKey) sortDesc = !sortDesc;
    else {
      sortKey = k;
      // texto ascendente por defecto; números descendente
      sortDesc = !["name", "user", "state", "pid"].includes(k);
    }
  }

  function select(pid: number) {
    selected = pid;
    // si está oculto dentro de una rama plegada o por un filtro, se hace visible
    filter = rows.some((r) => r.p.pid === pid) ? filter : "";
    for (let cur = byPid.get(pid); cur; cur = byPid.get(cur.ppid)) {
      if (cur.pid !== pid) collapsed.delete(cur.pid);
      if (cur.ppid === cur.pid) break;
    }
    queueMicrotask(() => table?.reveal(pid));
  }

  function toggle(pid: number) {
    if (collapsed.has(pid)) collapsed.delete(pid);
    else collapsed.add(pid);
  }

  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    const typing = target.tagName === "INPUT" && (target as HTMLInputElement).type !== "checkbox";
    if (e.key === "/" && !typing) {
      e.preventDefault();
      filterInput.focus();
      return;
    }
    if (e.key === "Escape") {
      if (typing) filterInput.blur();
      else selected = null;
      return;
    }
    if (typing || !rows.length) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const i = rows.findIndex((r) => r.p.pid === selected);
      const n = e.key === "ArrowDown" ? Math.min(rows.length - 1, i + 1) : Math.max(0, i - 1);
      selected = rows[n].p.pid;
      table?.reveal(selected);
    } else if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && tree && selected != null) {
      if (e.key === "ArrowLeft") collapsed.add(selected);
      else collapsed.delete(selected);
    }
  }

  // ---------- estadísticas ----------
  let stats = $derived.by(() => {
    const byState: Record<string, number> = {};
    let threads = 0;
    let cpu = 0;
    const users = new Set<string>();
    for (const p of all) {
      byState[p.state] = (byState[p.state] ?? 0) + 1;
      threads += p.threads;
      cpu += p.cpu;
      if (!p.kernel) users.add(p.user);
    }
    return { byState, threads, cpu: cpu / ncores, users: users.size, kernel: all.filter((p) => p.kernel).length };
  });

  const PALETTE = () => [C.cyan, C.magenta, C.lime, C.amber, C.violet, C.teal, C.blue, C.orange];
  function userColor(user: string) {
    let h = 0;
    for (const ch of user) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
    return user === "root" ? C.red : PALETTE()[h % 8];
  }

  // ---------- carrera de barras: top CPU ----------
  let race = $derived.by(() => {
    const top = list.toSorted((a, b) => b.cpu - a.cpu).slice(0, 10);
    return {
      tooltip: { ...tooltipBase, formatter: (p: any) => `${p.name}<br><b>${(p.value / cpuDiv).toFixed(1)}%</b>` },
      grid: { left: 150, right: 60, top: 6, bottom: 6 },
      xAxis: { type: "value", max: (v: any) => Math.max(10, Math.ceil(v.max / 10) * 10), show: false },
      yAxis: {
        type: "category",
        inverse: true,
        data: top.map((p) => `${p.name} · ${p.pid}`),
        axisLabel: { color: C.text, fontFamily: MONO, fontSize: 10, width: 140, overflow: "truncate" },
        axisLine: { show: false },
        axisTick: { show: false },
      },
      series: [
        {
          // la lista ya viene ordenada; sin realtimeSort ni animaciones, porque al reordenar
          // mientras cambia el conjunto de procesos las etiquetas y las barras se desincronizaban
          type: "bar",
          barWidth: "62%",
          showBackground: true,
          backgroundStyle: { color: rgba(C.cyan, 0.04) },
          label: { show: true, position: "right", color: C.text, fontFamily: MONO, fontSize: 10, formatter: (p: any) => `${(p.value / cpuDiv).toFixed(1)}%` },
          data: top.map((p) => ({ value: p.cpu, pid: p.pid, itemStyle: { color: heatColor(p.cpu / cpuDiv), shadowColor: heatColor(p.cpu / cpuDiv), shadowBlur: 10, borderRadius: [0, 3, 3, 0] } })),
        },
      ],
    };
  });

  // ---------- dispersión CPU × memoria ----------
  let scatter = $derived.by(() => {
    const pts = list.filter((p) => p.mem > 0);
    return {
      tooltip: {
        ...tooltipBase,
        formatter: (p: any) => `<b>${p.data.name}</b> (${p.data.pid})<br>CPU ${(p.data.value[0] / cpuDiv).toFixed(1)}% · ${fmtBytes(p.data.value[1])}<br>${p.data.threads} hilos · ${p.data.user}`,
      },
      grid: { left: 64, right: 16, top: 12, bottom: 30 },
      xAxis: {
        type: "value",
        name: "CPU %",
        nameLocation: "middle",
        nameGap: 20,
        nameTextStyle: { color: C.muted, fontFamily: MONO, fontSize: 10 },
        axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 10, formatter: (v: number) => (v / cpuDiv).toFixed(0) },
        splitLine: { lineStyle: { color: C.grid } },
      },
      yAxis: {
        type: "log",
        min: 1024 * 1024,
        axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 10, formatter: (v: number) => fmtBytes(v, 0) },
        splitLine: { lineStyle: { color: C.grid } },
      },
      series: [
        {
          type: "scatter",
          data: pts.map((p) => {
            const color = userColor(p.user);
            return {
              name: p.name,
              pid: p.pid,
              threads: p.threads,
              user: p.user,
              value: [p.cpu, Math.max(p.mem, 1024 * 1024)],
              symbolSize: Math.min(40, 5 + Math.sqrt(p.threads) * 3),
              itemStyle: { color: rgba(color, p.pid === selected ? 0.95 : 0.45), borderColor: p.pid === selected ? C.text : color, borderWidth: p.pid === selected ? 2 : 1 },
            };
          }),
        },
      ],
      animationDurationUpdate: 400,
    };
  });

  // ---------- treemap de memoria: usuario → aplicación → proceso ----------
  let treemap = $derived.by(() => {
    const users = new Map<string, Map<string, ProcInfo[]>>();
    for (const p of list) {
      if (p.mem <= 0) continue;
      if (!users.has(p.user)) users.set(p.user, new Map());
      const apps = users.get(p.user)!;
      if (!apps.has(p.name)) apps.set(p.name, []);
      apps.get(p.name)!.push(p);
    }
    const data = [...users].map(([user, apps]) => {
      const color = userColor(user);
      const nodes = [...apps]
        .map(([name, ps]) => ({
          name: ps.length > 1 ? `${name} ×${ps.length}` : name,
          value: ps.reduce((a, p) => a + p.mem, 0),
          children: ps.length > 1 ? ps.map((p) => ({ name: `${p.name} (${p.pid})`, value: p.mem, pid: p.pid })) : undefined,
          pid: ps.length === 1 ? ps[0].pid : undefined,
        }))
        .sort((a, b) => b.value - a.value);
      // las apps pequeñas se agrupan para que el mapa siga siendo legible
      const big = nodes.slice(0, 30);
      const rest = nodes.slice(30);
      if (rest.length) big.push({ name: `otros (${rest.length})`, value: rest.reduce((a, n) => a + n.value, 0), children: undefined, pid: undefined });
      return { name: user, value: big.reduce((a, n) => a + n.value, 0), children: big, itemStyle: { color: rgba(color, 0.55), borderColor: color } };
    });
    return {
      tooltip: { ...tooltipBase, formatter: (p: any) => `${p.treePathInfo.slice(1).map((t: any) => t.name).join(" › ")}<br><b>${fmtBytes(p.value)}</b> · ${fmtPct((p.value / s.mem.total) * 100, 1)} de la RAM` },
      series: [
        {
          type: "treemap",
          data,
          roam: false,
          nodeClick: false,
          breadcrumb: { show: false },
          width: "100%",
          height: "100%",
          top: 0,
          left: 0,
          label: { show: true, color: C.text, fontFamily: MONO, fontSize: 10, formatter: (p: any) => `${p.name}\n${fmtBytes(p.value, 0)}` },
          upperLabel: { show: true, height: 18, color: C.bg, fontFamily: MONO, fontSize: 10, fontWeight: 700, formatter: (p: any) => `${p.name} · ${fmtBytes(p.value, 0)}` },
          levels: [
            { itemStyle: { borderWidth: 2, gapWidth: 2, borderColor: C.bg } },
            { itemStyle: { borderWidth: 1, gapWidth: 1, borderColorSaturation: 0.4 }, colorSaturation: [0.3, 0.6] },
            { itemStyle: { gapWidth: 1, borderColorSaturation: 0.5 }, colorSaturation: [0.3, 0.5] },
          ],
          animationDurationUpdate: 500,
        },
      ],
    };
  });

  // ---------- sunburst de CPU: usuario → aplicación ----------
  let sunburst = $derived.by(() => {
    const users = new Map<string, Map<string, number>>();
    for (const p of list) {
      if (p.cpu < 0.1) continue;
      if (!users.has(p.user)) users.set(p.user, new Map());
      const apps = users.get(p.user)!;
      apps.set(p.name, (apps.get(p.name) ?? 0) + p.cpu);
    }
    const data = [...users].map(([user, apps]) => {
      const color = userColor(user);
      return {
        name: user,
        itemStyle: { color, shadowColor: color, shadowBlur: 8 },
        children: [...apps]
          .sort((a, b) => b[1] - a[1])
          .map(([name, v]) => ({ name, value: v / cpuDiv, itemStyle: { color: rgba(color, 0.55) } })),
      };
    });
    return {
      tooltip: { ...tooltipBase, formatter: (p: any) => `${p.name}<br><b>${p.value.toFixed(1)}%</b> CPU` },
      series: [
        {
          type: "sunburst",
          data,
          radius: ["14%", "92%"],
          sort: undefined,
          itemStyle: { borderColor: C.bg, borderWidth: 1 },
          label: { color: C.text, fontFamily: MONO, fontSize: 9, minAngle: 12 },
          levels: [{}, { r0: "14%", r: "42%", label: { rotate: 0, fontSize: 10 } }, { r0: "44%", r: "92%", label: { align: "right" } }],
          animationDurationUpdate: 500,
        },
      ],
    };
  });

  // ---------- estados ----------
  let states = $derived({
    tooltip: { ...tooltipBase, formatter: (p: any) => `${p.name}<br><b>${p.value}</b> (${p.percent.toFixed(1)}%)` },
    series: [
      {
        type: "pie",
        radius: ["52%", "76%"],
        padAngle: 2,
        itemStyle: { borderRadius: 3 },
        label: { color: C.muted, fontFamily: MONO, fontSize: 10, formatter: "{b}\n{c}" },
        labelLine: { lineStyle: { color: rgba(C.cyan, 0.3) } },
        data: Object.entries(stats.byState).map(([st, n]) => {
          const key = STATES[st]?.color ?? "muted";
          const color = key === "muted" ? C.muted : C[key];
          return { name: STATES[st]?.label ?? st, value: n, itemStyle: { color, shadowColor: color, shadowBlur: 8 } };
        }),
      },
    ],
    animationDurationUpdate: 400,
  });

  const pickPid = (params: any) => {
    const pid = params?.data?.pid;
    if (typeof pid === "number") select(pid);
  };
</script>

<svelte:window onkeydown={onKey} />

<div class="grid">
  <div class="stats" style="grid-column: span 4">
    <Stat label="Procesos" value={String(all.length)} sub={`${stats.kernel} del kernel`} color={C.cyan} />
    <Stat label="Hilos" value={String(stats.threads)} color={C.violet} />
    <Stat label="Ejecutando" value={String(stats.byState.R ?? 0)} color={C.lime} history={metrics.h.running} />
    <Stat label="Durmiendo" value={String((stats.byState.S ?? 0) + (stats.byState.I ?? 0))} color={C.blue} />
    <Stat label="Espera disco" value={String(stats.byState.D ?? 0)} color={C.amber} />
    <Stat label="Zombis" value={String(stats.byState.Z ?? 0)} color={(stats.byState.Z ?? 0) > 0 ? C.red : C.muted} />
    <Stat label="Usuarios" value={String(stats.users)} color={C.teal} />
    <Stat label="CPU procesos" value={fmtPct(stats.cpu, 1)} sub="suma / núcleos" color={heatColor(stats.cpu)} />
  </div>

  <Panel title="Procesos" span={shown ? 3 : 4} accent={C.cyan}>
    {#snippet extra()}
      <span class="keys">↑↓ mover · ←→ plegar · / buscar · Esc</span>
    {/snippet}
    <div class="toolbar">
      <input class="search" bind:this={filterInput} bind:value={filter} placeholder="Filtrar por nombre, comando, usuario o PID…" />
      <label><input type="checkbox" bind:checked={tree} /> Árbol</label>
      <label><input type="checkbox" bind:checked={showKernel} /> Hilos del kernel</label>
      <label title="Activado: 100% = un núcleo, como top. Desactivado: 100% = toda la CPU."><input type="checkbox" bind:checked={perCore} /> % por núcleo</label>
      {#if tree}
        <button onclick={() => collapsed.clear()}>Desplegar todo</button>
      {/if}
    </div>
    {#if procs.list.length === 0}
      <div class="loading">LEYENDO /proc…</div>
    {:else}
      <ProcTable
        bind:this={table}
        {rows}
        {sortKey}
        {sortDesc}
        {selected}
        {cpuDiv}
        memTotal={s.mem.total}
        uptime={s.uptime}
        height={520}
        onsort={sortBy}
        onselect={(pid) => (selected = pid)}
        ontoggle={toggle}
      />
    {/if}
  </Panel>
  {#if shown}
    <Panel title="Detalle" accent={C.magenta}>
      {#snippet extra()}<button class="close" onclick={() => (selected = null)} aria-label="Cerrar detalle">✕</button>{/snippet}
      <ProcDetail
        p={shown}
        alive={!!current}
        {cpuDiv}
        uptime={s.uptime}
        parent={byPid.get(shown.ppid)}
        childCount={all.filter((p) => p.ppid === shown.pid).length}
        onselect={select}
      />
    </Panel>
  {/if}

  <Panel title="Top CPU" span={2} accent={C.lime}>
    {#snippet extra()}<span>clic para seleccionar</span>{/snippet}
    <EChart height={280} option={race} onclick={pickPid} />
  </Panel>
  <Panel title="CPU × memoria" span={2} accent={C.violet}>
    {#snippet extra()}<span>tamaño = hilos · color = usuario</span>{/snippet}
    <EChart height={280} option={scatter} onclick={pickPid} />
  </Panel>

  <Panel title="Mapa de memoria" span={2} accent={C.magenta}>
    {#snippet extra()}<span>usuario › aplicación › proceso</span>{/snippet}
    <EChart height={340} option={treemap} onclick={pickPid} />
  </Panel>
  <Panel title="CPU por usuario" accent={C.cyan}>
    <EChart height={340} option={sunburst} />
  </Panel>
  <Panel title="Estados" accent={C.amber}>
    <EChart height={340} option={states} />
  </Panel>
</div>

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: 10px;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 10px;
    font-size: 11px;
    flex-wrap: wrap;
  }
  .search {
    flex: 1;
    min-width: min(240px, 100%);
    background: color-mix(in srgb, var(--bg) 70%, transparent);
    border: 1px solid color-mix(in srgb, var(--cyan) 30%, transparent);
    color: var(--text);
    font: inherit;
    font-size: 12px;
    padding: 6px 10px;
    outline: none;
  }
  .search:focus {
    border-color: var(--cyan);
    box-shadow: 0 0 10px color-mix(in srgb, var(--cyan) 30%, transparent);
  }
  .toolbar label {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--muted);
    cursor: pointer;
    white-space: nowrap;
  }
  .toolbar input[type="checkbox"] {
    accent-color: var(--cyan);
  }
  .toolbar button,
  .close {
    background: none;
    border: 1px solid color-mix(in srgb, var(--cyan) 30%, transparent);
    color: var(--text);
    font: inherit;
    font-size: 11px;
    padding: 4px 8px;
    cursor: pointer;
  }
  .close {
    padding: 0 6px;
  }
  .keys {
    font-size: 10px;
  }
  .loading {
    height: 520px;
    display: grid;
    place-content: center;
    font-family: var(--display);
    letter-spacing: 0.3em;
    color: var(--cyan);
  }
</style>
