<script lang="ts">
  import EChart from "$lib/charts/EChart.svelte";
  import TimeChart from "$lib/charts/TimeChart.svelte";
  import { neonGauge, tooltipBase } from "$lib/charts/echarts";
  import Panel from "$lib/ui/Panel.svelte";
  import Stat from "$lib/ui/Stat.svelte";
  import { metrics } from "$lib/store.svelte";
  import { bench } from "$lib/bench.svelte";
  import { C, MONO, heatColor, rgba, tempColor } from "$lib/theme";
  import { fmtBytes, fmtPct, fmtRate } from "$lib/format";
  import { want } from "$lib/collect";
  import type { GpuProc } from "$lib/types";

  // el uso por proceso (fdinfo / NVML) solo se mide con esta pestaña abierta
  $effect(() => {
    want("gpu_procs", "gpu-tab", true);
    return () => want("gpu_procs", "gpu-tab", false);
  });

  /** Nombres legibles de los motores de cada driver. */
  const ENGINE_NAMES: Record<string, string> = {
    gfx: "Gráficos 3D", "3d": "Gráficos 3D", render: "Render 3D", rcs: "Render 3D",
    compute: "Cómputo", ccs: "Cómputo",
    dec: "Decodificación", enc: "Codificación", enc_1: "Codificación 2", vcn: "Vídeo (VCN)", jpeg: "JPEG",
    video: "Vídeo", vcs: "Vídeo", "video-enhance": "Mejora de vídeo", vecs: "Mejora de vídeo",
    dma: "Copia (DMA)", copy: "Copia", bcs: "Copia",
  };
  const engName = (e: string) => ENGINE_NAMES[e] ?? e;
  const VENDOR = { amd: { label: "AMD", color: "#ed1c24" }, nvidia: { label: "NVIDIA", color: "#76b900" }, intel: { label: "Intel", color: "#0071c5" } };

  let procSort = $state<"vram" | "busy">("vram");
  const procBusy = (p: GpuProc) => p.engines.reduce((a, e) => a + e.busy, 0);

  let idx = $state(0);
  let gpus = $derived(metrics.snap!.gpus);
  let g = $derived(gpus[Math.min(idx, gpus.length - 1)]);
  let h = $derived(metrics.h.gpus[Math.min(idx, gpus.length - 1)]);

  const gbs = (v: number | null | undefined, d = 1) => (v == null ? "N/D" : `${(v / 1e9).toFixed(d)} GB/s`);
  const pcieGen = (gt: number | null) => (gt == null ? "?" : gt >= 64 ? "6.0" : gt >= 32 ? "5.0" : gt >= 16 ? "4.0" : gt >= 8 ? "3.0" : gt >= 5 ? "2.0" : "1.0");
  let engineNames = $derived(g ? g.engines.map((e) => e.name) : []);
  let procList = $derived(
    g ? g.processes.toSorted((a, b) => (procSort === "vram" ? b.vram + b.gtt - (a.vram + a.gtt) : procBusy(b) - procBusy(a))) : [],
  );
  /** Motores que aparecen en algún proceso (columnas de la tabla). */
  let procEngines = $derived([...new Set(procList.flatMap((p) => p.engines.map((e) => e.name)))].sort());

  // Anillo: VRAM por proceso (los 8 mayores; el resto agrupado)
  const VRAM_COLORS = () => [C.magenta, C.cyan, C.lime, C.amber, C.violet, C.teal, C.blue, C.orange];
  let vramRows = $derived.by(() => {
    const all = procList.filter((p) => p.vram > 0).toSorted((a, b) => b.vram - a.vram);
    const rows = all.slice(0, 7).map((p, i) => ({ name: p.name || "?", pid: p.pid as number | null, value: p.vram, color: VRAM_COLORS()[i] }));
    const rest = all.slice(7).reduce((a, p) => a + p.vram, 0);
    if (rest > 0) rows.push({ name: `otros ${all.length - 7}`, pid: null, value: rest, color: C.muted });
    return rows;
  });
  let vramTotalProcs = $derived(vramRows.reduce((a, r) => a + r.value, 0));
  let vramRose = $derived({
    tooltip: { ...tooltipBase, formatter: (p: any) => `${p.name}<br><b>${fmtBytes(p.value)}</b> (${p.percent.toFixed(1)}%)` },
    series: [
      {
        type: "pie",
        radius: ["52%", "80%"],
        center: ["50%", "50%"],
        itemStyle: { borderColor: C.bg, borderWidth: 2 },
        label: { show: false },
        data: vramRows.map((r) => ({ name: r.pid != null ? `${r.name} (${r.pid})` : r.name, value: r.value, itemStyle: { color: r.color } })),
      },
    ],
    graphic: {
      type: "text",
      left: "center",
      top: "middle",
      style: { text: fmtBytes(vramTotalProcs), fill: C.text, font: `700 12px ${MONO}`, textAlign: "center" },
    },
  });

  let powerGauge = $derived(
    neonGauge({
      value: g?.power ?? 0,
      max: g?.powerLimit ?? Math.max(100, (g?.power ?? 0) * 1.5),
      color: C.orange,
      label: "POTENCIA",
      text: g?.power != null ? `${g.power.toFixed(0)} W` : "N/D",
      sub: g?.powerLimit ? `límite ${g.powerLimit.toFixed(0)} W` : undefined,
    }),
  );

  let bwPct = $derived(g?.vramBw != null && g.vramPeakBw ? (g.vramBw / g.vramPeakBw) * 100 : 0);

  // Comparativa de anchos de banda del sistema (escala logarítmica: van de GB/s a cientos de GB/s)
  let compare = $derived.by(() => {
    const rows: { name: string; v: number; c: string }[] = [];
    if (g?.vramPeakBw) rows.push({ name: "VRAM teórico", v: g.vramPeakBw, c: rgba(C.magenta, 0.5) });
    if (g?.vramBw != null) rows.push({ name: "VRAM ahora", v: g.vramBw, c: C.magenta });
    if (g?.pciePeakBw) rows.push({ name: "PCIe (por sentido)", v: g.pciePeakBw, c: C.amber });
    const ram = metrics.snap?.membw;
    if (ram?.total != null) rows.push({ name: "RAM ahora", v: ram.total, c: C.cyan });
    if (bench.last) {
      rows.push({ name: "RAM lectura (test)", v: bench.last.read, c: C.cyan });
      rows.push({ name: "RAM escritura (test)", v: bench.last.write, c: rgba(C.cyan, 0.6) });
    }
    return {
      tooltip: { ...tooltipBase, trigger: "axis", axisPointer: { type: "shadow" }, formatter: (p: any) => `${p[0].name}<br><b>${gbs(p[0].data.real)}</b>` },
      grid: { left: 140, right: 80, top: 6, bottom: 6 },
      xAxis: { type: "log", show: false, min: 1e8 },
      yAxis: {
        type: "category",
        inverse: true,
        data: rows.map((r) => r.name),
        axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 10 },
        axisLine: { show: false },
        axisTick: { show: false },
      },
      series: [
        {
          type: "bar",
          barWidth: 12,
          showBackground: true,
          backgroundStyle: { color: rgba(C.cyan, 0.04) },
          // escala logarítmica: la barra tiene un mínimo para verse, la etiqueta el valor real
          label: { show: true, position: "right", color: C.text, fontFamily: MONO, fontSize: 10, formatter: (p: any) => gbs(p.data.real) },
          data: rows.map((r) => ({ value: Math.max(r.v, 1.1e8), real: r.v, itemStyle: { color: r.c } })),
        },
      ],
    };
  });

  let vramDonut = $derived({
    tooltip: { ...tooltipBase, formatter: (p: any) => `${p.name}<br><b>${fmtBytes(p.value)}</b>` },
    series: [
      {
        type: "pie",
        radius: ["58%", "76%"],
        center: ["50%", "50%"],
        label: { show: false },
        data: [
          { name: "VRAM usada", value: g?.vramUsed ?? 0, itemStyle: { color: C.magenta } },
          { name: "VRAM libre", value: Math.max(0, (g?.vramTotal ?? 0) - (g?.vramUsed ?? 0)), itemStyle: { color: C.dim } },
        ],
      },
      {
        type: "pie",
        radius: ["82%", "90%"],
        label: { show: false },
        data: [
          { name: "GTT usada (RAM del sistema)", value: g?.gttUsed ?? 0, itemStyle: { color: C.amber } },
          { name: "GTT libre", value: Math.max(0, (g?.gttTotal ?? 0) - (g?.gttUsed ?? 0)), itemStyle: { color: rgba(C.amber, 0.1) } },
        ],
      },
    ],
    graphic: [
      { type: "text", left: "center", top: "42%", style: { text: fmtBytes(g?.vramUsed ?? 0), fill: C.magenta, font: `700 16px ${MONO}` } },
      { type: "text", left: "center", top: "53%", style: { text: `de ${fmtBytes(g?.vramTotal ?? 0, 0)}`, fill: C.muted, font: `10px ${MONO}` } },
    ],
  });
