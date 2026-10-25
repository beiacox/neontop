<script lang="ts">
  import EChart from "$lib/charts/EChart.svelte";
  import TimeChart from "$lib/charts/TimeChart.svelte";
  import { neonGauge } from "$lib/charts/echarts";
  import Panel from "$lib/ui/Panel.svelte";
  import Meter from "$lib/ui/Meter.svelte";
  import { memSegments, metrics } from "$lib/store.svelte";
  import { procs } from "$lib/procs.svelte";
  import { C, heatColor, tempColor } from "$lib/theme";
  import { fmtBytes, fmtFreq, fmtPct, fmtRate, fmtBytesOf } from "$lib/format";
  import Sparkline from "$lib/charts/Sparkline.svelte";

  $effect(() => {
    procs.want("overview", true);
    return () => procs.want("overview", false);
  });

  let s = $derived(metrics.snap!);
  let topCpu = $derived(procs.list.toSorted((a, b) => b.cpu - a.cpu).slice(0, 8));
  let topMem = $derived(procs.list.toSorted((a, b) => b.mem - a.mem).slice(0, 8));
  let seg = $derived(memSegments(s.mem));
  let freqAvg = $derived(s.cpu.freqs.reduce((a, b) => a + b, 0) / Math.max(1, s.cpu.freqs.length));
  let swapUsed = $derived(s.mem.swapTotal - s.mem.swapFree);
  let physical = $derived(s.net.ifaces.filter((i) => i.kind === "ethernet" || i.kind === "wifi"));

  /** Suma de las historias de todas las interfaces físicas. */
  function netTotals(): number[][] {
    const len = metrics.h.t.length;
    const rx = new Array(len).fill(0);
    const tx = new Array(len).fill(0);
    for (const i of physical) {
      const nh = metrics.h.net.get(i.name);
      if (!nh) continue;
      nh.rx.forEach((v, j) => (rx[j] += v));
      nh.tx.forEach((v, j) => (tx[j] += v));
    }
    return [metrics.view(rx), metrics.view(tx)];
  }

  function diskTotals(): number[][] {
    const len = metrics.h.t.length;
    const r = new Array(len).fill(0);
    const w = new Array(len).fill(0);
    for (const d of s.disks.disks) {
      if (d.kind === "virtual") continue;
      const dh = metrics.h.disks.get(d.name);
      dh?.r.forEach((v, j) => (r[j] += v));
      dh?.w.forEach((v, j) => (w[j] += v));
    }
    return [metrics.view(r), metrics.view(w)];
  }
  let physDisks = $derived(s.disks.disks.filter((d) => d.kind !== "virtual"));
  // temperatura principal de cada chip (Tctl, junction, Composite… o la primera)
  let mainTemps = $derived(
    s.sensors.chips
      .map((c) => {
        const t = c.readings.filter((r) => r.kind === "temp");
        const main = t.find((r) => ["Tctl", "Tdie", "Package id 0", "junction", "Composite"].includes(r.label)) ?? t[0];
        return main ? { label: c.label, r: main } : null;
      })
      .filter((x) => x != null),
  );

  const psiRows = $derived([
    { label: "CPU", v: s.pressure.cpu.some, c: C.cyan },
    { label: "MEM", v: s.pressure.memory.some, c: C.magenta },
    { label: "I/O", v: s.pressure.io.some, c: C.amber },
  ]);
</script>

