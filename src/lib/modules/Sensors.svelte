<script lang="ts">
  import EChart from "$lib/charts/EChart.svelte";
  import TimeChart from "$lib/charts/TimeChart.svelte";
  import Sparkline from "$lib/charts/Sparkline.svelte";
  import { neonGauge, tooltipBase } from "$lib/charts/echarts";
  import Panel from "$lib/ui/Panel.svelte";
  import Stat from "$lib/ui/Stat.svelte";
  import { metrics } from "$lib/store.svelte";
  import { C, MONO, rgba, tempColor } from "$lib/theme";
  import type { SensorChip, SensorReading } from "$lib/types";

  let s = $derived(metrics.snap!);
  let chips = $derived(s.sensors.chips);

  const CAT_COLOR = () => ({ cpu: C.cyan, gpu: C.magenta, disk: C.amber, network: C.lime, board: C.violet, other: C.teal });
  const CAT_ICON = { cpu: "▦", gpu: "▣", disk: "▰", network: "◉", board: "▤", other: "✺" };
  const UNIT: Record<string, string> = { temp: "°C", fan: " RPM", in: " V", power: " W", curr: " A", pwm: "%" };

  /** Temperaturas "principales": se omiten los sensores internos redundantes de los NVMe. */
  let temps = $derived(
    chips.flatMap((c) =>
      c.readings
        .filter((r) => r.kind === "temp" && !(c.category === "disk" && /^Sensor \d+$/.test(r.label) && c.readings.some((x) => x.label === "Composite")))
        .map((r) => ({ chip: c, r, name: `${c.label} · ${r.label}` })),
    ),
  );
  let allTemps = $derived(chips.flatMap((c) => c.readings.filter((r) => r.kind === "temp")));
  let hottest = $derived(temps.toSorted((a, b) => b.r.value - a.r.value)[0]);
  const find = (cat: string, labels: string[]) => {
    const c = chips.find((x) => x.category === cat);
    return labels.map((l) => c?.readings.find((r) => r.label === l)).find(Boolean) ?? c?.readings.find((r) => r.kind === "temp");
  };
  let cpuT = $derived(find("cpu", ["Tctl", "Tdie", "Package id 0"]));
  let gpuT = $derived(find("gpu", ["junction", "edge"]));
  let fans = $derived(chips.flatMap((c) => c.readings.filter((r) => r.kind === "fan").map((r) => ({ chip: c, r }))));
  let power = $derived(chips.flatMap((c) => c.readings.filter((r) => r.kind === "power").map((r) => ({ chip: c, r }))));
  let hasBoard = $derived(chips.some((c) => c.category === "board"));

  /** Límite para normalizar: crítico, máximo o 100 °C. */
  const limit = (r: SensorReading) => r.crit ?? r.max ?? 100;

  // Radar: cada temperatura como % de su límite crítico
  let radar = $derived({
    tooltip: {
      ...tooltipBase,
      formatter: () => temps.map((t) => `${t.name}: <b>${t.r.value.toFixed(1)}°C</b> / ${limit(t.r).toFixed(0)}°`).join("<br>"),
    },
    radar: {
      indicator: temps.map((t) => ({ name: `${t.chip.label.replace(/^NVMe /, "")}\n${t.r.label}`, max: 100 })),
      radius: "62%",
      center: ["50%", "54%"],
      splitNumber: 4,
      axisName: { color: C.muted, fontFamily: MONO, fontSize: 9 },
      splitLine: { lineStyle: { color: [rgba(C.cyan, 0.12), rgba(C.cyan, 0.12), rgba(C.amber, 0.3), rgba(C.red, 0.5)] } },
      splitArea: { areaStyle: { color: [rgba(C.cyan, 0.02), rgba(C.cyan, 0.04), rgba(C.amber, 0.05), rgba(C.red, 0.07)] } },
      axisLine: { lineStyle: { color: rgba(C.cyan, 0.15) } },
    },
    series: [
      {
        type: "radar",
        symbol: "circle",
        symbolSize: 5,
        data: [
          {
            value: temps.map((t) => Math.min(100, (t.r.value / limit(t.r)) * 100)),
            lineStyle: { color: C.amber, width: 2 },
            itemStyle: { color: C.amber },
            areaStyle: { color: rgba(C.amber, 0.22) },
          },
        ],
      },
    ],
  });

  // Barras de todas las temperaturas con su margen hasta el límite
  let bars = $derived({
    tooltip: { ...tooltipBase, trigger: "axis", axisPointer: { type: "shadow" }, valueFormatter: (v: number) => `${v.toFixed(1)}°C` },
    grid: { left: 150, right: 60, top: 4, bottom: 4 },
    xAxis: { type: "value", max: 110, show: false },
    yAxis: {
      type: "category",
      inverse: true,
      data: temps.map((t) => t.name),
      axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 10, width: 140, overflow: "truncate" },
      axisLine: { show: false },
      axisTick: { show: false },
    },
    series: [
      {
        type: "bar",
        barWidth: 10,
        showBackground: true,
        backgroundStyle: { color: rgba(C.cyan, 0.04) },
        label: { show: true, position: "right", color: C.text, fontFamily: MONO, fontSize: 10, formatter: (p: any) => `${p.value.toFixed(1)}°` },
        data: temps.map((t) => ({
          value: t.r.value,
          itemStyle: {
            color: { type: "linear", x: 0, y: 0, x2: 1, y2: 0, colorStops: [{ offset: 0, color: rgba(tempColor(t.r.value), 0.3) }, { offset: 1, color: tempColor(t.r.value) }] },
          },
        })),
      },
      {
        // marca del límite crítico
        type: "scatter",
        symbol: "rect",
        symbolSize: [2, 14],
        itemStyle: { color: C.red },
        tooltip: { show: false },
        // en barras horizontales hay que dar [valor, categoría] explícitamente
        data: temps.filter((t) => t.r.crit ?? t.r.max).map((t) => [t.r.crit ?? t.r.max, t.name]),
      },
    ],
  });

  const palette = () => [C.cyan, C.magenta, C.amber, C.lime, C.violet, C.teal, C.blue, C.orange, C.red];
  let tempSeries = $derived(temps.map((t, i) => ({ label: t.name, color: palette()[i % palette().length], width: 1.4 })));
  let tempIds = $derived(temps.map((t) => t.r.id));

  function fmtReading(r: SensorReading) {
    const d = r.kind === "in" || r.kind === "curr" ? 3 : r.kind === "temp" ? 1 : 0;
    return `${r.value.toFixed(d)}${UNIT[r.kind]}`;
  }
  /** % para la barra de cada lectura. */
  function pctOf(r: SensorReading): number {
    if (r.kind === "temp") return (r.value / limit(r)) * 100;
    if (r.kind === "pwm") return r.value;
    if (r.max) return (r.value / r.max) * 100;
    return 0;
  }
  function readingColor(c: SensorChip, r: SensorReading) {
    return r.kind === "temp" ? tempColor(r.value) : CAT_COLOR()[c.category];
  }
