<script lang="ts">
  import EChart from "$lib/charts/EChart.svelte";
  import TimeChart from "$lib/charts/TimeChart.svelte";
  import Waffle from "$lib/charts/Waffle.svelte";
  import { neonGauge, tooltipBase } from "$lib/charts/echarts";
  import Panel from "$lib/ui/Panel.svelte";
  import Stat from "$lib/ui/Stat.svelte";
  import MemBenchPanel from "$lib/ui/MemBenchPanel.svelte";
  import MemBwPanel from "$lib/ui/MemBwPanel.svelte";
  import { memSegments, metrics } from "$lib/store.svelte";
  import { C, DISPLAY, MONO, rgba } from "$lib/theme";
  import { fmtBytes, fmtPct } from "$lib/format";

  let s = $derived(metrics.snap!);
  let m = $derived(s.mem);
  let seg = $derived(memSegments(m));
  let swapUsed = $derived(m.swapTotal - m.swapFree);
  let usedPct = $derived((seg.used / m.total) * 100);

  let parts = $derived([
    { label: "Aplicaciones", value: seg.apps, color: C.magenta },
    { label: "Compartida", value: seg.shmem, color: C.violet },
    { label: "Buffers", value: seg.buffers, color: C.amber },
    { label: "Caché", value: seg.cache, color: C.cyan },
    { label: "Libre", value: seg.free, color: C.dim },
  ]);

  // Donut con anillo exterior fino que separa "en uso" de "reclamable".
  let donut = $derived({
    tooltip: { ...tooltipBase, formatter: (p: any) => `${p.name}<br><b>${fmtBytes(p.value)}</b> (${p.percent.toFixed(1)}%)` },
    graphic: [
      {
        type: "text",
        left: "center",
        top: "40%",
        style: { text: fmtPct(usedPct, 1), fill: C.magenta, font: `700 26px ${DISPLAY}`, shadowColor: C.magenta, shadowBlur: 12 },
      },
      {
        type: "text",
        left: "center",
        top: "56%",
        style: { text: `${fmtBytes(seg.used)} en uso`, fill: C.muted, font: `11px ${MONO}` },
      },
    ],
    series: [
      {
        type: "pie",
        radius: ["58%", "78%"],
        padAngle: 2,
        itemStyle: { borderRadius: 3 },
        label: { show: false },
        emphasis: { scale: true, scaleSize: 6 },
        data: parts.map((p) => ({ name: p.label, value: p.value, itemStyle: { color: p.color, shadowColor: p.color, shadowBlur: p.label === "Libre" ? 0 : 12 } })),
      },
      {
        type: "pie",
        radius: ["83%", "86%"],
        silent: true,
        label: { show: false },
        data: [
          { value: seg.used, itemStyle: { color: C.magenta } },
          { value: m.available, itemStyle: { color: rgba(C.cyan, 0.35) } },
        ],
      },
    ],
    animationDurationUpdate: 400,
  });

  // Desglose fino de /proc/meminfo.
  let detailRows = $derived([
    { name: "Anon activa", v: m.activeAnon, c: C.magenta },
    { name: "Anon inactiva", v: m.inactiveAnon, c: rgba(C.magenta, 0.55) },
    { name: "Ficheros activos", v: m.activeFile, c: C.cyan },
    { name: "Ficheros inactivos", v: m.inactiveFile, c: rgba(C.cyan, 0.55) },
    { name: "Mapeada", v: m.mapped, c: C.teal },
    { name: "Slab", v: m.slab, c: C.amber },
    { name: "Tablas de páginas", v: m.pageTables, c: C.orange },
    { name: "Pilas del kernel", v: m.kernelStack, c: C.red },
    { name: "Dirty", v: m.dirty, c: C.lime },
    { name: "Writeback", v: m.writeback, c: C.violet },
  ]);

  let detail = $derived({
    tooltip: { ...tooltipBase, trigger: "axis", axisPointer: { type: "shadow" }, formatter: (p: any) => `${p[0].name}<br><b>${fmtBytes(p[0].data.real)}</b>` },
    grid: { left: 124, right: 70, top: 4, bottom: 4 },
    xAxis: { type: "log", show: false, min: 1024 * 64 },
    yAxis: {
      type: "category",
      inverse: true,
      data: detailRows.map((r) => r.name),
      axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 10 },
      axisLine: { show: false },
      axisTick: { show: false },
    },
    series: [
      {
        type: "bar",
        barWidth: 9,
        showBackground: true,
        backgroundStyle: { color: rgba(C.cyan, 0.04) },
        // la escala logarítmica no admite 0: la barra se dibuja con un mínimo de 64 KiB,
        // pero la etiqueta muestra el valor real
        label: { show: true, position: "right", color: C.text, fontFamily: MONO, fontSize: 10, formatter: (p: any) => fmtBytes(p.data.real) },
        data: detailRows.map((r) => ({ value: Math.max(r.v, 1024 * 64), real: r.v, itemStyle: { color: r.c, shadowColor: r.c, shadowBlur: 8 } })),
      },
    ],
    animationDurationUpdate: 300,
  });

  let commitPct = $derived(m.commitLimit ? (m.committedAs / m.commitLimit) * 100 : 0);