<div class="grid">
  <Panel title="CPU" accent={C.cyan} half>
    <EChart height={170} option={neonGauge({ value: s.cpu.total.usage, color: heatColor(s.cpu.total.usage), label: "USO", text: fmtPct(s.cpu.total.usage), sub: fmtFreq(freqAvg) })} />
  </Panel>
  <Panel title="Memoria" accent={C.magenta} half>
    <EChart
      height={170}
      option={neonGauge({ value: (seg.used / s.mem.total) * 100, color: C.magenta, label: "RAM", text: fmtPct((seg.used / s.mem.total) * 100), sub: fmtBytesOf(seg.used, s.mem.total) })}
    />
  </Panel>
  <Panel title="Swap" accent={C.violet} half>
    <EChart
      height={170}
      option={neonGauge({
        value: s.mem.swapTotal ? (swapUsed / s.mem.swapTotal) * 100 : 0,
        color: C.violet,
        label: "SWAP",
        text: s.mem.swapTotal ? fmtPct((swapUsed / s.mem.swapTotal) * 100) : "N/D",
        sub: fmtBytesOf(swapUsed, s.mem.swapTotal),
      })}
    />
  </Panel>
  <Panel title="Temp. CPU" accent={tempColor(s.cpu.temp ?? 0)} half>
    <EChart
      height={170}
      option={neonGauge({ value: s.cpu.temp ?? 0, max: 100, color: tempColor(s.cpu.temp ?? 0), label: "°C", text: s.cpu.temp != null ? `${s.cpu.temp.toFixed(1)}°` : "N/D", sub: s.cpu.temps.map((t) => `${t.label} ${t.celsius.toFixed(0)}°`).join("  ") })}
    />
  </Panel>

  <Panel title="Historial CPU" span={2}>
    {#snippet extra()}<span>user · system · iowait</span>{/snippet}
    <TimeChart
      height={200}
      stacked
      yMax={100}
      yFmt={(v) => `${v.toFixed(0)}%`}
      series={[
        { label: "user", color: C.cyan },
        { label: "system", color: C.magenta },
        { label: "iowait", color: C.amber },
      ]}
      data={() => [metrics.view(metrics.h.cpu.user), metrics.view(metrics.h.cpu.system), metrics.view(metrics.h.cpu.iowait)]}
    />
  </Panel>
  <Panel title="Núcleos" span={2} accent={C.lime}>
    {#snippet extra()}<span>{metrics.info?.physicalCores} físicos · {metrics.info?.logicalCores} hilos</span>{/snippet}
    <div class="cores">
      {#each s.cpu.cores as c, i}
        <Meter label="C{i}" pct={c.usage} color={heatColor(c.usage)} right="{c.usage.toFixed(0).padStart(3)}% {fmtFreq(s.cpu.freqs[i] ?? 0)}" segments={24} />
      {/each}
    </div>
  </Panel>

  <Panel title="Memoria" span={2} accent={C.magenta}>
    {#snippet extra()}<span>apps · shmem · buffers · caché</span>{/snippet}
    <TimeChart
      height={200}
      stacked
      yMax={s.mem.total}
      yFmt={(v) => fmtBytes(v, 0)}
      series={[
        { label: "apps", color: C.magenta },
        { label: "shmem", color: C.violet },
        { label: "buffers", color: C.amber },
        { label: "caché", color: C.cyan },
      ]}
      data={() => [metrics.view(metrics.h.mem.apps), metrics.view(metrics.h.mem.shmem), metrics.view(metrics.h.mem.buffers), metrics.view(metrics.h.mem.cache)]}
    />
  </Panel>
  <Panel title="Red" span={2} accent={C.teal}>
    {#snippet extra()}
      <span style:color={C.cyan}>▼ {fmtRate(physical.reduce((a, i) => a + i.rxRate, 0))}</span>
      <span style:color={C.magenta}>▲ {fmtRate(physical.reduce((a, i) => a + i.txRate, 0))}</span>
    {/snippet}
    <TimeChart
      height={200}
      mirrored
      yFmt={(v) => fmtBytes(v, 0)}
      series={[
        { label: "bajada", color: C.cyan, fill: true },
        { label: "subida", color: C.magenta, fill: true, negate: true },
      ]}
      data={netTotals}
    />
  </Panel>

  <Panel title="Presión (PSI)" accent={C.amber}>
    {#snippet extra()}<span>avg10 · some</span>{/snippet}
    <div class="psi">
      {#each psiRows as r}
        <Meter label={r.label} pct={r.v} color={r.c} right={`${r.v.toFixed(2)}%`} segments={20} />
      {/each}
    </div>
    <p class="hint">% de tiempo en que alguna tarea estuvo esperando por el recurso.</p>
  </Panel>
  <Panel title="Carga media" accent={C.lime}>
    {#snippet extra()}<span>{s.cpu.loadAvg.map((l) => l.toFixed(2)).join(" · ")}</span>{/snippet}
    <TimeChart
      height={150}
      yFmt={(v) => v.toFixed(1)}
      series={[
        { label: "1 min", color: C.lime, fill: true },
        { label: "5 min", color: C.amber },
        { label: "15 min", color: C.red, dash: [4, 4] },
      ]}
      data={() => metrics.h.load.map((l) => metrics.view(l))}
    />
  </Panel>
  <Panel title="Sistema" span={2} accent={C.blue}>
    <dl class="sys">
      <dt>Host</dt><dd>{metrics.info?.hostname}</dd>
      <dt>SO</dt><dd>{metrics.info?.os}</dd>
      <dt>Kernel</dt><dd>{metrics.info?.kernel}</dd>
      <dt>CPU</dt><dd>{metrics.info?.cpuModel}</dd>
      <dt>Procesos</dt><dd>{s.cpu.procsTotal} total · {s.cpu.procsRunning} ejecutando · {s.cpu.procsBlocked} bloqueados</dd>
      <dt>Sockets</dt><dd>{s.net.tcpEstablished} TCP establecidos · {s.net.tcpListen} escuchando · {s.net.udpSockets} UDP</dd>
    </dl>
  </Panel>
  <Panel title="Discos" span={2} accent={C.amber}>
    {#snippet extra()}
      <span style:color={C.cyan}>▼ {fmtRate(physDisks.reduce((a, d) => a + d.readRate, 0))}</span>
      <span style:color={C.magenta}>▲ {fmtRate(physDisks.reduce((a, d) => a + d.writeRate, 0))}</span>
    {/snippet}
    <TimeChart
      height={180}
      mirrored
      yFmt={(v) => fmtBytes(v, 0)}
      series={[
        { label: "lectura", color: C.cyan, fill: true },
        { label: "escritura", color: C.magenta, fill: true, negate: true },
      ]}
      data={diskTotals}
    />
  </Panel>
  <Panel title="Temperaturas" span={2} accent={C.red}>
    <div class="temps">
      {#each mainTemps as t (t.r.id)}
        <Meter label="" pct={(t.r.value / (t.r.crit ?? 100)) * 100} color={tempColor(t.r.value)} right={`${t.r.value.toFixed(1)}°C`} segments={30} />
        <span class="tlabel">{t.label} · {t.r.label}</span>
      {/each}
    </div>
  </Panel>
  <Panel title="Top procesos" span={4} accent={C.lime}>
    {#snippet extra()}<span>por CPU (% de un núcleo) · por memoria</span>{/snippet}
    <div class="top">
      <div>
        {#each topCpu as p (p.pid)}
          <div class="trow">
            <span class="tname" title={p.cmd}>{p.name}</span>
            <span class="tpid">{p.pid}</span>
            <div class="tspark"><Sparkline values={procs.hist.get(p.pid)?.cpu ?? []} color={heatColor(p.cpu)} height={16} max={Math.max(10, ...(procs.hist.get(p.pid)?.cpu ?? [0]))} version={procs.tick} /></div>
            <Meter label="" pct={p.cpu} color={heatColor(p.cpu)} right={`${p.cpu.toFixed(1)}%`} segments={20} />
          </div>
        {/each}
      </div>
      <div>
        {#each topMem as p (p.pid)}
          <div class="trow">
            <span class="tname" title={p.cmd}>{p.name}</span>
            <span class="tpid">{p.pid}</span>
            <div class="tspark"><Sparkline values={procs.hist.get(p.pid)?.mem ?? []} color={C.magenta} height={16} version={procs.tick} /></div>
            <Meter label="" pct={(p.mem / s.mem.total) * 100 * 4} color={C.magenta} right={fmtBytes(p.mem)} segments={20} />
          </div>
        {/each}
      </div>
    </div>
  </Panel>
</div>

<style>
  .temps {
    display: grid;
    grid-template-columns: 1fr 190px;
    column-gap: 12px;
    row-gap: 8px;
    align-items: center;
    padding-top: 6px;
  }
  .temps :global(.meter) {
    grid-template-columns: 0 1fr 70px;
  }
  .tlabel {
    font-size: 11px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .top {
    display: grid;
    grid-template-columns: 1fr 1fr;
    column-gap: 28px;
  }
  .trow {
    display: grid;
    grid-template-columns: 150px 56px 90px 1fr;
    align-items: center;
    gap: 10px;
    font-size: 11px;
    height: 22px;
  }
  .tname {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tpid {
    color: var(--muted);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .trow :global(.meter) {
    grid-template-columns: 0 1fr 86px;
  }
  .cores {
    display: grid;
    grid-template-columns: 1fr 1fr;
    column-gap: 18px;
    row-gap: 4px;
    padding-top: 4px;
  }
  .psi {
    display: grid;
    gap: 10px;
    padding: 10px 0;
  }
  .hint {
    font-size: 10px;
    color: var(--muted);
    margin: 4px 0 0;
  }
  .sys {
    display: grid;
    grid-template-columns: 90px 1fr;
    gap: 6px 12px;
    margin: 4px 0 0;
    font-size: 12px;
  }
  .sys dt {
    color: var(--muted);
    text-transform: uppercase;
    font-size: 10px;
    letter-spacing: 0.1em;
    align-self: center;
  }
  .sys dd {
    margin: 0;
    color: var(--text);
  }
  @media (max-width: 700px) {
    .top {
      grid-template-columns: minmax(0, 1fr);
    }
    .trow {
      grid-template-columns: minmax(0, 1fr) 44px 150px;
      gap: 6px;
    }
    .tspark {
      display: none;
    }
    .temps {
      grid-template-columns: minmax(0, 1fr) 150px;
    }
  }
</style>
