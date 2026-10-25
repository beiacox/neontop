<script lang="ts">
  import EChart from "$lib/charts/EChart.svelte";
  import TimeChart from "$lib/charts/TimeChart.svelte";
  import Heatmap from "$lib/charts/Heatmap.svelte";
  import { neonGauge, tooltipBase } from "$lib/charts/echarts";
  import Panel from "$lib/ui/Panel.svelte";
  import Stat from "$lib/ui/Stat.svelte";
  import { metrics } from "$lib/store.svelte";
  import { C, DISPLAY, MONO, heatColor, rgba, tempColor } from "$lib/theme";
  import { fmtFreq, fmtNum, fmtPct } from "$lib/format";

  let s = $derived(metrics.snap!);
  let info = $derived(metrics.info);
  let t = $derived(s.cpu.total);
  let freqAvg = $derived(s.cpu.freqs.reduce((a, b) => a + b, 0) / Math.max(1, s.cpu.freqs.length));
  let coreNames = $derived(s.cpu.cores.map((_, i) => `C${i}`));

  const BREAKDOWN = [
    { key: "user", label: "user", color: C.cyan },
    { key: "nice", label: "nice", color: C.teal },
    { key: "system", label: "system", color: C.magenta },
    { key: "iowait", label: "iowait", color: C.amber },
    { key: "irq", label: "irq", color: C.violet },
    { key: "softirq", label: "softirq", color: C.blue },
    { key: "steal", label: "steal", color: C.red },
  ] as const;

  // Radar del reparto: escala logarítmica suave para que lo pequeño (irq, softirq) se vea.
  let radar = $derived({
    tooltip: { ...tooltipBase },
    radar: {
      indicator: BREAKDOWN.map((b) => ({ name: b.label, max: 100 })),
      radius: "60%",
      center: ["50%", "54%"],
      splitNumber: 4,
      axisName: { color: C.muted, fontFamily: MONO, fontSize: 10 },
      splitLine: { lineStyle: { color: rgba(C.cyan, 0.15) } },
      splitArea: { areaStyle: { color: [rgba(C.cyan, 0.02), rgba(C.cyan, 0.05)] } },
      axisLine: { lineStyle: { color: rgba(C.cyan, 0.15) } },
    },
    series: [
      {
        type: "radar",
        symbol: "circle",
        symbolSize: 4,
        data: [
          {
            value: BREAKDOWN.map((b) => Math.sqrt(t[b.key]) * 10),
            name: "reparto",
            lineStyle: { color: C.magenta, width: 2, shadowColor: C.magenta, shadowBlur: 10 },
            itemStyle: { color: C.magenta },
            areaStyle: { color: rgba(C.magenta, 0.25) },
          },
        ],
        tooltip: {
          formatter: () => BREAKDOWN.map((b) => `${b.label}: <b>${t[b.key].toFixed(1)}%</b>`).join("<br>"),
        },
      },
    ],
    animationDurationUpdate: 300,
  });

  // Columnas radiales: un sector por núcleo alrededor del círculo, la longitud es el uso.
  let polar = $derived({
    tooltip: { ...tooltipBase, formatter: (p: any) => `${p.name}: <b>${p.value.toFixed(1)}%</b>` },
    polar: { radius: ["22%", "82%"], center: ["50%", "53%"] },
    angleAxis: {
      type: "category",
      data: coreNames,
      startAngle: 90,
      axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9 },
      axisLine: { lineStyle: { color: rgba(C.lime, 0.2) } },
      axisTick: { show: false },
      splitLine: { show: true, lineStyle: { color: rgba(C.lime, 0.06) } },
    },
    radiusAxis: {
      max: 100,
      axisLabel: { show: false },
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: rgba(C.lime, 0.08), type: "dashed" } },
    },
    graphic: [
      {
        type: "text",
        left: "center",
        top: "middle",
        style: { text: fmtPct(t.usage), fill: heatColor(t.usage), font: `700 16px ${DISPLAY}`, shadowColor: heatColor(t.usage), shadowBlur: 10 },
      },
    ],
    series: [
      {
        type: "bar",
        coordinateSystem: "polar",
        barWidth: "70%",
        itemStyle: { borderRadius: 3 },
        data: s.cpu.cores.map((c) => ({
          value: Math.max(c.usage, 1.5),
          itemStyle: { color: heatColor(c.usage), shadowColor: heatColor(c.usage), shadowBlur: 10 },
        })),
      },
    ],
    animationDurationUpdate: 300,
  });

  // Frecuencia por núcleo sobre su máximo teórico.
  let freqBars = $derived({
    tooltip: { ...tooltipBase, trigger: "axis", axisPointer: { type: "shadow" }, valueFormatter: (v: number) => fmtFreq(v) },
    grid: { left: 44, right: 10, top: 10, bottom: 24 },
    xAxis: {
      type: "category",
      data: coreNames,
      axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 10 },
      axisLine: { lineStyle: { color: rgba(C.cyan, 0.2) } },
      axisTick: { show: false },
    },
    yAxis: {
      type: "value",
      max: Math.max(...(info?.freqMax ?? [0]), ...s.cpu.freqs) || undefined,
      axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 10, formatter: (v: number) => (v / 1000).toFixed(1) },
      splitLine: { lineStyle: { color: C.grid } },
    },
    series: [
      {
        type: "bar",
        barWidth: "56%",
        showBackground: true,
        backgroundStyle: { color: rgba(C.violet, 0.07) },
        itemStyle: {
          color: {
            type: "linear",
            x: 0,
            y: 1,
            x2: 0,
            y2: 0,
            colorStops: [
              { offset: 0, color: rgba(C.violet, 0.3) },
              { offset: 0.6, color: C.violet },
              { offset: 1, color: C.cyan },
            ],
          },
          shadowColor: C.violet,
          shadowBlur: 10,
        },
        data: s.cpu.freqs,
      },
      {
        // marca del mínimo/máximo de cada núcleo
        type: "scatter",
        symbol: "rect",
        symbolSize: [18, 2],
        itemStyle: { color: C.amber },
        tooltip: { show: false },
        data: info?.freqMax ?? [],
      },
    ],
    animationDurationUpdate: 300,
  });

  let tempGauge = $derived(
    neonGauge({
      value: s.cpu.temp ?? 0,
      max: 100,
      color: tempColor(s.cpu.temp ?? 0),
      label: "PAQUETE",
      text: s.cpu.temp != null ? `${s.cpu.temp.toFixed(1)}°C` : "N/D",
      sub: s.cpu.temps.filter((x) => x.celsius !== s.cpu.temp).map((x) => `${x.label} ${x.celsius.toFixed(1)}°`).join("  "),
    }),
  );