</script>

<div class="grid">
  <div class="stats" style="grid-column: span 4">
    <Stat label="En uso" value={fmtBytes(seg.used)} sub={fmtPct(usedPct, 1)} color={C.magenta} history={metrics.h.mem.apps} />
    <Stat label="Disponible" value={fmtBytes(m.available)} sub={`de ${fmtBytes(m.total)}`} color={C.cyan} />
    <Stat label="Caché" value={fmtBytes(seg.cache)} color={C.cyan} history={metrics.h.mem.cache} />
    <Stat label="Buffers" value={fmtBytes(m.buffers)} color={C.amber} history={metrics.h.mem.buffers} />
    <Stat label="Swap" value={fmtBytes(swapUsed)} sub={m.swapTotal ? `de ${fmtBytes(m.swapTotal)}` : "sin swap"} color={C.violet} history={metrics.h.swap} />
    <Stat label="Dirty" value={fmtBytes(m.dirty)} sub={`writeback ${fmtBytes(m.writeback)}`} color={C.lime} history={metrics.h.dirty} />
    <Stat label="Zswap" value={fmtBytes(m.zswap)} sub={m.zswapped ? `comprime ${fmtBytes(m.zswapped)}` : "inactivo"} color={C.teal} />
    <Stat label="Comprometida" value={fmtPct(commitPct)} sub={`${fmtBytes(m.committedAs)} / ${fmtBytes(m.commitLimit)}`} color={C.orange} />
  </div>

  <Panel title="Composición" accent={C.magenta}>
    <EChart height={260} option={donut} />
  </Panel>
  <Panel title="Historial de memoria" span={3} accent={C.magenta}>
    {#snippet extra()}
      {#each parts.slice(0, 4) as p}<span style:color={p.color}>{p.label.toLowerCase()} {fmtBytes(p.value)}</span>{/each}
    {/snippet}
    <TimeChart
      height={250}
      stacked
      yMax={m.total}
      yFmt={(v) => fmtBytes(v, 0)}
      series={[
        { label: "apps", color: C.magenta },
        { label: "compartida", color: C.violet },
        { label: "buffers", color: C.amber },
        { label: "caché", color: C.cyan },
      ]}
      data={() => [metrics.view(metrics.h.mem.apps), metrics.view(metrics.h.mem.shmem), metrics.view(metrics.h.mem.buffers), metrics.view(metrics.h.mem.cache)]}
    />
  </Panel>

  <Panel title="Mapa de RAM" span={2} accent={C.cyan}>
    {#snippet extra()}<span>cada celda ≈ {fmtBytes(m.total / 200)}</span>{/snippet}
    <Waffle segments={parts} fmt={(v) => fmtBytes(v)} />
  </Panel>
  <Panel title="Desglose del kernel" span={2} accent={C.amber}>
    {#snippet extra()}<span>escala logarítmica</span>{/snippet}
    <EChart height={250} option={detail} />
  </Panel>

  <Panel title="Swap" accent={C.violet}>
    <EChart
      height={210}
      option={neonGauge({
        value: m.swapTotal ? (swapUsed / m.swapTotal) * 100 : 0,
        color: C.violet,
        label: "SWAP",
        text: m.swapTotal ? fmtPct((swapUsed / m.swapTotal) * 100, 1) : "N/D",
        sub: `caché ${fmtBytes(m.swapCached)}`,
      })}
    />
  </Panel>
  <Panel title="Swap y escritura pendiente" span={2} accent={C.violet}>
    <TimeChart
      height={200}
      yFmt={(v) => fmtBytes(v, 0)}
      series={[
        { label: "swap", color: C.violet, fill: true },
        { label: "dirty", color: C.lime },
        { label: "writeback", color: C.amber, dash: [4, 3] },
      ]}
      data={() => [metrics.view(metrics.h.swap), metrics.view(metrics.h.dirty), metrics.view(metrics.h.writeback)]}
    />
  </Panel>
  <Panel title="Presión de memoria" accent={C.red}>
    {#snippet extra()}<span>PSI avg10</span>{/snippet}
    <TimeChart
      height={200}
      yFmt={(v) => `${v.toFixed(1)}%`}
      series={[
        { label: "some", color: C.amber, fill: true },
        { label: "full", color: C.red },
      ]}
      data={() => [metrics.view(metrics.h.psi.mem), metrics.view(metrics.h.psi.memFull)]}
    />
  </Panel>
  <Panel title="Tráfico de RAM en vivo" span={4} accent={C.cyan}>
    {#snippet extra()}<span>contadores del controlador de memoria</span>{/snippet}
    <MemBwPanel />
  </Panel>
  <Panel title="Ancho de banda de RAM" span={4} accent={C.cyan}>
    {#snippet extra()}<span>test bajo demanda</span>{/snippet}
    <MemBenchPanel />
  </Panel>
</div>

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: 10px;
  }
</style>