</script>

{#if !g}
  <div class="empty">
    <p>No se ha detectado ninguna GPU compatible.</p>
    <p class="hint">NeonTop soporta AMD (amdgpu), NVIDIA (driver propietario, vía NVML) e Intel (i915/xe).</p>
  </div>
{:else}
  <div class="grid">
    <div class="picker" style="grid-column: span 4">
      {#each gpus as x, i}
        <button class:on={i === idx} onclick={() => (idx = i)}>
          <span class="vendor" style:background={VENDOR[x.vendor]?.color}>{VENDOR[x.vendor]?.label ?? x.vendor}</span>
          {x.name}
          <small>{x.integrated ? "integrada · " : ""}{x.driver}{x.driverVersion ? ` ${x.driverVersion}` : ""} · {x.pciSlot}</small>
        </button>
      {/each}
    </div>

    <div class="stats" style="grid-column: span 4">
      <Stat label="Uso GPU" value={g.busy != null ? fmtPct(g.busy) : "N/D"} color={heatColor(g.busy ?? 0)} history={h?.busy} />
      <Stat label="Controlador VRAM" value={g.memBusy != null ? fmtPct(g.memBusy) : "N/D"} sub="actividad del bus de memoria" color={C.violet} history={h?.memBusy} />
      <Stat label="Tráfico VRAM" value={gbs(g.vramBw, 2)} sub="estimado" color={C.magenta} history={h?.vramBw} />
      <Stat label={g.integrated ? "Memoria GPU" : "VRAM usada"} value={fmtBytes(g.vramUsed)} sub={g.vramTotal ? `de ${fmtBytes(g.vramTotal, 0)}` : "suma de procesos"} color={C.cyan} history={h?.vramUsed} />
      <Stat label="GTT" value={fmtBytes(g.gttUsed)} sub="RAM del sistema usada por la GPU" color={C.amber} history={h?.gttUsed} />
      <Stat label="Reloj núcleo" value={g.gfxClock != null ? `${g.gfxClock.toFixed(0)} MHz` : "N/D"} sub={g.gfxClockMax ? `máx. ${g.gfxClockMax.toFixed(0)} MHz` : undefined} color={C.teal} history={h?.gfxClock} />
      <Stat label="Potencia" value={g.power != null ? `${g.power.toFixed(0)} W` : "N/D"} sub={g.powerLimit ? `límite ${g.powerLimit.toFixed(0)} W` : undefined} color={C.orange} history={h?.power} />
      <Stat label="Temperatura" value={g.tempEdge != null ? `${g.tempEdge.toFixed(0)}°C` : "N/D"} sub={g.tempHotspot != null ? `unión ${g.tempHotspot.toFixed(0)}° · mem ${g.tempMem?.toFixed(0) ?? "?"}°` : undefined} color={tempColor(g.tempEdge ?? 0)} history={h?.tempEdge} />
    </div>

    {#if g.throttle.length}
      <div class="throttle" style="grid-column: span 4">
        <span>⚠ Bajando relojes por:</span>
        {#each g.throttle as t}<b>{t}</b>{/each}
      </div>
    {/if}

    <Panel title="Motores" span={2} accent={C.lime}>
      {#snippet extra()}<span>{g.processesSampled ? "desglose por motor (fdinfo)" : "contadores globales"}</span>{/snippet}
      <div class="engines">
        {#each g.engines as e (e.name)}
          <div class="eng">
            <span class="ename">{engName(e.name)}<small>{e.name}</small></span>
            <div class="ebar"><i style:width="{Math.min(100, e.busy)}%" style:background={heatColor(e.busy)}></i></div>
            <b style:color={heatColor(e.busy)}>{e.busy.toFixed(1)}%</b>
          </div>
        {:else}
          <p class="hint">El driver no informa del uso por motor.</p>
        {/each}
      </div>
    </Panel>
    <Panel title="Motores en el tiempo" span={2} accent={C.lime}>
      {#key engineNames.join()}
        <TimeChart
          height={200}
          yMin={0}
          yMax={100}
          yFmt={(v) => `${v.toFixed(0)}%`}
          series={engineNames.map((n, i) => ({ label: engName(n), color: [C.lime, C.cyan, C.magenta, C.amber, C.violet, C.teal][i % 6], fill: i === 0 }))}
          data={() => engineNames.map((n) => metrics.view(h?.engines.get(n) ?? []))}
        />
      {/key}
    </Panel>

    <Panel title="Procesos en la GPU" span={3} accent={C.cyan}>
      {#snippet extra()}
        <span>ordenar:</span>
        <button class="sortbtn" class:on={procSort === "vram"} onclick={() => (procSort = "vram")}>memoria</button>
        <button class="sortbtn" class:on={procSort === "busy"} onclick={() => (procSort = "busy")}>uso</button>
      {/snippet}
      {#if !g.processesSampled}
        <p class="hint">Midiendo…</p>
      {:else if !procList.length}
        <p class="hint">Ningún proceso visible está usando esta GPU.</p>
      {:else}
        <div class="ptable" style:--cols="minmax(140px, 1.5fr) 64px 90px 90px {procEngines.map(() => '90px').join(' ')}">
          <div class="phead">
            <span>Proceso</span><span class="r">PID</span><span class="r">VRAM</span><span class="r">GTT</span>
            {#each procEngines as e}<span class="r">{engName(e)}</span>{/each}
          </div>
          {#each procList.slice(0, 40) as p (p.pid)}
            <div class="prow">
              <span class="pname">{p.name || "?"}</span>
              <span class="r dim">{p.pid}</span>
              <span class="r" style:color={C.magenta}>{fmtBytes(p.vram)}</span>
              <span class="r" style:color={C.amber}>{fmtBytes(p.gtt)}</span>
              {#each procEngines as en}
                {@const v = p.engines.find((x) => x.name === en)?.busy ?? 0}
                <span class="pbar"><i style:width="{Math.min(100, v)}%" style:background={heatColor(v)}></i><em>{v.toFixed(1)}%</em></span>
              {/each}
            </div>
          {/each}
        </div>
        <p class="hint">
          {g.vendor === "nvidia"
            ? "Datos de NVML."
            : "Datos de /proc/*/fdinfo: solo se ven los procesos de tu usuario (los de root, como algunos servidores gráficos, no aparecen)."}
        </p>
      {/if}
    </Panel>
    <Panel title="VRAM por proceso" accent={C.magenta}>
      {#if vramRows.length}
        <EChart height={170} option={vramRose} />
        <ul class="vlegend">
          {#each vramRows as r (r.name + r.pid)}
            <li style:--c={r.color}><i></i><span title={r.name}>{r.name}</span><b>{fmtBytes(r.value)}</b></li>
          {/each}
        </ul>
      {:else}
        <p class="hint">Ningún proceso visible usa VRAM ahora mismo.</p>
      {/if}
    </Panel>

    <Panel title="Tráfico de VRAM" span={3} accent={C.magenta}>
      {#snippet extra()}
        <span>pico teórico {gbs(g.vramPeakBw)}</span>
        <span style:color={C.magenta}>ahora {gbs(g.vramBw, 2)}</span>
      {/snippet}
      <TimeChart
        height={240}
        yMin={0}
        yMax={g.vramPeakBw ?? undefined}
        yFmt={(v) => `${(v / 1e9).toFixed(0)} GB/s`}
        series={[{ label: "tráfico VRAM", color: C.magenta, fill: true, width: 2 }]}
        data={() => [metrics.view(h?.vramBw ?? [])]}
      />
      <p class="hint">
        Estimación: actividad del controlador de memoria × ancho de banda al reloj actual
        ({g.vramType ?? "?"}, {g.busWidth ?? "?"} bits). El eje llega al máximo teórico para ver la proporción.
      </p>
    </Panel>
    <Panel title="Uso del bus de VRAM" accent={C.violet}>
      <EChart
        height={250}
        option={neonGauge({ value: bwPct, color: C.magenta, label: "DEL MÁXIMO", text: fmtPct(bwPct, 1), sub: gbs(g.vramBw, 2) })}
      />
    </Panel>

    {#if g.pcieRx != null}
      <Panel title="Tráfico PCIe" span={3} accent={C.amber}>
        {#snippet extra()}
          <span style:color={C.cyan}>▼ hacia la GPU {fmtRate(g.pcieRx ?? 0)}</span>
          <span style:color={C.amber}>▲ desde la GPU {fmtRate(g.pcieTx ?? 0)}</span>
        {/snippet}
        <TimeChart
          height={200}
          mirrored
          yFmt={(v) => fmtBytes(v, 0) + "/s"}
          series={[
            { label: "recibido", color: C.cyan, fill: true },
            { label: "enviado", color: C.amber, fill: true, negate: true },
          ]}
          data={() => [metrics.view(h?.pcieRx ?? []), metrics.view(h?.pcieTx ?? [])]}
        />
      </Panel>
    {/if}
    <Panel title="Bus PCIe" accent={C.amber}>
      <dl class="info">
        {#if g.integrated}
          <dt>Enlace</dt><dd>integrada (sin PCIe externo)</dd>
        {:else}
          <dt>Enlace</dt><dd>PCIe {pcieGen(g.pcieSpeed)} x{g.pcieWidth ?? "?"} · {g.pcieSpeed ?? "?"} GT/s</dd>
          <dt>Máximo</dt><dd>PCIe {pcieGen(g.pcieMaxSpeed)} x{g.pcieMaxWidth ?? "?"}</dd>
          <dt>Pico</dt><dd>{gbs(g.pciePeakBw)} por sentido</dd>
          <dt>Tráfico</dt>
          <dd class:dim={g.pcieRx == null}>{g.pcieRx != null ? `▼ ${fmtRate(g.pcieRx)} · ▲ ${fmtRate(g.pcieTx ?? 0)}` : "no expuesto por el driver"}</dd>
        {/if}
      </dl>
      {#if g.pcieRx == null && !g.integrated}
        <p class="hint">El tráfico PCIe en vivo solo lo publica NVML (NVIDIA); amdgpu e Intel no lo exponen en estas generaciones.</p>
      {/if}
    </Panel>

    <Panel title="GPU vs memoria" span={g.pcieRx != null ? 4 : 3} accent={C.cyan}>
      {#snippet extra()}<span style:color={C.cyan}>motor gráfico</span><span style:color={C.violet}>controlador VRAM</span>{/snippet}
      <TimeChart
        height={200}
        yMin={0}
        yMax={100}
        yFmt={(v) => `${v.toFixed(0)}%`}
        series={[
          { label: "GPU", color: C.cyan, fill: true },
          { label: "VRAM", color: C.violet },
        ]}
        data={() => [metrics.view(h?.busy ?? []), metrics.view(h?.memBusy ?? [])]}
      />
    </Panel>
    <Panel title="Anchos de banda del sistema" span={2} accent={C.amber}>
      {#snippet extra()}<span>escala logarítmica</span>{/snippet}
      <EChart height={200} option={compare} />
      {#if !bench.last}<p class="hint">Ejecuta el test de RAM (pestaña Memoria) para compararla aquí.</p>{/if}
    </Panel>

    <Panel title="Memoria de vídeo" accent={C.magenta} half>
      {#snippet extra()}<span style:color={C.amber}>anillo exterior: GTT</span>{/snippet}
      <EChart height={220} option={vramDonut} />
    </Panel>
    <Panel title="Potencia" accent={C.orange} half>
      <EChart height={220} option={powerGauge} />
    </Panel>
    <Panel title="Relojes" span={2} accent={C.teal}>
      {#snippet extra()}<span style:color={C.teal}>núcleo MHz</span><span style:color={C.amber}>memoria MHz</span>{/snippet}
      <TimeChart
        height={180}
        yMin={0}
        yFmt={(v) => `${v.toFixed(0)}`}
        series={[
          { label: "núcleo MHz", color: C.teal, fill: true },
          { label: "memoria MHz", color: C.amber },
        ]}
        data={() => [metrics.view(h?.gfxClock ?? []), metrics.view(h?.memClock ?? [])]}
      />
    </Panel>

    <Panel title="Tarjeta" span={2} accent={C.blue}>
      <dl class="info wide">
        <dt>Modelo</dt><dd>{g.name}</dd>
        <dt>Driver</dt><dd>{g.driver}{g.driverVersion ? ` ${g.driverVersion}` : ""}</dd>
        <dt>Memoria</dt><dd>{g.vramTotal ? fmtBytes(g.vramTotal, 0) : "compartida"} {g.vramType ?? ""}{g.busWidth ? ` · bus de ${g.busWidth} bits` : ""}{g.vramPeakBw ? ` · ${gbs(g.vramPeakBw)}` : ""}</dd>
        <dt>Ranura</dt><dd>{g.pciSlot}</dd>
        <dt>Ventilador</dt><dd>{g.fanRpm != null ? `${Math.round(g.fanRpm)} RPM` : ""}{g.fanPct != null ? ` (${g.fanPct.toFixed(0)}%)` : ""}{g.fanRpm == null && g.fanPct == null ? "—" : ""}</dd>
        <dt>Temperaturas</dt><dd>{[g.tempEdge != null ? `borde ${g.tempEdge.toFixed(0)}°` : "", g.tempHotspot != null ? `unión ${g.tempHotspot.toFixed(0)}°` : "", g.tempMem != null ? `memoria ${g.tempMem.toFixed(0)}°` : ""].filter(Boolean).join(" · ") || "—"}</dd>
      </dl>
    </Panel>
  </div>
{/if}

<style>
  .vlegend {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
    display: grid;
    gap: 3px;
    font-size: 10.5px;
  }
  .vlegend li {
    display: grid;
    grid-template-columns: 8px minmax(0, 1fr) auto;
    gap: 7px;
    align-items: center;
    font-variant-numeric: tabular-nums;
  }
  .vlegend i {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--c);
  }
  .vlegend span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: 10px;
  }
  .picker {
    display: flex;
    gap: 8px;
  }
  .picker button {
    background: none;
    border: 1px solid color-mix(in srgb, var(--cyan) 25%, transparent);
    color: var(--text);
    font: inherit;
    padding: 6px 12px;
    cursor: pointer;
  }
  .picker button {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .vendor {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.1em;
    color: #fff;
    padding: 1px 5px;
  }
  .throttle {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    font-size: 11px;
    color: var(--amber);
    border: 1px solid color-mix(in srgb, var(--amber) 40%, transparent);
    padding: 6px 10px;
  }
  .throttle b {
    background: color-mix(in srgb, var(--amber) 15%, transparent);
    padding: 1px 6px;
    font-weight: 500;
  }
  .engines {
    display: grid;
    gap: 9px;
    padding-top: 4px;
  }
  .eng {
    display: grid;
    grid-template-columns: 150px 1fr 60px;
    gap: 10px;
    align-items: center;
    font-size: 11px;
  }
  .ename {
    display: flex;
    flex-direction: column;
  }
  .ename small {
    color: var(--muted);
    font-size: 9px;
  }
  .ebar {
    height: 10px;
    background: color-mix(in srgb, var(--cyan) 6%, transparent);
  }
  .ebar i {
    display: block;
    height: 100%;
  }
  .eng b {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .sortbtn {
    background: none;
    border: 1px solid color-mix(in srgb, var(--cyan) 25%, transparent);
    color: var(--muted);
    font: inherit;
    font-size: 10px;
    padding: 1px 7px;
    cursor: pointer;
  }
  .sortbtn.on {
    color: var(--bg);
    background: var(--cyan);
  }
  .ptable {
    font-size: 11px;
    max-height: 340px;
    overflow: auto;
  }
  .phead,
  .prow {
    display: grid;
    grid-template-columns: var(--cols);
    gap: 10px;
    align-items: center;
    padding: 3px 4px;
  }
  .phead {
    font-size: 9.5px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    border-bottom: 1px solid color-mix(in srgb, var(--cyan) 25%, transparent);
    position: sticky;
    top: 0;
    background: var(--bg);
  }
  .prow:hover {
    background: color-mix(in srgb, var(--cyan) 6%, transparent);
  }
  .pname {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .r {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .pbar {
    position: relative;
    height: 14px;
    background: color-mix(in srgb, var(--cyan) 5%, transparent);
  }
  .pbar i {
    position: absolute;
    inset: 0 auto 0 0;
    opacity: 0.5;
  }
  .pbar em {
    position: relative;
    display: block;
    text-align: right;
    padding-right: 4px;
    font-style: normal;
    line-height: 14px;
    font-variant-numeric: tabular-nums;
  }
  .picker button.on {
    border-color: var(--cyan);
    background: color-mix(in srgb, var(--cyan) 12%, transparent);
  }
  .picker small {
    color: var(--muted);
  }
  .hint {
    font-size: 10px;
    color: var(--muted);
    margin: 6px 0 0;
  }
  .info {
    display: grid;
    grid-template-columns: 70px 1fr;
    gap: 6px 10px;
    margin: 4px 0 0;
    font-size: 12px;
  }
  .info.wide {
    grid-template-columns: 90px 1fr 90px 1fr;
  }
  @media (max-width: 700px) {
    .info.wide {
      grid-template-columns: 90px 1fr;
    }
    .eng {
      grid-template-columns: 90px 1fr 50px;
    }
  }
  .info dt {
    color: var(--muted);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    align-self: center;
  }
  .info dd {
    margin: 0;
  }
  .dim {
    color: var(--muted);
  }
  .empty {
    height: 50vh;
    display: grid;
    place-content: center;
    text-align: center;
  }
</style>