</script>

<div class="grid">
  <div class="stats" style="grid-column: span 4">
    <Stat label="CPU" value={cpuT ? `${cpuT.value.toFixed(1)}°C` : "N/D"} sub={cpuT?.label} color={tempColor(cpuT?.value ?? 0)} history={cpuT ? metrics.h.sensors.get(cpuT.id) : undefined} />
    <Stat label="GPU" value={gpuT ? `${gpuT.value.toFixed(1)}°C` : "N/D"} sub={gpuT?.label} color={tempColor(gpuT?.value ?? 0)} history={gpuT ? metrics.h.sensors.get(gpuT.id) : undefined} />
    <Stat label="Más caliente" value={hottest ? `${hottest.r.value.toFixed(1)}°C` : "N/D"} sub={hottest?.name} color={tempColor(hottest?.r.value ?? 0)} />
    <Stat label="Media" value={temps.length ? `${(temps.reduce((a, t) => a + t.r.value, 0) / temps.length).toFixed(1)}°C` : "N/D"} sub={`${allTemps.length} sensores de temperatura`} color={C.teal} />
    {#each fans.slice(0, 2) as f}
      <Stat label="Ventilador {f.chip.label}" value={`${f.r.value.toFixed(0)} RPM`} sub={f.r.max ? `máx. ${f.r.max} RPM` : f.r.label} color={C.cyan} history={metrics.h.sensors.get(f.r.id)} />
    {/each}
    {#each power.slice(0, 2) as p}
      <Stat label="Potencia {p.chip.label}" value={`${p.r.value.toFixed(1)} W`} sub={p.r.label} color={C.orange} history={metrics.h.sensors.get(p.r.id)} />
    {/each}
    <Stat label="Chips" value={String(chips.length)} sub={chips.map((c) => c.driver).join(", ")} color={C.violet} />
  </div>

  <Panel title="Mapa térmico" span={2} accent={C.amber}>
    {#snippet extra()}<span>% del límite crítico de cada sensor</span>{/snippet}
    <EChart height={320} option={radar} />
  </Panel>
  <Panel title="Temperaturas" span={2} accent={C.red}>
    {#snippet extra()}<span><span style:color={C.red}>▎</span> límite crítico</span>{/snippet}
    <EChart height={320} option={bars} />
  </Panel>

  <Panel title="Historial de temperaturas" span={4} accent={C.amber}>
    {#snippet extra()}
      {#each tempSeries as t}<span style:color={t.color}>{t.label}</span>{/each}
    {/snippet}
    {#key tempIds.join()}
      <TimeChart height={240} yFmt={(v) => `${v.toFixed(0)}°`} series={tempSeries} data={() => tempIds.map((id) => metrics.view(metrics.h.sensors.get(id) ?? []))} />
    {/key}
  </Panel>

  <div class="chips" style="grid-column: span 4">
    {#each chips as c (c.id)}
      <Panel title="{CAT_ICON[c.category]} {c.label}" accent={CAT_COLOR()[c.category]}>
        {#snippet extra()}<span>{c.driver}</span>{/snippet}
        <div class="readings">
          {#each c.readings as r (r.id)}
            {@const pct = pctOf(r)}
            {@const color = readingColor(c, r)}
            <div class="reading">
              <span class="rl">{r.label}</span>
              <div class="spark"><Sparkline values={metrics.h.sensors.get(r.id) ?? []} {color} height={18} /></div>
              <b style:color={color}>{fmtReading(r)}</b>
              {#if pct > 0}
                <div class="bar"><i style:width="{Math.min(100, pct)}%" style:background={color}></i></div>
              {/if}
              {#if r.crit || r.max}
                <span class="lim">{r.max ? `máx ${r.max.toFixed(0)}${UNIT[r.kind]}` : ""}{r.crit ? ` · crít ${r.crit.toFixed(0)}${UNIT[r.kind]}` : ""}</span>
              {/if}
            </div>
          {/each}
        </div>
      </Panel>
    {/each}
  </div>

  {#if fans.length || power.length}
    <Panel title="Ventiladores y potencia" span={2} accent={C.cyan}>
      {#snippet extra()}
        {#each fans as f, i}<span style:color={[C.cyan, C.teal, C.blue][i % 3]}>{f.chip.label} {f.r.label} RPM</span>{/each}
        {#each power as p, i}<span style:color={[C.orange, C.amber][i % 2]}>{p.chip.label} {p.r.label} W</span>{/each}
      {/snippet}
      <TimeChart
        height={200}
        yFmt={(v) => v.toFixed(0)}
        series={[
          ...fans.map((f, i) => ({ label: `${f.chip.label} ${f.r.label} RPM`, color: [C.cyan, C.teal, C.blue][i % 3] })),
          ...power.map((p, i) => ({ label: `${p.chip.label} ${p.r.label} W`, color: [C.orange, C.amber][i % 2], dash: [4, 3] })),
        ]}
        data={() => [...fans, ...power].map((x) => metrics.view(metrics.h.sensors.get(x.r.id) ?? []))}
      />
    </Panel>
  {/if}
  {#if gpuT || cpuT}
    <Panel title="Temp. CPU" accent={tempColor(cpuT?.value ?? 0)} half>
      <EChart height={200} option={neonGauge({ value: cpuT?.value ?? 0, max: cpuT ? limit(cpuT) : 100, color: tempColor(cpuT?.value ?? 0), label: cpuT?.label ?? "CPU", text: cpuT ? `${cpuT.value.toFixed(1)}°` : "N/D" })} />
    </Panel>
    <Panel title="Temp. GPU" accent={tempColor(gpuT?.value ?? 0)} half>
      <EChart height={200} option={neonGauge({ value: gpuT?.value ?? 0, max: gpuT ? limit(gpuT) : 100, color: tempColor(gpuT?.value ?? 0), label: gpuT?.label ?? "GPU", text: gpuT ? `${gpuT.value.toFixed(1)}°` : "N/D" })} />
    </Panel>
  {/if}

  {#each s.sensors.batteries as b}
    <Panel title="Batería {b.name}" span={2} accent={C.lime}>
      {#snippet extra()}<span>{b.status}{s.sensors.acOnline ? " · conectado a corriente" : ""}</span>{/snippet}
      <div class="battery">
        <EChart height={180} option={neonGauge({ value: b.capacity, color: b.capacity < 20 ? C.red : C.lime, label: "CARGA", text: `${b.capacity}%`, sub: b.power != null ? `${b.power.toFixed(1)} W` : undefined })} />
        <dl>
          <dt>Energía</dt><dd>{b.energyNow?.toFixed(1) ?? "?"} / {b.energyFull?.toFixed(1) ?? "?"} Wh</dd>
          <dt>Salud</dt><dd>{b.energyFull && b.energyDesign ? `${((b.energyFull / b.energyDesign) * 100).toFixed(0)}% del diseño` : "N/D"}</dd>
          <dt>Ciclos</dt><dd>{b.cycles ?? "N/D"}</dd>
        </dl>
      </div>
    </Panel>
  {/each}

  {#if !hasBoard}
    <Panel title="Sensores de placa base" span={4} accent={C.muted}>
      <p class="hint">
        No se detectan sensores de la placa base (ventiladores de caja, voltajes, temperatura del chipset). Suelen requerir un módulo del kernel
        según el chip: <code>nct6775</code> (Nuvoton, la mayoría de placas ASUS/MSI/ASRock), <code>it87</code> (ITE, Gigabyte) o <code>asus_ec_sensors</code>.
        Puedes detectarlo con <code>sudo sensors-detect</code> (paquete lm_sensors); NeonTop los mostrará automáticamente al cargarse.
      </p>
    </Panel>
  {/if}
</div>

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: 10px;
  }
  .chips {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 14px;
  }
  .readings {
    display: grid;
    gap: 7px;
  }
  .reading {
    display: grid;
    grid-template-columns: 90px 1fr 80px;
    column-gap: 10px;
    align-items: center;
    font-size: 11px;
  }
  .rl {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .reading b {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .bar {
    grid-column: 1 / -1;
    height: 3px;
    background: color-mix(in srgb, var(--cyan) 7%, transparent);
    margin-top: 2px;
  }
  .bar i {
    display: block;
    height: 100%;
  }
  .lim {
    grid-column: 1 / -1;
    font-size: 9px;
    color: var(--muted);
    text-align: right;
  }
  .battery {
    display: grid;
    grid-template-columns: 200px 1fr;
    align-items: center;
  }
  dl {
    display: grid;
    grid-template-columns: 70px 1fr;
    gap: 6px 10px;
    font-size: 12px;
  }
  dt {
    color: var(--muted);
    font-size: 10px;
    text-transform: uppercase;
  }
  dd {
    margin: 0;
  }
  .hint {
    font-size: 11px;
    color: var(--muted);
    margin: 0;
    line-height: 1.6;
  }
  code {
    color: var(--cyan);
  }
</style>
