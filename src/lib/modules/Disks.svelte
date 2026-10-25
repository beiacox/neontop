<script lang="ts">
  import EChart from "$lib/charts/EChart.svelte";
  import TimeChart from "$lib/charts/TimeChart.svelte";
  import Heatmap from "$lib/charts/Heatmap.svelte";
  import Sparkline from "$lib/charts/Sparkline.svelte";
  import { neonGauge, tooltipBase } from "$lib/charts/echarts";
  import Panel from "$lib/ui/Panel.svelte";
  import Stat from "$lib/ui/Stat.svelte";
  import { metrics } from "$lib/store.svelte";
  import { C, MONO, heatColor, rgba, tempColor } from "$lib/theme";
  import { fmtBytes, fmtNum, fmtPct, fmtRate } from "$lib/format";
  import type { DiskKind, DiskSnapshot } from "$lib/types";

  let s = $derived(metrics.snap!);
  let showVirtual = $state(false);
  let selectedName = $state<string | null>(null);

  const KIND: Record<DiskKind, { icon: string; label: string }> = {
    nvme: { icon: "▰", label: "NVMe" },
    ssd: { icon: "▭", label: "SSD SATA" },
    hdd: { icon: "◉", label: "HDD" },
    usb: { icon: "⏏", label: "USB" },
    virtual: { icon: "◇", label: "Virtual" },
  };

  let disks = $derived(s.disks.disks.filter((d) => showVirtual || d.kind !== "virtual"));
  // por defecto, el disco con más actividad acumulada
  let sel = $derived<DiskSnapshot | undefined>(
    disks.find((d) => d.name === selectedName) ?? disks.toSorted((a, b) => b.readTotal + b.writeTotal - (a.readTotal + a.writeTotal))[0],
  );
  let hist = $derived(sel ? metrics.h.disks.get(sel.name) : undefined);

  let totals = $derived.by(() => {
    const real = s.disks.disks.filter((d) => d.kind !== "virtual");
    const busiest = real.toSorted((a, b) => b.util - a.util)[0];
    const temps = real.flatMap((d) => d.temps.filter((t) => t.label === "Composite" || d.temps.length === 1).map((t) => t.celsius));
    const fsTotal = s.disks.filesystems.reduce((a, f) => a + f.total, 0);
    const fsUsed = s.disks.filesystems.reduce((a, f) => a + f.used, 0);
    return {
      read: real.reduce((a, d) => a + d.readRate, 0),
      write: real.reduce((a, d) => a + d.writeRate, 0),
      iops: real.reduce((a, d) => a + d.readIops + d.writeIops, 0),
      busiest,
      maxTemp: temps.length ? Math.max(...temps) : null,
      fsTotal,
      fsUsed,
      count: real.length,
      capacity: real.reduce((a, d) => a + d.size, 0),
    };
  });

  /** Suma de la historia de lectura/escritura de todos los discos físicos. */
  function totalSeries(key: "r" | "w"): number[] {
    const out = new Array(metrics.h.t.length).fill(0);
    for (const d of s.disks.disks) {
      if (d.kind === "virtual") continue;
      metrics.h.disks.get(d.name)?.[key].forEach((v, i) => (out[i] += v));
    }
    return metrics.view(out);
  }

  const view = (k: keyof NonNullable<typeof hist>) => () => {
    const h = sel && metrics.h.disks.get(sel.name);
    return h ? metrics.view(h[k]) : [];
  };

  // Sunburst: disco → sistema de ficheros → usado / libre
  let space = $derived.by(() => {
    const byDisk = new Map<string, typeof s.disks.filesystems>();
    for (const f of s.disks.filesystems) {
      const k = f.disk ?? f.device;
      if (!byDisk.has(k)) byDisk.set(k, []);
      byDisk.get(k)!.push(f);
    }
    const palette = [C.cyan, C.magenta, C.lime, C.amber, C.violet, C.teal, C.blue, C.orange];
    const data = [...byDisk].map(([disk, fss], i) => {
      const color = palette[i % palette.length];
      const d = s.disks.disks.find((x) => x.name === disk);
      const assigned = fss.reduce((a, f) => a + f.total, 0);
      const children = fss.map((f) => ({
        name: f.mount,
        value: f.total,
        itemStyle: { color: rgba(color, 0.7) },
        children: [
          { name: `${f.mount} · usado`, value: f.used, itemStyle: { color } },
          { name: `${f.mount} · libre`, value: Math.max(0, f.total - f.used), itemStyle: { color: rgba(color, 0.15) } },
        ],
      }));
      // espacio del disco que no está en ningún sistema de ficheros montado
      if (d && d.size > assigned * 1.02) children.push({ name: `${disk} · sin montar`, value: d.size - assigned, itemStyle: { color: C.dim }, children: [] });
      return { name: disk, value: Math.max(d?.size ?? 0, assigned), itemStyle: { color }, children };
    });
    return {
      tooltip: { ...tooltipBase, formatter: (p: any) => `${p.name}<br><b>${fmtBytes(p.value)}</b>` },
      series: [
        {
          type: "sunburst",
          data,
          radius: ["12%", "94%"],
          sort: undefined,
          itemStyle: { borderColor: C.bg, borderWidth: 1 },
          label: { color: C.text, fontFamily: MONO, fontSize: 9, minAngle: 14 },
          levels: [{}, { r0: "12%", r: "38%", label: { rotate: 0, fontSize: 10 } }, { r0: "40%", r: "74%" }, { r0: "76%", r: "94%", label: { show: false } }],
        },
      ],
    };
  });

  const fsColor = (pct: number) => (pct > 90 ? C.red : pct > 75 ? C.amber : C.cyan);
