<script lang="ts">
  /** Paneles del SAI en la pestaña Energía (se colocan directamente en la rejilla). */
  import EChart from "$lib/charts/EChart.svelte";
  import TimeChart from "$lib/charts/TimeChart.svelte";
  import { neonGauge } from "$lib/charts/echarts";
  import Panel from "$lib/ui/Panel.svelte";
  import { metrics } from "$lib/store.svelte";
  import { fmtWatts } from "$lib/energy.svelte";
  import { fmtDuration } from "$lib/format";
  import { C } from "$lib/theme";
  import type { UpsSnapshot } from "$lib/types";

  interface Props {
    ups: UpsSnapshot;
    /** W medidos por componentes (CPU, GPU…) y el resto hasta el total del SAI. */
    parts: number;
    rest: number;
  }
  let { ups, parts, rest }: Props = $props();

  let st = $derived(
    ups.lowBattery
      ? { text: "BATERÍA BAJA", color: C.red }
      : ups.onBattery
        ? { text: "EN BATERÍA", color: C.amber }
        : { text: ups.status.includes("CHARG") ? "EN RED · CARGANDO" : "EN RED", color: C.lime },
  );
  let power = $derived(ups.powerW ?? 0);
  let nominal = $derived(ups.nominalW ?? 0);
  let step = $derived(nominal ? nominal / 100 : 0);
  const n = (v: number | null, d = 0, u = "") => (v == null ? "—" : `${v.toFixed(d)}${u}`);
</script>

<Panel title="SAI · {ups.model}" span={2} accent={st.color}>
  {#snippet extra()}<span class="badge" style:--c={st.color}>{st.text}</span>{/snippet}
  <div class="main">
    <div>
      <span class="lbl">consumo del puesto</span>
      <b class="big" style:color={st.color}>{fmtWatts(power)}</b>
      <span class="sub">{n(ups.loadPct, 0, " %")} de {n(nominal, 0, " W")}{step ? ` · resolución ${step.toFixed(0)} W` : ""}</span>
    </div>
    <div class="split">
      <div class="bar">
        <i style:width="{power ? (Math.min(parts, power) / power) * 100 : 0}%" style:background={C.cyan}></i>
        <i style:width="{power ? (rest / power) * 100 : 0}%" style:background={C.violet}></i>
      </div>
      <span><i style:color={C.cyan}>■</i> PC medido (CPU, GPU) {fmtWatts(parts)}</span>
      <span><i style:color={C.violet}>■</i> resto: placa, RAM, discos, monitores, pérdidas… {fmtWatts(rest)}</span>
    </div>
  </div>
  <dl>
    <dt>Batería</dt><dd>{n(ups.batteryPct, 0, " %")} · {n(ups.batteryV, 1, " V")}{ups.batteryNominalV ? ` (nominal ${ups.batteryNominalV} V)` : ""}</dd>
    <dt>Autonomía</dt><dd>{ups.runtimeMin != null ? `${ups.runtimeMin.toFixed(0)} min con esta carga` : "—"}</dd>
    <dt>Red</dt><dd>{n(ups.lineV, 1, " V")}{ups.lineHz ? ` · ${ups.lineHz} Hz` : ""}{ups.outputV ? ` · salida ${ups.outputV} V` : ""}</dd>
    <dt>Cortes</dt>
    <dd>
      {ups.transfers ?? 0} desde que arrancó apcupsd{ups.lastTransfer && ups.transfers ? ` · último: ${ups.lastTransfer}` : ""}
      {#if ups.onBatteryTotalS}· {fmtDuration(ups.onBatteryTotalS)} en batería{/if}
    </dd>
    {#if ups.batteryDate}<dt>Batería de</dt><dd>{ups.batteryDate}</dd>{/if}
    {#if ups.tempC != null}<dt>Temp.</dt><dd>{ups.tempC.toFixed(1)} °C</dd>{/if}
  </dl>
</Panel>
<Panel title="Carga del SAI" accent={C.amber} half>
  <EChart height={210} option={neonGauge({ value: power, max: nominal || 100, color: (ups.loadPct ?? 0) > 80 ? C.red : C.amber, label: `MÁX ${nominal.toFixed(0)} W`, text: fmtWatts(power), sub: n(ups.loadPct, 0, " %") })} />
</Panel>
<Panel title="Batería" accent={st.color} half>
  <EChart
    height={210}
    option={neonGauge({ value: ups.batteryPct ?? 0, color: (ups.batteryPct ?? 100) < 30 ? C.red : C.lime, label: "BATERÍA", text: n(ups.batteryPct, 0, "%"), sub: ups.runtimeMin != null ? `${ups.runtimeMin.toFixed(0)} min` : undefined })}
  />
</Panel>
<Panel title="Red eléctrica" span={4} accent={C.cyan}>
  {#snippet extra()}<span style:color={C.cyan}>tensión de entrada (V)</span>{/snippet}
  <TimeChart height={140} yMin={Math.max(0, Math.round(ups.lineV ?? 120) - 8)} yMax={Math.round(ups.lineV ?? 120) + 8} yFmt={(v) => `${v.toFixed(0)} V`} series={[{ label: "red", color: C.cyan, width: 1.6 }]} data={() => [metrics.view(metrics.h.ups.lineV)]} />
</Panel>

<style>
  .badge {
    color: var(--c);
    border: 1px solid var(--c);
    padding: 0 6px;
    font-size: 10px;
    letter-spacing: 0.1em;
  }
  .main {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 18px;
    align-items: center;
    margin-bottom: 10px;
  }
  .main > div:first-child {
    display: grid;
  }
  .lbl {
    font-size: 10px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.12em;
  }
  .big {
    font-family: var(--display);
    font-size: 32px;
    font-variant-numeric: tabular-nums;
  }
  .sub {
    font-size: 11px;
    color: var(--muted);
  }
  .split {
    display: grid;
    gap: 4px;
    font-size: 11px;
    min-width: 0;
  }
  .split i {
    font-style: normal;
  }
  .bar {
    display: flex;
    height: 10px;
    background: color-mix(in srgb, var(--cyan) 6%, transparent);
    margin-bottom: 4px;
  }
  .bar i {
    display: block;
    height: 100%;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 5px 12px;
    margin: 0;
    font-size: 12px;
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
  @media (max-width: 700px) {
    .main {
      grid-template-columns: 1fr;
      gap: 8px;
    }
  }
</style>
