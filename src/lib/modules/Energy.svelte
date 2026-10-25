<script lang="ts">
  import EChart from "$lib/charts/EChart.svelte";
  import TimeChart from "$lib/charts/TimeChart.svelte";
  import Sparkline from "$lib/charts/Sparkline.svelte";
  import { neonGauge, tooltipBase } from "$lib/charts/echarts";
  import Panel from "$lib/ui/Panel.svelte";
  import Stat from "$lib/ui/Stat.svelte";
  import { metrics } from "$lib/store.svelte";
  import { procs } from "$lib/procs.svelte";
  import { want } from "$lib/collect";
  import { energy, estimateProcs, fmtWatts, fmtWh, KIND_LABEL, kindColor, dayKey, cpuWatts, REST_ID, REST_LABEL, REST_COLOR } from "$lib/energy.svelte";
  import UpsPanel from "$lib/ui/UpsPanel.svelte";
  import { C, MONO, rgba } from "$lib/theme";
  import { fmtDuration } from "$lib/format";
  import { platform } from "$lib/platform.svelte";
  import type { EnergySource } from "$lib/types";

  // la estimación por proceso necesita la lista de procesos y el uso de GPU por proceso
  $effect(() => {
    procs.want("energy-tab", true);
    want("gpu_procs", "energy-tab", true);
    return () => {
      procs.want("energy-tab", false);
      want("gpu_procs", "energy-tab", false);
    };
  });

  let s = $derived(metrics.snap!);
  let e = $derived(s.energy);
  let sources = $derived(e.sources);
  let byId = $derived(new Map(sources.map((x) => [x.id, x])));
  let summed = $derived(sources.filter((x) => x.inTotal));
  let cpuW = $derived(cpuWatts(s));
  let gpuW = $derived(sources.filter((x) => x.kind === "gpu" || x.kind === "igpu").reduce((a, x) => a + (x.watts ?? 0), 0));
  let hasGpu = $derived(sources.some((x) => x.kind === "gpu" || x.kind === "igpu"));

  // Sesión
  let elapsed = $derived(Math.max(1, (s.ts - energy.startTs) / 1000));
  let sessionWh = $derived(e.totalJ / 3600);
  let avgW = $derived(e.totalJ / elapsed);
  let today = $derived.by(() => {
    energy.version;
    return energy.days[dayKey()] ?? { wh: 0, src: {}, hours: new Array(24).fill(0) };
  });
  let monthWh = $derived.by(() => {
    energy.version;
    return energy.monthWh();
  });

  const METHOD: Record<string, string> = {
    ups: "SAI: todo lo enchufado (PC, monitores…)",
    platform: "sensor de plataforma (equipo completo)",
    psu: "fuente de alimentación (equipo completo)",
    battery: "descarga de la batería (equipo completo)",
    sum: "suma de componentes medidos",
    none: "sin fuentes medibles",
  };

  // ── Permiso RAPL ──
  let busy = $state(false);
  let raplMsg = $state<string | null>(null);
  async function rapl(enable: boolean, persistent: boolean) {
    busy = true;
    raplMsg = null;
    try {
      await energy.raplAccess(enable, persistent);
      raplMsg = enable ? "Permiso concedido. Las lecturas de CPU aparecerán en unos segundos." : "Permiso retirado.";
    } catch (err) {
      raplMsg = String(err);
    } finally {
      busy = false;
    }
  }

  // ── Potencia apilada por componente ──
  let ups = $derived(sources.find((x) => x.kind === "ups"));
  let stackIds = $derived(summed.map((x) => x.id));
  let stackSeries = $derived([
    ...summed.map((x) => ({ label: x.label, color: kindColor(x.kind), width: 1.4 })),
    ...(ups ? [{ label: REST_LABEL, color: REST_COLOR(), width: 1.4 }] : []),
  ]);
  /** Series apiladas; con SAI se añade el resto (SAI − componentes) para que sumen su total. */
  function stackData() {
    const parts = stackIds.map((id) => metrics.view(metrics.h.energy.get(id) ?? []));
    if (!ups) return parts;
    const total = metrics.view(metrics.h.energy.get(ups.id) ?? []);
    const rest = total.map((t, i) => Math.max(0, t - parts.reduce((a, p) => a + (p[i] ?? 0), 0)));
    return [...parts, rest];
  }
  let restW = $derived(ups ? Math.max(0, (ups.watts ?? 0) - summed.reduce((a, x) => a + (x.watts ?? 0), 0)) : 0);
  // todas las fuentes, en líneas (incluye subzonas y sensores que no suman)
  let allIds = $derived(sources.map((x) => x.id));
  const PALETTE = () => [C.cyan, C.magenta, C.amber, C.lime, C.violet, C.teal, C.blue, C.orange, C.red];
  let allSeries = $derived([
    { label: "Total medido", color: C.text, width: 2, dash: [5, 3] },
    // mismos colores que las tarjetas; las subzonas (núcleos dentro del paquete) en discontinua
    ...sources.map((x) => ({ label: x.label, color: kindColor(x.kind), width: 1.3, dash: x.parent ? [4, 3] : undefined })),
  ]);

  // ── Reparto de la energía de la sesión (sunburst: subzonas dentro de su padre) ──
  let share = $derived.by(() => {
    const tops = ups ? [ups] : sources.filter((x) => !x.parent && (x.inTotal || x.system));
    const node = (x: EnergySource): any => {
      // el SAI contiene a los componentes medidos y al resto del puesto
      if (x.kind === "ups") {
        const parts = summed.map(node);
        const rest = x.joules / 3600 - summed.reduce((a, c) => a + c.joules / 3600, 0);
        if (rest > 0) parts.push({ name: REST_LABEL, value: rest, itemStyle: { color: REST_COLOR() } });
        return { name: x.label, value: x.joules / 3600, itemStyle: { color: kindColor(x.kind) }, children: parts };
      }
      const kids = sources.filter((c) => c.parent === x.id);
      const wh = x.joules / 3600;
      const children = kids.map((k) => ({ name: k.label, value: k.joules / 3600, itemStyle: { color: kindColor(k.kind) } }));
      const rest = wh - children.reduce((a, c) => a + c.value, 0);
      if (children.length && rest > 0) children.push({ name: `${x.label} · resto`, value: rest, itemStyle: { color: rgba(kindColor(x.kind), 0.45) } });
      return { name: x.label, value: wh, itemStyle: { color: kindColor(x.kind) }, children: children.length ? children : undefined };
    };
    return {
      tooltip: { ...tooltipBase, formatter: (p: any) => `${p.name}<br><b>${fmtWh(p.value)}</b> · ${energy.fmtCost(p.value)}` },
      series: [
        {
          type: "sunburst",
          radius: ["34%", "94%"],
          center: ["50%", "50%"],
          sort: undefined,
          nodeClick: false,
          data: tops.map(node),
          itemStyle: { borderColor: C.bg, borderWidth: 2 },
          label: { show: false },
          levels: [{}, { r0: "34%", r: "72%" }, { r0: "74%", r: "94%" }],
        },
      ],
      graphic: {
        type: "text",
        left: "center",
        top: "middle",
        style: { text: fmtWh(sessionWh), fill: C.text, font: `700 12px ${MONO}`, textAlign: "center" },
      },
    };
  });

  let shareRows = $derived.by(() => {
    if (ups) {
      const tot = ups.joules || 1;
      const restJ = Math.max(0, ups.joules - summed.reduce((a, c) => a + c.joules, 0));
      return [
        { id: ups.id, label: ups.label, wh: ups.joules / 3600, pct: 100, color: kindColor("ups"), sub: false },
        ...summed.map((c) => ({ id: c.id, label: c.label, wh: c.joules / 3600, pct: (c.joules / tot) * 100, color: kindColor(c.kind), sub: true })),
        { id: REST_ID, label: REST_LABEL, wh: restJ / 3600, pct: (restJ / tot) * 100, color: REST_COLOR(), sub: true },
      ];
    }
    const tops = sources.filter((x) => !x.parent && (x.inTotal || x.system));
    const tot = tops.reduce((a, x) => a + x.joules, 0) || 1;
    return tops.flatMap((x) => [
      { id: x.id, label: x.label, wh: x.joules / 3600, pct: (x.joules / tot) * 100, color: kindColor(x.kind), sub: false },
      ...sources.filter((c) => c.parent === x.id).map((c) => ({ id: c.id, label: c.label, wh: c.joules / 3600, pct: (c.joules / tot) * 100, color: kindColor(c.kind), sub: true })),
    ]);
  });

  // ── Barras: hoy por horas ──
  let hourBars = $derived.by(() => {
    const h = today.hours;
    const nowH = new Date().getHours();
    return {
      tooltip: { ...tooltipBase, trigger: "axis", axisPointer: { type: "shadow" }, formatter: (p: any) => `${p[0].name}:00<br><b>${fmtWh(p[0].value)}</b> · ${energy.fmtCost(p[0].value)}` },
      grid: { left: 50, right: 8, top: 10, bottom: 22 },
      xAxis: { type: "category", data: h.map((_, i) => String(i).padStart(2, "0")), axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9, interval: 2 }, axisLine: { lineStyle: { color: C.grid } }, axisTick: { show: false } },
      yAxis: { type: "value", axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9, formatter: (v: number) => `${v.toFixed(0)} Wh` }, splitLine: { lineStyle: { color: C.grid } } },
      series: [
        {
          type: "bar",
          barWidth: "70%",
          data: h.map((v, i) => ({
            value: v,
            itemStyle: {
              color: { type: "linear", x: 0, y: 0, x2: 0, y2: 1, colorStops: [{ offset: 0, color: i === nowH ? C.amber : C.cyan }, { offset: 1, color: rgba(i === nowH ? C.amber : C.cyan, 0.2) }] },
              shadowColor: i === nowH ? C.amber : C.cyan,
              shadowBlur: 6,
            },
          })),
        },
      ],
    };
  });

  // ── Barras apiladas: últimos 30 días por fuente ──
  let dayBars = $derived.by(() => {
    energy.version;
    const days: string[] = [];
    const d = new Date();
    for (let i = 29; i >= 0; i--) days.push(dayKey(new Date(d.getFullYear(), d.getMonth(), d.getDate() - i)));
    // fuentes que alguna vez sumaron al total
    const ids = new Set<string>();
    for (const k of days) for (const id of Object.keys(energy.days[k]?.src ?? {})) ids.add(id);
    const cur = new Map(sources.map((x) => [x.id, x]));
    const counted = [...ids];
    const pal = PALETTE();
    return {
      tooltip: {
        ...tooltipBase,
        trigger: "axis",
        axisPointer: { type: "shadow" },
        formatter: (p: any[]) => {
          const k = days[p[0].dataIndex];
          const tot = energy.days[k]?.wh ?? 0;
          return `<b>${k}</b> · ${fmtWh(tot)} · ${energy.fmtCost(tot)}<br>` + p.filter((x) => x.value > 0).map((x) => `${x.marker}${x.seriesName}: ${fmtWh(x.value)}`).join("<br>");
        },
      },
      grid: { left: 56, right: 8, top: 10, bottom: 22 },
      xAxis: { type: "category", data: days.map((k) => k.slice(8) + "/" + k.slice(5, 7)), axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9, interval: 4 }, axisLine: { lineStyle: { color: C.grid } }, axisTick: { show: false } },
      yAxis: { type: "value", axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9, formatter: (v: number) => fmtWh(v) }, splitLine: { lineStyle: { color: C.grid } } },
      series: counted.map((id, i) => ({
        type: "bar",
        stack: "e",
        name: energy.labels[id] ?? id,
        barWidth: "60%",
        itemStyle: { color: id === REST_ID ? REST_COLOR() : cur.get(id) ? kindColor(cur.get(id)!.kind) : pal[i % pal.length] },
        data: days.map((k) => energy.days[k]?.src[id] ?? 0),
      })),
    };
  });

  // ── Dispersión: uso vs potencia (curva de eficiencia) ──
  let scatter = $derived.by(() => {
    metrics.tick;
    const n = 600;
    const pick = (use: number[], w: number[]) => {
      const len = Math.min(use.length, w.length);
      const step = Math.max(1, Math.floor(len / n));
      const out: number[][] = [];
      for (let i = Math.max(0, len - n * step); i < len; i += step) if (w[i] > 0) out.push([use[i], w[i]]);
      return out;
    };
    const cpuSrc = sources.find((x) => x.kind === "cpu");
    const gpuSrc = sources.find((x) => x.kind === "gpu" || x.kind === "igpu");
    const series: any[] = [];
    if (cpuSrc) series.push({ name: "CPU", type: "scatter", symbolSize: 4, itemStyle: { color: rgba(C.cyan, 0.55) }, data: pick(metrics.raw(metrics.h.cpu.usage), metrics.raw(metrics.h.energy.get(cpuSrc.id) ?? [])) });
    if (gpuSrc && metrics.h.gpus[0]) series.push({ name: "GPU", type: "scatter", symbolSize: 4, itemStyle: { color: rgba(C.magenta, 0.55) }, data: pick(metrics.raw(metrics.h.gpus[0].busy), metrics.raw(metrics.h.energy.get(gpuSrc.id) ?? [])) });
    return {
      tooltip: { ...tooltipBase, formatter: (p: any) => `${p.seriesName}: ${p.value[0].toFixed(0)}% → <b>${p.value[1].toFixed(1)} W</b>` },
      legend: { top: 0, right: 0, textStyle: { color: C.muted, fontFamily: MONO, fontSize: 10 }, itemWidth: 8, itemHeight: 8 },
      grid: { left: 44, right: 12, top: 24, bottom: 30 },
      xAxis: { type: "value", min: 0, max: 100, name: "uso %", nameLocation: "middle", nameGap: 20, nameTextStyle: { color: C.muted, fontSize: 9 }, axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9 }, splitLine: { lineStyle: { color: C.grid } } },
      yAxis: { type: "value", axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9, formatter: (v: number) => `${v} W` }, splitLine: { lineStyle: { color: C.grid } } },
      series,
    };
  });

  // ── Por proceso ──
  let procRows = $derived.by(() => {
    procs.tick;
    return estimateProcs(s, procs.list, s.gpus).slice(0, 14);
  });
  $effect(() => {
    procs.tick;
    energy.accumulateProcs(estimateProcs(metrics.snap!, procs.list, metrics.snap!.gpus));
  });
  let procMax = $derived(Math.max(1, ...procRows.map((r) => r.totalW)));

  // ── Estadísticas de cada fuente en la ventana visible ──
  function stats(id: string) {
    metrics.tick;
    const v = metrics.raw(metrics.h.energy.get(id) ?? []).filter((x) => x > 0);
    if (!v.length) return null;
    let min = Infinity;
    let max = 0;
    let sum = 0;
    for (const x of v) {
      min = Math.min(min, x);
      max = Math.max(max, x);
      sum += x;
    }
    return { min, max, avg: sum / v.length };
  }

  function role(x: EnergySource) {
    if (x.system) return "mide todo el equipo";
    if (x.parent) return `dentro de ${byId.get(x.parent)?.label ?? x.parent}`;
    if (x.inTotal) return "suma al total";
    return x.kind === "charge" ? "potencia de carga" : "no suma (podría solaparse)";
  }

  let gauges = $derived(sources.filter((x) => x.limit && x.watts != null));

  // precio editable
  let priceIn = $state(energy.price);
  let currencyIn = $state(energy.currency);
  const savePrice = () => energy.setPrice(Number(priceIn) || 0, currencyIn || "$");
  const proj = (hours: number) => (avgW * hours) / 1000;