</script>

<div class="grid">
  <div class="stats" style="grid-column: span 4">
    <Stat label="Uso total" value={fmtPct(t.usage, 1)} color={heatColor(t.usage)} history={metrics.h.cpu.usage} />
    <Stat label="Frecuencia media" value={fmtFreq(freqAvg)} color={C.violet} history={metrics.h.freqAvg} />
    <Stat label="Temperatura" value={s.cpu.temp != null ? `${s.cpu.temp.toFixed(1)}°C` : "N/D"} color={tempColor(s.cpu.temp ?? 0)} history={metrics.h.temp} />
    <Stat label="Carga 1m" value={s.cpu.loadAvg[0].toFixed(2)} sub={`5m ${s.cpu.loadAvg[1].toFixed(2)} · 15m ${s.cpu.loadAvg[2].toFixed(2)}`} color={C.lime} />
    <Stat label="Cambios ctx/s" value={fmtNum(s.cpu.ctxtRate)} color={C.cyan} history={metrics.h.ctxt} />
    <Stat label="Interrupciones/s" value={fmtNum(s.cpu.intrRate)} color={C.amber} history={metrics.h.intr} />
    <Stat label="Forks/s" value={fmtNum(s.cpu.forkRate)} color={C.magenta} history={metrics.h.fork} />
    <Stat label="Procesos" value={`${s.cpu.procsRunning}/${s.cpu.procsTotal}`} sub={`${s.cpu.procsBlocked} bloqueados`} color={C.teal} history={metrics.h.running} />
  </div>

  <Panel title="Reparto del tiempo de CPU" span={3}>
    {#snippet extra()}
      {#each BREAKDOWN as b}
        <span style:color={b.color}>{b.label} {t[b.key].toFixed(1)}</span>
      {/each}
    {/snippet}
    <TimeChart
      height={240}
      stacked
      yMax={100}
      yFmt={(v) => `${v.toFixed(0)}%`}
      series={BREAKDOWN.map((b) => ({ label: b.label, color: b.color }))}
      data={() => BREAKDOWN.map((b) => metrics.view(metrics.h.cpu[b.key]))}
    />
  </Panel>
  <Panel title="Perfil de carga" accent={C.magenta}>
    <EChart height={250} option={radar} />
  </Panel>

  <Panel title="Mapa de calor por núcleo" span={4} accent={C.violet}>
    {#snippet extra()}<span>uso de cada hilo en la ventana de tiempo</span>{/snippet}
    <Heatmap height={Math.max(160, s.cpu.cores.length * 16)} rows={() => metrics.h.cores.map((data, i) => ({ label: `C${i}`, data }))} />
  </Panel>

  <Panel title="Uso radial" accent={C.lime}>
    <EChart height={260} option={polar} />
  </Panel>
  <Panel title="Frecuencia por núcleo" span={2} accent={C.violet}>
    {#snippet extra()}<span>GHz · <span style:color={C.amber}>━</span> máx.</span>{/snippet}
    <EChart height={260} option={freqBars} />
  </Panel>
  <Panel title="Temperatura" accent={tempColor(s.cpu.temp ?? 0)}>
    <EChart height={260} option={tempGauge} />
  </Panel>

  <Panel title="Frecuencia en el tiempo" span={2} accent={C.violet}>
    <TimeChart
      height={200}
      yFmt={(v) => (v / 1000).toFixed(1) + "G"}
      series={s.cpu.freqs.map((_, i) => ({ label: `C${i}`, color: rgba(i % 2 ? C.violet : C.cyan, 0.55), width: 1 }))}
      data={() => metrics.h.freqs.map((f) => metrics.view(f))}
    />
  </Panel>
  <Panel title="Temperatura y carga" span={2} accent={C.amber}>
    {#snippet extra()}<span style:color={C.amber}>temperatura °C</span><span style:color={C.cyan}>uso %</span>{/snippet}
    <TimeChart
      height={200}
      yFmt={(v) => v.toFixed(0)}
      series={[
        { label: "°C", color: C.amber, fill: true },
        { label: "uso %", color: C.cyan, dash: [3, 3], width: 1 },
      ]}
      data={() => [metrics.view(metrics.h.temp), metrics.view(metrics.h.cpu.usage)]}
    />
  </Panel>
</div>

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: 10px;
  }
</style>
