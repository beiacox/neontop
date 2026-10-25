<script lang="ts">
  import EChart from "$lib/charts/EChart.svelte";
  import XYChart from "$lib/charts/XYChart.svelte";
  import { tooltipBase } from "$lib/charts/echarts";
  import Panel from "$lib/ui/Panel.svelte";
  import Stat from "$lib/ui/Stat.svelte";
  import { games, stabilityLabel, benchCsv, type Benchmark, type SessionDetail } from "$lib/games.svelte";
  import { histOption, pctOption } from "$lib/gameCharts";
  import { saveTextFile } from "$lib/export";
  import { platform } from "$lib/platform.svelte";
  import { C, MONO, rgba } from "$lib/theme";
  import { fmtDuration } from "$lib/format";

  let { id }: { id: string } = $props();

  let b = $derived(games.benchmarks.find((x) => x.id === id) ?? null);
  let detail = $state<SessionDetail | null>(null);
  let loading = $state(true);
  $effect(() => {
    const want = id;
    loading = true;
    detail = null;
    games.detail(want).then((d) => {
      if (want === id) {
        detail = d;
        loading = false;
      }
    });
  });

  let label = $derived(b ? stabilityLabel(b.stability, b.frames) : stabilityLabel(0, 0));
  const f1 = (v: number) => (v > 0 && Number.isFinite(v) ? v.toFixed(1) : "—");

  /** Otras partidas/benchmarks del mismo juego, de la más antigua a la más reciente. */
  let siblings = $derived(b ? games.benchmarks.filter((x) => x.exe === b!.exe && x.kind === b!.kind).toReversed().slice(-20) : []);
  let compare = $derived({
    tooltip: { ...tooltipBase, trigger: "axis", axisPointer: { type: "shadow" }, valueFormatter: (v: number) => `${v.toFixed(1)} FPS` },
    legend: { top: 0, right: 0, textStyle: { color: C.muted, fontFamily: MONO, fontSize: 10 }, itemWidth: 10, itemHeight: 8 },
    grid: { left: 40, right: 8, top: 24, bottom: 24 },
    xAxis: {
      type: "category",
      data: siblings.map((x) => new Date(x.date).toLocaleDateString("es", { day: "2-digit", month: "2-digit" })),
      axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9 },
      axisLine: { lineStyle: { color: C.grid } },
      axisTick: { show: false },
    },
    yAxis: { type: "value", axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9 }, splitLine: { lineStyle: { color: C.grid } } },
    series: [
      {
        name: "Media",
        type: "bar",
        barGap: "10%",
        data: siblings.map((x) => ({ value: x.avg, itemStyle: { color: x.id === id ? C.cyan : rgba(C.cyan, 0.35) } })),
      },
      {
        name: "1 % bajo",
        type: "bar",
        data: siblings.map((x) => ({ value: x.low1, itemStyle: { color: x.id === id ? C.amber : rgba(C.amber, 0.35) } })),
      },
    ],
  });

  let hist = $derived(detail ? histOption(detail.hist.counts, detail.hist.step, detail.pct.p999, detail.pct.median) : null);
  let pct = $derived(detail && b ? pctOption({ avg: b.avg, ...detail.pct }) : null);

  let msg = $state<string | null>(null);
  async function exportCsv(x: Benchmark) {
    msg = null;
    try {
      const name = `neontop-${x.title.replace(/[^\w.-]+/g, "_")}-${new Date(x.date).toISOString().slice(0, 16).replace(/[-:T]/g, "")}.csv`;
      const p = await saveTextFile(name, await benchCsv(x));
      if (p) msg = `Guardado en ${p}`;
    } catch (e) {
      msg = String(e);
    }
  }
  function remove() {
    if (b && confirm(`¿Borrar ${b.kind === "bench" ? "este benchmark" : "esta partida"} de ${b.title}?`)) games.remove(b.id);
  }
</script>