</script>

<div class="grid">
  <div class="stats" style="grid-column: span 4">
    <Stat label="Lectura" value={fmtRate(totals.read)} color={C.cyan} history={metrics.h.t.length ? totalSeries("r") : []} />
    <Stat label="Escritura" value={fmtRate(totals.write)} color={C.magenta} history={metrics.h.t.length ? totalSeries("w") : []} />
    <Stat label="IOPS" value={fmtNum(totals.iops)} sub="lecturas + escrituras/s" color={C.lime} />
    <Stat label="Más ocupado" value={totals.busiest ? fmtPct(totals.busiest.util) : "—"} sub={totals.busiest?.name} color={heatColor(totals.busiest?.util ?? 0)} />
    <Stat label="Temp. máx." value={totals.maxTemp != null ? `${totals.maxTemp.toFixed(0)}°C` : "N/D"} sub="discos con sensor" color={tempColor(totals.maxTemp ?? 0)} />
    <Stat label="Espacio usado" value={fmtPct((totals.fsUsed / Math.max(1, totals.fsTotal)) * 100)} sub={`${fmtBytes(totals.fsUsed, 0)} / ${fmtBytes(totals.fsTotal, 0)}`} color={C.amber} />
    <Stat label="Discos" value={String(totals.count)} sub={`${fmtBytes(totals.capacity, 1)} en total`} color={C.violet} />
    <Stat label="Sist. ficheros" value={String(s.disks.filesystems.length)} sub="montados" color={C.teal} />
  </div>

  <div class="cards" style="grid-column: span 4">
    {#each disks as d (d.name)}
      {@const h = metrics.h.disks.get(d.name)}
      {@const temp = d.temps.find((t) => t.label === "Composite") ?? d.temps[0]}
      <button class="card" class:active={sel?.name === d.name} onclick={() => (selectedName = d.name)}>
        <div class="top">
          <span class="icon">{KIND[d.kind].icon}</span>
          <b>{d.name}</b>
          <span class="kind">{KIND[d.kind].label}</span>
          {#if temp}<span class="temp" style:color={tempColor(temp.celsius)}>{temp.celsius.toFixed(0)}°</span>{/if}
        </div>
        <div class="model">{d.model || "—"} · {fmtBytes(d.size, 0)}</div>
        <div class="util"><i style:width="{d.util}%" style:background={heatColor(d.util)}></i></div>
        {#if h}
          <Sparkline values={h.r} color={C.cyan} height={18} />
          <Sparkline values={h.w} color={C.magenta} height={18} />
        {/if}
        <div class="rates"><span class="rx">▼{fmtRate(d.readRate)}</span><span class="tx">▲{fmtRate(d.writeRate)}</span><span>{d.util.toFixed(0)}%</span></div>
      </button>
    {/each}
    <label class="toggle"><input type="checkbox" bind:checked={showVirtual} /> mostrar virtuales (zram…)</label>
  </div>

  {#if sel}
    <Panel title="E/S · {sel.name}" span={3} accent={C.cyan}>
      {#snippet extra()}
        <span style:color={C.cyan}>▼ lectura {fmtRate(sel.readRate)}</span>
        <span style:color={C.magenta}>▲ escritura {fmtRate(sel.writeRate)}</span>
      {/snippet}
      {#key sel.name}
        <TimeChart
          height={240}
          mirrored
          yFmt={(v) => fmtBytes(v, 0) + "/s"}
          series={[
            { label: "lectura", color: C.cyan, fill: true, width: 2 },
            { label: "escritura", color: C.magenta, fill: true, width: 2, negate: true },
          ]}
          data={() => [view("r")(), view("w")()]}
        />
      {/key}
    </Panel>
    <Panel title="Ocupación" accent={heatColor(sel.util)}>
      <EChart
        height={250}
        option={neonGauge({ value: sel.util, color: heatColor(sel.util), label: "TIEMPO OCUPADO", text: fmtPct(sel.util, 1), sub: `cola ${sel.queue.toFixed(2)}` })}
      />
    </Panel>

    <Panel title="Operaciones por segundo" span={2} accent={C.lime}>
      {#key sel.name}
        <TimeChart
          height={200}
          mirrored
          yFmt={(v) => fmtNum(v)}
          series={[
            { label: "lecturas/s", color: C.lime, fill: true },
            { label: "escrituras/s", color: C.amber, fill: true, negate: true },
          ]}
          data={() => [view("riops")(), view("wiops")()]}
        />
      {/key}
    </Panel>
    <Panel title="Latencia y cola" span={2} accent={C.amber}>
      {#snippet extra()}<span>ms por operación · profundidad media de cola</span>{/snippet}
      {#key sel.name}
        <TimeChart
          height={200}
          yFmt={(v) => v.toFixed(1)}
          series={[
            { label: "lectura ms", color: C.cyan },
            { label: "escritura ms", color: C.magenta },
            { label: "cola", color: C.amber, dash: [4, 3], width: 1 },
          ]}
          data={() => [view("rlat")(), view("wlat")(), view("queue")()]}
        />
      {/key}
    </Panel>
  {/if}

  <Panel title="Mapa de actividad" span={4} accent={C.violet}>
    {#snippet extra()}<span>% de tiempo ocupado de cada disco</span>{/snippet}
    <Heatmap
      height={Math.max(90, disks.length * 22)}
      labelWidth={70}
      rows={() => disks.map((d) => ({ label: d.name, data: metrics.h.disks.get(d.name)?.util ?? [] }))}
    />
  </Panel>

  <Panel title="Sistemas de ficheros" span={2} accent={C.amber}>
    <div class="fslist">
      {#each s.disks.filesystems as f (f.device)}
        {@const pct = (f.used / Math.max(1, f.total)) * 100}
        <div class="fs" title={f.otherMounts.length ? `También en: ${f.otherMounts.join(", ")}` : f.device}>
          <div class="fstop">
            <b>{f.mount}</b>
            <span class="dim">{f.device} · {f.fstype}{f.readonly ? " · solo lectura" : ""}{f.otherMounts.length ? ` · +${f.otherMounts.length} montajes` : ""}</span>
            <span class="pct" style:color={fsColor(pct)}>{pct.toFixed(0)}%</span>
          </div>
          <div class="fsbar"><i style:width="{pct}%" style:background={fsColor(pct)}></i></div>
          <div class="fsbot dim">
            <span>{fmtBytes(f.used)} usados</span>
            <span>{fmtBytes(f.avail)} libres de {fmtBytes(f.total)}</span>
            {#if f.inodesTotal}<span>inodos {fmtPct(((f.inodesTotal - f.inodesFree) / f.inodesTotal) * 100)}</span>{/if}
          </div>
        </div>
      {/each}
    </div>
  </Panel>
  <Panel title="Espacio por disco" span={2} accent={C.magenta}>
    {#snippet extra()}<span>disco › montaje › usado/libre</span>{/snippet}
    <EChart height={360} option={space} />
  </Panel>

  <Panel title="E/S total" span={4} accent={C.teal}>
    {#snippet extra()}<span>todos los discos físicos</span>{/snippet}
    <TimeChart
      height={180}
      mirrored
      yFmt={(v) => fmtBytes(v, 0) + "/s"}
      series={[
        { label: "lectura", color: C.cyan, fill: true },
        { label: "escritura", color: C.magenta, fill: true, negate: true },
      ]}
      data={() => [totalSeries("r"), totalSeries("w")]}
    />
  </Panel>
</div>

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: 10px;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 10px;
  }
  .card {
    text-align: left;
    background: var(--panel);
    border: 1px solid color-mix(in srgb, var(--cyan) 18%, transparent);
    padding: 8px 10px;
    color: var(--text);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .card.active {
    border-color: var(--cyan);
    background: color-mix(in srgb, var(--cyan) 8%, var(--panel));
  }
  .top {
    display: flex;
    align-items: baseline;
    gap: 6px;
  }
  .icon {
    color: var(--cyan);
  }
  .kind {
    color: var(--muted);
    font-size: 9px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }
  .temp {
    margin-left: auto;
    font-weight: 700;
  }
  .model {
    color: var(--muted);
    font-size: 10px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .util {
    height: 4px;
    background: color-mix(in srgb, var(--cyan) 8%, transparent);
    margin: 2px 0;
  }
  .util i {
    display: block;
    height: 100%;
  }
  .rates {
    display: flex;
    gap: 8px;
    font-size: 10px;
  }
  .rates span:last-child {
    margin-left: auto;
    color: var(--muted);
  }
  .rx {
    color: var(--cyan);
  }
  .tx {
    color: var(--magenta);
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--muted);
    cursor: pointer;
    align-self: end;
  }
  .toggle input {
    accent-color: var(--cyan);
  }
  .fslist {
    display: grid;
    gap: 12px;
  }
  .fstop,
  .fsbot {
    display: flex;
    gap: 10px;
    align-items: baseline;
    font-size: 11px;
  }
  .fstop .pct {
    margin-left: auto;
    font-weight: 700;
  }
  .fsbot {
    font-size: 10px;
    margin-top: 3px;
  }
  .fsbar {
    height: 8px;
    margin-top: 4px;
    background: color-mix(in srgb, var(--cyan) 7%, transparent);
  }
  .fsbar i {
    display: block;
    height: 100%;
  }
  .dim {
    color: var(--muted);
  }
</style>
