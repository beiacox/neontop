<script lang="ts">
  import EChart from "$lib/charts/EChart.svelte";
  import { tooltipBase } from "$lib/charts/echarts";
  import { bench } from "$lib/bench.svelte";
  import { C, MONO, rgba } from "$lib/theme";
  import { fmtBytes } from "$lib/format";

  let r = $derived(bench.last);
  const gbs = (v: number) => `${(v / 1e9).toFixed(1)} GB/s`;

  let bars = $derived(
    r
      ? [
          { label: "Lectura", v: r.read, c: C.cyan, hint: `${r.threads} hilos` },
          { label: "Escritura", v: r.write, c: C.magenta, hint: `${r.threads} hilos` },
          { label: "Copia", v: r.copy, c: C.violet, hint: "lectura + escritura" },
          { label: "Lectura 1 hilo", v: r.readSingle, c: C.teal, hint: "un solo núcleo" },
        ]
      : [],
  );
  let top = $derived(Math.max(...bars.map((b) => b.v), 1));

  // evolución entre ejecuciones
  let history = $derived({
    tooltip: { ...tooltipBase, trigger: "axis", valueFormatter: (v: number) => `${v.toFixed(1)} GB/s` },
    grid: { left: 40, right: 10, top: 10, bottom: 22 },
    xAxis: {
      type: "category",
      data: bench.runs.map((x) => new Date(x.ts).toLocaleTimeString("es", { hour: "2-digit", minute: "2-digit", hour12: false })),
      axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9 },
      axisLine: { lineStyle: { color: rgba(C.cyan, 0.2) } },
    },
    yAxis: { type: "value", axisLabel: { color: C.muted, fontFamily: MONO, fontSize: 9 }, splitLine: { lineStyle: { color: C.grid } } },
    series: [
      { name: "Lectura", type: "line", data: bench.runs.map((x) => x.read / 1e9), itemStyle: { color: C.cyan }, symbolSize: 5 },
      { name: "Escritura", type: "line", data: bench.runs.map((x) => x.write / 1e9), itemStyle: { color: C.magenta }, symbolSize: 5 },
      { name: "Copia", type: "line", data: bench.runs.map((x) => x.copy / 1e9), itemStyle: { color: C.violet }, symbolSize: 5 },
    ],
  });
</script>

<div class="bench">
  <div class="top">
    <button class="run" disabled={bench.running} onclick={() => bench.run()}>
      {bench.running ? "Midiendo…" : r ? "↻ Repetir test" : "▶ Ejecutar test"}
    </button>
    <p class="hint">
      Mide el máximo que da tu RAM leyendo, escribiendo y copiando {r ? fmtBytes(r.bufferBytes, 0) : "512 MiB"} con todos los hilos (~1–2 s de carga).
      Es el <em>máximo</em> alcanzable; el uso real en cada momento está en «Tráfico de RAM en vivo».
    </p>
  </div>

  {#if bench.error}<p class="err">{bench.error}</p>{/if}

  {#if r}
    <div class="results">
      <div class="bars">
        {#each bars as b}
          <div class="row" style:--c={b.c}>
            <span class="label">{b.label}<small>{b.hint}</small></span>
            <div class="track"><i style:width="{(b.v / top) * 100}%"></i></div>
            <b>{gbs(b.v)}</b>
          </div>
        {/each}
      </div>
      <div class="lat">
        <span>Latencia</span>
        <b>{r.latencyNs.toFixed(1)}<small> ns</small></b>
        <span class="sub">acceso aleatorio a memoria</span>
        <span class="sub">{new Date(r.ts).toLocaleString("es", { dateStyle: "short", timeStyle: "short" })} · {(r.durationMs / 1000).toFixed(1)} s</span>
      </div>
    </div>
    {#if bench.runs.length > 1}
      <EChart height={130} option={history} />
    {/if}
  {/if}
</div>

<style>
  .top {
    display: flex;
    gap: 14px;
    align-items: center;
  }
  .run {
    background: var(--cyan);
    color: var(--bg);
    border: none;
    font: inherit;
    font-weight: 700;
    font-size: 12px;
    padding: 8px 16px;
    cursor: pointer;
    white-space: nowrap;
    box-shadow: 0 0 12px color-mix(in srgb, var(--cyan) 50%, transparent);
  }
  .run:disabled {
    opacity: 0.6;
    cursor: progress;
  }
  .hint {
    font-size: 10.5px;
    color: var(--muted);
    margin: 0;
  }
  .err {
    color: var(--red);
    font-size: 11px;
  }
  .results {
    display: grid;
    grid-template-columns: 1fr 220px;
    gap: 20px;
    align-items: center;
    margin: 14px 0 6px;
  }
  .bars {
    display: grid;
    gap: 8px;
  }
  .row {
    display: grid;
    grid-template-columns: 120px 1fr 90px;
    gap: 10px;
    align-items: center;
    font-size: 11px;
  }
  .label {
    display: flex;
    flex-direction: column;
  }
  .label small {
    color: var(--muted);
    font-size: 9px;
  }
  .track {
    height: 12px;
    background: color-mix(in srgb, var(--c) 8%, transparent);
  }
  .track i {
    display: block;
    height: 100%;
    background: linear-gradient(90deg, color-mix(in srgb, var(--c) 35%, transparent), var(--c));
  }
  .row b {
    text-align: right;
    color: var(--c);
    font-variant-numeric: tabular-nums;
  }
  .lat {
    display: flex;
    flex-direction: column;
    border-left: 2px solid var(--amber);
    padding-left: 12px;
  }
  .lat span {
    font-size: 10px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }
  .lat b {
    font-family: var(--display);
    font-size: 28px;
    color: var(--amber);
    text-shadow: 0 0 10px color-mix(in srgb, var(--amber) 60%, transparent);
  }
  .lat small {
    font-size: 12px;
  }
  .lat .sub {
    text-transform: none;
    letter-spacing: 0;
  }
  /* teléfono: botón arriba a lo ancho, explicación debajo y resultados apilados */
  @media (max-width: 700px) {
    .top {
      flex-direction: column;
      align-items: stretch;
      gap: 10px;
    }
    .results {
      grid-template-columns: minmax(0, 1fr);
      gap: 14px;
    }
    .row {
      grid-template-columns: 96px minmax(0, 1fr) 76px;
      gap: 8px;
    }
  }
</style>