{#if !b}
  <div class="grid">
    <Panel title="Registro no encontrado" span={4} accent={C.muted}>
      <p class="hint">Este registro ya no está en el historial.</p>
      <button class="back" onclick={() => (games.viewing = null)}>← Volver</button>
    </Panel>
  </div>
{:else}
  <div class="grid">
    <div class="head" style="grid-column: span 4">
      <button class="back" onclick={() => (games.viewing = null)}>← Volver</button>
      <div class="title">
        <h2>{b.title}</h2>
        <span>
          <b class="kind">{b.kind === "bench" ? "BENCHMARK" : "PARTIDA"}</b>
          {new Date(b.date).toLocaleString("es", { dateStyle: "full", timeStyle: "short" })} · {fmtDuration(b.seconds)} · {b.exe} · {b.api}
        </span>
      </div>
      <div class="actions">
        {#if b.kind === "bench" && (b.frametimes || detail?.frametimes)}<button onclick={() => exportCsv(b!)}>CSV</button>{/if}
        {#if platform.desktop || platform.webActions}<button class="del" onclick={remove}>Borrar</button>{/if}
      </div>
    </div>
    {#if msg}<p class="hint" style="grid-column: span 4">{msg}</p>{/if}

    <div class="stats" style="grid-column: span 4">
      <Stat label="Media" value={f1(b.avg)} sub={`${b.frames.toLocaleString("es")} fotogramas`} color={C.cyan} />
      <Stat label="1 % bajo" value={f1(b.low1)} sub={detail ? `${f1(detail.pct.p99)} ms` : undefined} color={C.amber} />
      <Stat label="0,1 % bajo" value={f1(b.low01)} sub={detail ? `${f1(detail.pct.p999)} ms` : undefined} color={C.orange} />
      <Stat label="Mín / máx" value={`${b.minFps ? b.minFps.toFixed(0) : "—"} / ${b.maxFps ? b.maxFps.toFixed(0) : "—"}`} sub="FPS por segundo" color={C.violet} />
      <Stat label="Tirones" value={String(b.stutters)} sub={`${b.seconds > 0 ? ((b.stutters / b.seconds) * 60).toFixed(1) : 0}/min · ${b.hitches} parones`} color={b.hitches ? C.red : C.magenta} />
      <Stat label="Ritmo" value={`±${b.jitterMs.toFixed(2)} ms`} sub="variación entre fotogramas" color={C.teal} />
      <Stat label="Estabilidad" value={b.stability.toFixed(0)} sub={label.text} color={C[label.color]} />
      <Stat label="CPU / GPU" value={`${b.cpu.toFixed(0)} / ${b.gpu.toFixed(0)} %`} sub={`${b.gpuPower.toFixed(0)} W · máx ${b.cpuTempMax.toFixed(0)}/${b.gpuTempMax.toFixed(0)} °C`} color={C.lime} />
    </div>

    {#if loading}
      <Panel title="Cargando…" span={4} accent={C.muted}><p class="hint">Leyendo los datos de la partida…</p></Panel>
    {:else if !detail}
      <Panel title="Sin datos detallados" span={4} accent={C.muted}>
        <p class="hint">Este registro se guardó con una versión anterior de NeonTop, que solo guardaba el resumen. Las partidas nuevas se guardan completas.</p>
      </Panel>
    {:else}
      <Panel title="FPS durante la partida" span={4} accent={C.lime}>
        {#snippet extra()}<span style:color={C.lime}>FPS</span><span style:color={C.amber}>1 % bajo (5 s)</span>{/snippet}
        <XYChart
          height={240}
          yFloor={60}
          x={detail.t}
          ys={[detail.fps, detail.low1]}
          series={[
            { label: "FPS", color: C.lime, fill: true, width: 1.6 },
            { label: "1 % bajo", color: C.amber, width: 1.3 },
          ]}
        />
      </Panel>

      <Panel title="Peor fotograma" span={2} accent={C.red}>
        {#snippet extra()}<span>ms · los picos son tirones</span>{/snippet}
        <XYChart height={200} yFloor={20} yFmt={(v) => `${v.toFixed(0)} ms`} x={detail.t} ys={[detail.ftMax]} series={[{ label: "peor fotograma", color: C.red, fill: true, width: 1.2 }]} />
      </Panel>
      <Panel title="CPU y GPU" span={2} accent={C.magenta}>
        {#snippet extra()}<span style:color={C.magenta}>GPU</span><span style:color={C.cyan}>CPU</span>{/snippet}
        <XYChart
          height={200}
          yMin={0}
          yMax={100}
          yFmt={(v) => `${v.toFixed(0)}%`}
          x={detail.t}
          ys={[detail.gpu, detail.cpu]}
          series={[
            { label: "GPU", color: C.magenta, width: 1.5 },
            { label: "CPU", color: C.cyan, width: 1.3 },
          ]}
        />
      </Panel>

      <Panel title="Temperaturas" span={2} accent={C.amber}>
        {#snippet extra()}<span style:color={C.amber}>GPU</span><span style:color={C.teal}>CPU</span>{/snippet}
        <XYChart
          height={180}
          yFloor={60}
          yFmt={(v) => `${v.toFixed(0)}°`}
          x={detail.t}
          ys={[detail.gpuTemp, detail.cpuTemp]}
          series={[
            { label: "GPU °C", color: C.amber, width: 1.5 },
            { label: "CPU °C", color: C.teal, width: 1.3 },
          ]}
        />
      </Panel>
      <Panel title="Potencia de la GPU" span={2} accent={C.orange}>
        <XYChart height={180} yFmt={(v) => `${v.toFixed(0)} W`} x={detail.t} ys={[detail.gpuPower]} series={[{ label: "W", color: C.orange, fill: true, width: 1.4 }]} />
      </Panel>

      <Panel title="Distribución de tiempos" span={2} accent={C.lime}>
        {#snippet extra()}<span>escala logarítmica · rojo = tirón</span>{/snippet}
        {#if hist}<EChart height={220} option={hist} />{/if}
      </Panel>
      <Panel title="FPS por percentil" span={2} accent={C.amber}>
        {#if pct}<EChart height={220} option={pct} />{/if}
      </Panel>
    {/if}

    {#if siblings.length > 1}
      <Panel title={b.kind === "bench" ? "Benchmarks de este juego" : "Partidas de este juego"} span={4} accent={C.cyan}>
        {#snippet extra()}<span>la resaltada es esta</span>{/snippet}
        <EChart height={200} option={compare} onclick={(p: any) => (games.viewing = siblings[p.dataIndex]?.id ?? games.viewing)} />
      </Panel>
    {/if}
  </div>
{/if}

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: 10px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-wrap: wrap;
  }
  .title {
    flex: 1;
    min-width: 0;
  }
  .title h2 {
    margin: 0;
    font-family: var(--display);
    font-size: 22px;
    color: var(--lime);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title span {
    font-size: 11px;
    color: var(--muted);
  }
  .kind {
    color: var(--cyan);
    letter-spacing: 0.1em;
    margin-right: 6px;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  button {
    font: inherit;
    font-size: 12px;
    padding: 6px 12px;
    background: transparent;
    color: var(--cyan);
    border: 1px solid color-mix(in srgb, var(--cyan) 40%, transparent);
    cursor: pointer;
  }
  button.del {
    color: var(--red);
    border-color: color-mix(in srgb, var(--red) 50%, transparent);
  }
  .hint {
    font-size: 11px;
    color: var(--muted);
    margin: 0;
    line-height: 1.6;
  }
</style>