</script>

<div class="grid">
  <div class="stats" style="grid-column: span 4">
    <Stat label="Consumo ahora" value={e.totalMethod === "none" ? "N/D" : fmtWatts(e.totalW)} sub={METHOD[e.totalMethod]} color={C.amber} history={metrics.h.energyTotal} />
    <Stat label="CPU" value={fmtWatts(cpuW)} sub={cpuW == null ? (e.rapl === "denied" ? "sin permiso RAPL" : "no disponible") : "paquete (RAPL)"} color={C.cyan} history={sources.find((x) => x.kind === "cpu") ? metrics.h.energy.get(sources.find((x) => x.kind === "cpu")!.id) : undefined} />
    <Stat label="GPU" value={hasGpu ? fmtWatts(gpuW) : "N/D"} sub={sources.filter((x) => x.kind === "gpu" || x.kind === "igpu").map((x) => x.label).join(", ") || "sin sensor"} color={C.magenta} />
    <Stat label="Media sesión" value={fmtWatts(avgW)} sub={`pico ${fmtWatts(energy.peakW)} · mín ${Number.isFinite(energy.minW) ? fmtWatts(energy.minW) : "N/D"}`} color={C.teal} />
    <Stat label="Sesión" value={fmtWh(sessionWh)} sub={`${energy.fmtCost(sessionWh)} · ${fmtDuration(elapsed)}`} color={C.lime} />
    <Stat label="Hoy" value={fmtWh(today.wh)} sub={energy.fmtCost(today.wh)} color={C.violet} />
    <Stat label="Este mes" value={fmtWh(monthWh)} sub={energy.fmtCost(monthWh)} color={C.orange} />
    <Stat label="Proyección 30 días" value={`${proj(24 * 30).toFixed(1)} kWh`} sub={`${energy.currency}${(proj(24 * 30) * energy.price).toFixed(2)} al ritmo medio`} color={C.red} />
  </div>

  {#if e.ups}
    <UpsPanel ups={e.ups} rest={restW} parts={summed.reduce((a, x) => a + (x.watts ?? 0), 0)} />
  {/if}

  {#if e.rapl === "denied" && platform.desktop}
    <Panel title="Consumo de CPU bloqueado" span={4} accent={C.amber}>
      <div class="rapl">
        <p>
          Tu procesador mide su propio consumo (contadores <b>RAPL</b>), pero el kernel solo deja leerlos a root desde 2020, por la vulnerabilidad
          PLATYPUS (CVE-2020-8694): con lecturas muy frecuentes de energía, un programa local podría llegar a deducir datos que procesa la CPU. En un
          equipo personal de un solo usuario el riesgo es bajo, pero es tu decisión.
        </p>
        <div class="btns">
          <button disabled={busy} onclick={() => rapl(true, false)}>Permitir hasta reiniciar</button>
          <button class="primary" disabled={busy} onclick={() => rapl(true, true)}>Permitir siempre</button>
        </div>
        <p class="hint">
          Se pedirá tu contraseña en la ventana de autenticación del sistema (polkit). «Siempre» instala la regla
          <code>/etc/udev/rules.d/60-neontop-rapl.rules</code>; para revertirlo, borra ese fichero o usa «Retirar permiso» aquí.
        </p>
        {#if raplMsg}<p class="msg">{raplMsg}</p>{/if}
      </div>
    </Panel>
  {/if}

  <Panel title="Potencia por componente" span={3} accent={C.amber}>
    {#snippet extra()}
      {#each stackSeries as x}<span style:color={x.color}>{x.label}</span>{/each}
      <span>{ups ? "apilado = total del SAI" : "apilado = total medido"}</span>
    {/snippet}
    {#key stackIds.join() + (ups ? "+ups" : "")}
      <TimeChart height={260} stacked yFmt={(v) => `${v.toFixed(0)} W`} series={stackSeries} data={stackData} />
    {/key}
  </Panel>
  <Panel title="Reparto de la energía" accent={C.lime}>
    {#snippet extra()}<span>desde el arranque</span>{/snippet}
    <EChart height={190} option={share} />
    <ul class="legend">
      {#each shareRows as r (r.id)}
        <li style:--c={r.color} class:sub={r.sub}>
          <i></i><span>{r.label}</span><b>{fmtWh(r.wh)}</b><em>{r.pct.toFixed(0)}%</em>
        </li>
      {/each}
    </ul>
  </Panel>

  <div class="sources" style="grid-column: span 4">
    {#each sources as x (x.id)}
      {@const st = stats(x.id)}
      {@const color = kindColor(x.kind)}
      <div class="src" style:--c={color}>
        <div class="top">
          <span class="badge">{KIND_LABEL[x.kind]}</span>
          <span class="name" title={x.label}>{x.label}</span>
          <span class="method" title={x.method === "counter" ? "Contador de energía del hardware: exacto" : "Potencia instantánea integrada en el tiempo"}>{x.method === "counter" ? "contador" : "sensor"}</span>
        </div>
        <div class="mid">
          <b>{fmtWatts(x.watts)}</b>
          <div class="spark"><Sparkline values={metrics.h.energy.get(x.id) ?? []} {color} height={28} /></div>
        </div>
        {#if x.limit}
          <div class="bar"><i style:width="{Math.min(100, ((x.watts ?? 0) / x.limit) * 100)}%"></i></div>
        {/if}
        <div class="foot">
          <span>{st ? `mín ${st.min.toFixed(1)} · media ${st.avg.toFixed(1)} · máx ${st.max.toFixed(1)} W` : "sin datos"}</span>
          <span>{fmtWh(x.joules / 3600)}{x.limit ? ` · límite ${x.limit.toFixed(0)} W` : ""}</span>
        </div>
        <div class="role">{role(x)}</div>
      </div>
    {/each}
    {#if !sources.length}
      <p class="hint">No se encontró ninguna fuente de consumo medible.</p>
    {/if}
  </div>

  <Panel title="Todas las fuentes" span={4} accent={C.cyan}>
    {#snippet extra()}
      {#each allSeries as x}<span style:color={x.color}>{x.label}</span>{/each}
    {/snippet}
    {#key allIds.join()}
      <TimeChart height={220} yFmt={(v) => `${v.toFixed(0)} W`} series={allSeries} data={() => [metrics.view(metrics.h.energyTotal), ...allIds.map((id) => metrics.view(metrics.h.energy.get(id) ?? []))]} />
    {/key}
  </Panel>

  <Panel title="Hoy, por horas" span={2} accent={C.cyan}>
    {#snippet extra()}<span>{fmtWh(today.wh)} · {energy.fmtCost(today.wh)}</span>{/snippet}
    <EChart height={220} option={hourBars} />
  </Panel>
  <Panel title="Últimos 30 días" span={2} accent={C.violet}>
    {#snippet extra()}<span>se guarda mientras NeonTop está abierto</span>{/snippet}
    <EChart height={220} option={dayBars} />
  </Panel>

  <Panel title="Curva de eficiencia" span={2} accent={C.teal}>
    {#snippet extra()}<span>uso frente a potencia · últimas muestras</span>{/snippet}
    {#if scatter.series.length}
      <EChart height={240} option={scatter} />
    {:else}
      <p class="hint">Hace falta la potencia de la CPU o de la GPU para dibujar la curva.</p>
    {/if}
  </Panel>
  {#each gauges.slice(0, 2) as g (g.id)}
    <Panel title={`${KIND_LABEL[g.kind]} · límite`} accent={kindColor(g.kind)} half>
      {#snippet extra()}<span title={g.label}>{g.label}</span>{/snippet}
      <EChart height={240} option={neonGauge({ value: g.watts ?? 0, max: g.limit!, color: kindColor(g.kind), label: "LÍMITE " + g.limit!.toFixed(0) + " W", text: fmtWatts(g.watts), sub: `${(((g.watts ?? 0) / g.limit!) * 100).toFixed(0)}%` })} />
    </Panel>
  {/each}

  <Panel title="Consumo estimado por proceso" span={2} accent={C.magenta}>
    {#snippet extra()}<span>reparto de la potencia medida</span>{/snippet}
    {#if cpuW == null && !hasGpu}
      <p class="hint">Sin potencia de CPU ni de GPU no se puede repartir el consumo entre procesos.</p>
    {:else}
      <table class="proctable">
        <thead><tr><th>Proceso</th><th>CPU</th><th>GPU</th><th class="barcol">Total</th><th>Acum.</th></tr></thead>
        <tbody>
          {#each procRows as r (r.name)}
            <tr>
              <td class="pname" title={r.name}>{r.name}{r.count > 1 ? ` ×${r.count}` : ""}</td>
              <td>{r.cpuW == null ? "—" : r.cpuW.toFixed(1)}</td>
              <td>{r.gpuW == null ? "—" : r.gpuW.toFixed(1)}</td>
              <td class="barcol">
                <div class="pbar">
                  <i style:width="{((r.cpuW ?? 0) / procMax) * 100}%" style:background={C.cyan}></i><i style:width="{((r.gpuW ?? 0) / procMax) * 100}%" style:background={C.magenta}></i>
                  <span>{r.totalW.toFixed(1)} W</span>
                </div>
              </td>
              <td>{fmtWh((energy.procJ.get(r.name) ?? 0) / 3600)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="hint">
        Estimación: la potencia de la CPU se reparte según el tiempo de CPU de cada proceso y la de la GPU según el uso de sus motores; incluye el
        consumo en reposo repartido. «Acum.» cuenta mientras esta pestaña está abierta.
        {#if cpuW == null}<br />Sin permiso RAPL solo se reparte la GPU.{/if}
      </p>
    {/if}
  </Panel>

  <Panel title="Coste" span={2} accent={C.orange}>
    <div class="cost">
      <label>
        Precio por kWh
        <span class="inp">
          <input class="cur" bind:value={currencyIn} onchange={savePrice} maxlength="5" />
          <input type="number" min="0" step="0.01" bind:value={priceIn} onchange={savePrice} />
        </span>
      </label>
      <table class="ctable">
        <thead><tr><th></th><th>Energía</th><th>Coste</th></tr></thead>
        <tbody>
          <tr><td>Sesión ({fmtDuration(elapsed)})</td><td>{fmtWh(sessionWh)}</td><td>{energy.fmtCost(sessionWh)}</td></tr>
          <tr><td>Hoy</td><td>{fmtWh(today.wh)}</td><td>{energy.fmtCost(today.wh)}</td></tr>
          <tr><td>Este mes</td><td>{fmtWh(monthWh)}</td><td>{energy.fmtCost(monthWh)}</td></tr>
          <tr class="sep"><td>1 hora al ritmo medio</td><td>{fmtWh(avgW)}</td><td>{energy.fmtCost(avgW)}</td></tr>
          <tr><td>24 horas</td><td>{fmtWh(avgW * 24)}</td><td>{energy.fmtCost(avgW * 24)}</td></tr>
          <tr><td>30 días</td><td>{fmtWh(avgW * 24 * 30)}</td><td>{energy.fmtCost(avgW * 24 * 30)}</td></tr>
          <tr><td>1 año</td><td>{fmtWh(avgW * 24 * 365)}</td><td>{energy.fmtCost(avgW * 24 * 365)}</td></tr>
        </tbody>
      </table>
      <p class="hint">
        {#if e.totalMethod === "ups"}
          El total es lo que mide el <b>SAI</b>: todo lo enchufado a él (PC, monitores y lo que tengas conectado), incluidas las pérdidas de la
          fuente. Es el consumo real del puesto en el enchufe.
        {:else}
          El total es lo <b>medido</b>: {METHOD[e.totalMethod]}. Sin un sensor del equipo completo (SAI, plataforma, fuente de alimentación con
          sensor o batería) no se incluyen placa base, discos, ventiladores, RAM (en AMD), pérdidas de la fuente ni el monitor, así que el
          consumo real en el enchufe es mayor.
        {/if}
      </p>
      {#if e.rapl === "ok" && platform.desktop}
        <p class="hint">
          RAPL: acceso {e.raplRule ? "permanente (regla udev)" : "hasta reiniciar"}.
          {#if !e.raplRule}<button class="link" disabled={busy} onclick={() => rapl(true, true)}>Hacer permanente</button>{/if}
          <button class="link" disabled={busy} onclick={() => rapl(false, false)}>Retirar permiso</button>
          {#if raplMsg}<br /><span class="msg">{raplMsg}</span>{/if}
        </p>
      {:else if e.rapl === "absent"}
        <p class="hint">Este procesador o kernel no expone contadores RAPL: el consumo de la CPU no se puede medir.</p>
      {/if}
    </div>
  </Panel>
</div>

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: 10px;
  }
  .sources {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 12px;
  }
  .src {
    border: 1px solid color-mix(in srgb, var(--c) 30%, transparent);
    border-left: 3px solid var(--c);
    border-radius: var(--radius-sm);
    background: linear-gradient(135deg, color-mix(in srgb, var(--c) 7%, var(--panel)), var(--panel));
    padding: 10px 12px;
    display: grid;
    gap: 6px;
    min-width: 0;
  }
  .top {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 11px;
  }
  .badge {
    color: var(--c);
    border: 1px solid color-mix(in srgb, var(--c) 50%, transparent);
    padding: 0 5px;
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    border-radius: var(--radius-sm);
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .method {
    color: var(--muted);
    font-size: 9px;
  }
  .mid {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 12px;
    align-items: center;
  }
  .mid b {
    font-family: var(--display);
    font-size: 22px;
    color: var(--c);
    font-variant-numeric: tabular-nums;
    min-width: 86px;
  }
  .bar {
    height: 3px;
    background: color-mix(in srgb, var(--c) 12%, transparent);
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--c);
  }
  .foot {
    display: grid;
    gap: 2px;
    font-size: 9.5px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .role {
    font-size: 9px;
    color: color-mix(in srgb, var(--c) 70%, var(--muted));
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .rapl p {
    margin: 0 0 10px;
    font-size: 12px;
    line-height: 1.6;
  }
  .btns {
    display: flex;
    gap: 10px;
    margin-bottom: 10px;
  }
  .btns button,
  .link {
    background: transparent;
    color: var(--cyan);
    border: 1px solid color-mix(in srgb, var(--cyan) 50%, transparent);
    padding: 6px 14px;
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .btns button.primary {
    background: color-mix(in srgb, var(--cyan) 18%, transparent);
  }
  .link {
    padding: 1px 8px;
    font-size: 10px;
    margin-left: 6px;
  }
  button:disabled {
    opacity: 0.5;
    cursor: wait;
  }
  .msg {
    color: var(--amber);
  }
  .hint {
    font-size: 11px;
    color: var(--muted);
    margin: 8px 0 0;
    line-height: 1.6;
  }
  code {
    color: var(--cyan);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  th {
    text-align: right;
    color: var(--muted);
    font-weight: normal;
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    padding: 0 6px 5px;
  }
  th:first-child,
  td:first-child {
    text-align: left;
  }
  td {
    text-align: right;
    padding: 3px 6px;
    border-top: 1px solid color-mix(in srgb, var(--cyan) 7%, transparent);
  }
  .pname {
    max-width: 150px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .barcol {
    width: 38%;
  }
  .pbar {
    position: relative;
    display: flex;
    height: 14px;
    background: color-mix(in srgb, var(--cyan) 5%, transparent);
  }
  .pbar i {
    display: block;
    height: 100%;
    opacity: 0.75;
  }
  .pbar span {
    position: absolute;
    right: 4px;
    top: 0;
    line-height: 14px;
    font-size: 10px;
  }
  .cost label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 11px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    margin-bottom: 10px;
  }
  .inp {
    display: flex;
    gap: 6px;
  }
  .cost input {
    background: color-mix(in srgb, var(--cyan) 6%, transparent);
    border: 1px solid color-mix(in srgb, var(--cyan) 30%, transparent);
    color: var(--text);
    font: inherit;
    font-size: 12px;
    padding: 4px 8px;
    width: 90px;
  }
  .cost input.cur {
    width: 48px;
  }
  .legend {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
    display: grid;
    gap: 4px;
    font-size: 10.5px;
  }
  .legend li {
    display: grid;
    grid-template-columns: 8px 1fr auto 34px;
    gap: 8px;
    align-items: center;
    font-variant-numeric: tabular-nums;
  }
  .legend li.sub {
    padding-left: 14px;
    color: var(--muted);
  }
  .legend i {
    width: 8px;
    height: 8px;
    background: var(--c);
    border-radius: 2px;
  }
  .legend span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .legend em {
    font-style: normal;
    color: var(--muted);
    text-align: right;
  }
  .proctable td {
    white-space: nowrap;
  }
  /* teléfono: sin las columnas CPU/GPU (la barra ya muestra el reparto con sus colores) */
  @media (max-width: 700px) {
    .proctable th:nth-child(2),
    .proctable td:nth-child(2),
    .proctable th:nth-child(3),
    .proctable td:nth-child(3) {
      display: none;
    }
  }
  .ctable tr.sep td {
    border-top: 1px solid color-mix(in srgb, var(--cyan) 25%, transparent);
  }
</style>
