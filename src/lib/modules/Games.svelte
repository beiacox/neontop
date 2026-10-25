<script lang="ts">
  import EChart from "$lib/charts/EChart.svelte";
  import TimeChart from "$lib/charts/TimeChart.svelte";
  import FrameChart from "$lib/charts/FrameChart.svelte";
  import { neonGauge } from "$lib/charts/echarts";
  import Panel from "$lib/ui/Panel.svelte";
  import Stat from "$lib/ui/Stat.svelte";
  import { metrics } from "$lib/store.svelte";
  import { procs } from "$lib/procs.svelte";
  import { want } from "$lib/collect";
  import { games, stabilityLabel, benchCsv, type Benchmark, type GameTrack } from "$lib/games.svelte";
  import { saveTextFile } from "$lib/export";
  import { histOption, pctOption } from "$lib/gameCharts";
  import SessionView from "$lib/ui/SessionView.svelte";
  import FpsAuto from "$lib/ui/FpsAuto.svelte";
  import { platform } from "$lib/platform.svelte";
  import { C } from "$lib/theme";
  import { fmtBytes, fmtDuration } from "$lib/format";

  // datos del proceso del juego (CPU, RAM, VRAM)
  $effect(() => {
    procs.want("games-tab", true);
    want("gpu_procs", "games-tab", true);
    return () => {
      procs.want("games-tab", false);
      want("gpu_procs", "games-tab", false);
    };
  });

  let s = $derived(metrics.snap!);
  let layerOk = $derived(s.games.layerInstalled);
  let list = $derived((games.version, games.list));
  let tr = $derived((games.version, games.current));
  let st = $derived((games.version, tr?.session));
  /**
   * Instantánea de las estadísticas: los objetos FrameStats/GameTrack se mutan en sitio, así
   * que la vista lee copias nuevas en cada muestra (si no, Svelte no vería los cambios).
   */
  let v = $derived.by(() => {
    games.version;
    if (!tr) return null;
    const S = tr.session;
    return {
      count: S.count, avg: S.avgFps, low1: S.low1, low01: S.low01, p99: S.percentile(99), p999: S.percentile(99.9),
      stutters: S.stutters, hitches: S.hitches, jitter: S.jitterMs, stability: S.stability, seconds: S.seconds,
      minFps: tr.minFps, maxFps: tr.maxFps, ended: tr.ended,
    };
  });

  const API: Record<string, string> = { vulkan: "Vulkan", opengl: "OpenGL", egl: "OpenGL ES / EGL", "?": "?" };
  const f0 = (v: number) => (v > 0 && Number.isFinite(v) ? v.toFixed(0) : "—");
  const f1 = (v: number) => (v > 0 && Number.isFinite(v) ? v.toFixed(1) : "—");

  let median = $derived((games.version, tr ? tr.median() : 0));
  let lastSecFt = $derived(tr && tr.info.fps > 0 ? 1000 / tr.info.fps : 0);
  let score = $derived(v?.stability ?? 0);
  let label = $derived(stabilityLabel(score, v?.count ?? 0));
  let perMin = (n: number) => (v && v.seconds > 0 ? (n / v.seconds) * 60 : 0);

  // ── Diagnóstico: qué limita los FPS ahora ──
  let verdict = $derived.by(() => {
    if (!tr || !tr.info.active || !st || st.count < 60) return null;
    const g = s.gpus.find((x) => !x.integrated) ?? s.gpus[0];
    const gpu = g?.busy ?? 0;
    const maxCore = Math.max(0, ...s.cpu.cores.map((c) => c.usage));
    const { y } = tr.lastFrames(3);
    const mean = y.reduce((a, b) => a + b, 0) / Math.max(1, y.length);
    const cv = Math.sqrt(y.reduce((a, b) => a + (b - mean) ** 2, 0) / Math.max(1, y.length)) / Math.max(0.01, mean);
    if (gpu >= 92) return { text: "Limitado por la GPU", detail: `GPU al ${gpu.toFixed(0)} %: la tarjeta gráfica es el techo. Bajar resolución o calidad gráfica sube los FPS.`, color: C.magenta };
    if (cv < 0.06 && gpu < 85) return { text: `Tope de FPS / sincronía vertical (~${(1000 / mean).toFixed(0)} FPS)`, detail: `Ritmo muy constante con la GPU al ${gpu.toFixed(0)} %: hay un límite de FPS o VSync activo.`, color: C.lime };
    if (maxCore >= 88 && gpu < 85) return { text: "Limitado por la CPU", detail: `Un núcleo al ${maxCore.toFixed(0)} % con la GPU al ${gpu.toFixed(0)} %: el hilo principal del juego no da más.`, color: C.cyan };
    return { text: "Sin un límite claro", detail: `GPU ${gpu.toFixed(0)} % · núcleo más cargado ${maxCore.toFixed(0)} %. Puede influir el motor, la red o el disco.`, color: C.amber };
  });

  // ── Proceso del juego ──
  let proc = $derived((procs.tick, tr ? procs.list.find((p) => p.pid === tr.pid) : undefined));
  let vram = $derived(tr ? s.gpus.flatMap((g) => g.processes).filter((p) => p.pid === tr.pid).reduce((a, p) => a + p.vram, 0) : 0);
  let gpuUse = $derived(
    tr ? s.gpus.flatMap((g) => g.processes).filter((p) => p.pid === tr.pid).reduce((a, p) => a + p.engines.reduce((b, e) => Math.max(b, e.busy), 0), 0) : 0,
  );

  // ── Distribución y percentiles (mismas gráficas que las partidas guardadas) ──
  let hist = $derived.by(() => {
    games.version;
    return st && st.count ? histOption(st.grouped(0.25, 100), 0.25, st.percentile(99.9), median) : null;
  });
  let pct = $derived.by(() => {
    games.version;
    return st && st.count
      ? pctOption({ avg: st.avgFps, p50: st.percentile(50), p90: st.percentile(90), p95: st.percentile(95), p99: st.percentile(99), p999: st.percentile(99.9), max: st.max })
      : null;
  });

  // ── Grabación ──
  let recSecs = $state<number | null>(60);
  let rec = $derived((games.version, tr?.rec));
  let recLeft = $derived(rec?.until ? Math.max(0, rec.until - s.ts / 1000) : null);
  let recElapsed = $derived(rec ? s.ts / 1000 - rec.start : 0);
  let exportMsg = $state<string | null>(null);

  async function exportBench(b: Benchmark) {
    exportMsg = null;
    try {
      const name = `neontop-${b.title.replace(/[^\w.-]+/g, "_")}-${new Date(b.date).toISOString().slice(0, 16).replace(/[-:T]/g, "")}.csv`;
      const p = await saveTextFile(name, await benchCsv(b));
      if (p) exportMsg = `Guardado en ${p}`;
    } catch (e) {
      exportMsg = String(e);
    }
  }

  /** Grabación anterior del mismo juego, para comparar. */
  // ── Historial agrupado por juego ──
  let filterGame = $state<string | null>(null);
  let gameGroups = $derived.by(() => {
    const m = new Map<string, { exe: string; title: string; sessions: number; benches: number; seconds: number; best: number; last: number }>();
    for (const b of games.benchmarks) {
      const g = m.get(b.exe) ?? { exe: b.exe, title: b.title, sessions: 0, benches: 0, seconds: 0, best: 0, last: 0 };
      if (b.kind === "bench") g.benches++;
      else g.sessions++;
      g.seconds += b.seconds;
      g.best = Math.max(g.best, b.avg);
      if (b.date > g.last) {
        g.last = b.date;
        g.title = b.title;
      }
      m.set(b.exe, g);
    }
    return [...m.values()].sort((a, b) => b.last - a.last);
  });
  let shownHistory = $derived(filterGame ? games.benchmarks.filter((b) => b.exe === filterGame) : games.benchmarks);

  const previous = (b: Benchmark) => games.benchmarks.find((x) => x.exe === b.exe && x.date < b.date);
  const delta = (a: number, b: number | undefined) => (b ? ((a - b) / b) * 100 : null);

  let fpsSeries = $derived(tr ? [{ label: "FPS", color: C.lime, width: 1.8, fill: true }, { label: "1 % bajo (5 s)", color: C.amber, width: 1.4 }] : []);
  const pickGame = (t: GameTrack) => (games.selected = t.pid);
</script>

{#if games.viewing}
  <SessionView id={games.viewing} />
{:else if !tr}
  <div class="grid">
    <Panel title="Cómo medir un juego" span={4} accent={C.lime}>
      <div class="setup">
        <p>
          NeonTop mide cada fotograma con una capa propia (como MangoHud): funciona con juegos <b>Vulkan</b>, <b>OpenGL</b> y todos los de
          <b>Proton/Wine</b> (DirectX a través de DXVK y VKD3D). Se activa solo en los juegos que lances con ella: NeonTop puede
          ponerla por ti en Steam, Heroic, Lutris y Prism Launcher.
        </p>
        {#if platform.mode !== "web"}
          <div class="autobox"><FpsAuto /></div>
        {/if}
        <h4 class="manual">A mano (otros lanzadores o terminal)</h4>
        <div class="how">
          <div>
            <h4>Steam</h4>
            <p>Propiedades del juego → General → Opciones de lanzamiento:</p>
            <code class="cmd">neontop-fps %command%</code>
            <p class="hint">Combinable: <code>neontop-fps gamemoderun mangohud %command%</code></p>
          </div>
          <div>
            <h4>Lutris, Heroic, Prism</h4>
            <p>Comando envoltorio / prefijo de comando:</p>
            <code class="cmd">neontop-fps</code>
            <p class="hint">Lutris: Opciones del sistema · Heroic: Avanzado · Prism Launcher: Comandos personalizados</p>
          </div>
          <div>
            <h4>Terminal</h4>
            <code class="cmd">neontop-fps ./juego</code>
          </div>
        </div>
        <p class="status">
          {#if layerOk}
            <span class="ok">● Capa instalada.</span> Abre un juego con la opción de arriba y aparecerá aquí automáticamente.
          {:else}
            <span class="warn">● La capa no está instalada</span> (se instala con el paquete de NeonTop 0.9 o posterior).
          {/if}
        </p>
      </div>
    </Panel>
    {#if games.benchmarks.length}
      {@render benchTable()}
    {/if}
  </div>
{:else}
  <div class="grid">
    {#if list.length > 1}
      <div class="picker" style="grid-column: span 4">
        {#each list as g (g.pid)}
          <button class:on={g.pid === tr.pid} onclick={() => pickGame(g)}>
            <b>{g.info.title}</b><span>{g.ended ? "terminado" : g.info.active ? `${g.info.fps} FPS` : "en pausa"} · {API[g.info.api]}</span>
          </button>
        {/each}
      </div>
    {/if}

    <div class="gamehead" style="grid-column: span 4">
      <div class="title">
        <h2>{tr.info.title}</h2>
        <span>
          {tr.info.exe} · PID {tr.pid} · {API[tr.info.api]} · {tr.info.bits} bits{tr.info.steamAppId ? ` · Steam ${tr.info.steamAppId}` : ""} · {fmtDuration(tr.info.duration)}
          {#if v?.ended}<b class="ended">SESIÓN TERMINADA · {new Date(v.ended).toLocaleTimeString("es", { hour: "2-digit", minute: "2-digit" })}</b>
          {:else if !tr.info.active}<b class="paused">EN PAUSA / MINIMIZADO</b>{/if}
        </span>
      </div>
      <div class="actions">
        {#if v?.ended}
          <span class="endnote">Resumen final de la partida · guardado en el historial</span>
          <button class="close" onclick={() => games.dismiss(tr!.pid)}>Cerrar</button>
        {:else if rec}
          <span class="recording">● GRABANDO {recLeft != null ? `· quedan ${recLeft.toFixed(0)} s` : `· ${recElapsed.toFixed(0)} s`}</span>
          <button class="stop" onclick={() => games.stop(tr)}>Detener</button>
        {:else}
          <select bind:value={recSecs} title="Duración de la grabación">
            <option value={30}>30 s</option>
            <option value={60}>60 s</option>
            <option value={120}>2 min</option>
            <option value={300}>5 min</option>
            <option value={null}>manual</option>
          </select>
          <button class="rec" onclick={() => games.start(tr, recSecs)}>● Grabar benchmark</button>
        {/if}
        {#if !v?.ended}
          <button onclick={() => (tr!.resetSession(), games.version++)} title="Empieza de cero las estadísticas de la sesión">Reiniciar</button>
        {/if}
      </div>
    </div>

    <div class="stats" style="grid-column: span 4">
      <Stat label="FPS" value={tr.info.active ? String(tr.info.fps) : "—"} sub={`${f1(lastSecFt)} ms por fotograma`} color={C.lime} history={metrics.h.gameFps.get(tr.pid)} />
      <Stat label="Media" value={f1(v!.avg)} sub={`${v!.count.toLocaleString("es")} fotogramas`} color={C.cyan} />
      <Stat label="1 % bajo" value={f1(v!.low1)} sub={`${f1(v!.p99)} ms`} color={C.amber} history={metrics.h.gameLow.get(tr.pid)} />
      <Stat label="0,1 % bajo" value={f1(v!.low01)} sub={`${f1(v!.p999)} ms`} color={C.orange} />
      <Stat label="Mín / máx" value={`${f0(v!.minFps)} / ${f0(v!.maxFps)}`} sub="FPS por segundo" color={C.violet} />
      <Stat label="Tirones" value={String(v!.stutters)} sub={`${perMin(v!.stutters).toFixed(1)}/min · ${v!.hitches} parones`} color={v!.hitches ? C.red : C.magenta} />
      <Stat label="Ritmo" value={`±${v!.jitter.toFixed(2)} ms`} sub="variación entre fotogramas" color={C.teal} />
      <Stat label="Estabilidad" value={v!.count >= 30 ? `${score.toFixed(0)}` : "—"} sub={v!.count >= 30 ? label.text : "midiendo…"} color={C[label.color]} />
    </div>

    <Panel title="Tiempos de fotograma" span={4} accent={C.lime}>
      {#snippet extra()}
        <span>últimos 10 s · mediana {median.toFixed(2)} ms</span><span style:color={C.red}>● tirón (&gt; 2× mediana)</span>
      {/snippet}
      {#key tr.pid}
        <FrameChart height={230} data={() => tr!.lastFrames(10)} median={() => tr!.median()} />
      {/key}
    </Panel>

    <Panel title="FPS durante la sesión" span={3} accent={C.lime}>
      {#snippet extra()}
        {#each fpsSeries as x}<span style:color={x.color}>{x.label}</span>{/each}
      {/snippet}
      {#key tr.pid}
        <TimeChart height={220} yFloor={60} series={fpsSeries} data={() => [metrics.view(metrics.h.gameFps.get(tr!.pid) ?? []), metrics.view(metrics.h.gameLow.get(tr!.pid) ?? [])]} />
      {/key}
    </Panel>
    <Panel title="Estabilidad" accent={C[label.color]}>
      <EChart height={170} option={neonGauge({ value: score, color: C[label.color], label: "ESTABILIDAD", text: v!.count >= 30 ? score.toFixed(0) : "—", sub: label.text })} />
      <dl class="kv">
        <dt>Constancia</dt><dd>{v!.avg ? ((v!.low1 / v!.avg) * 100).toFixed(0) : "—"} % <small>(1 % bajo / media)</small></dd>
        <dt>Ritmo</dt><dd>±{v!.jitter.toFixed(2)} ms</dd>
        <dt>Parones</dt><dd>{v!.hitches} <small>(&gt; 50 ms)</small></dd>
      </dl>
    </Panel>

    <Panel title="Qué limita los FPS" span={2} accent={verdict?.color ?? C.muted}>
      {#snippet extra()}<span style:color={C.magenta}>GPU</span><span style:color={C.cyan}>CPU</span>{/snippet}
      {#if verdict}
        <p class="verdict" style:color={verdict.color}>{verdict.text}</p>
        <p class="hint">{verdict.detail}</p>
      {:else}
        <p class="hint">{v?.ended ? "El juego se ha cerrado." : tr.info.active ? "Midiendo…" : "El juego está en pausa o minimizado."}</p>
      {/if}
      <TimeChart
        height={150}
        yMax={100}
        yFmt={(v) => `${v.toFixed(0)}%`}
        series={[
          { label: "GPU", color: C.magenta, width: 1.6 },
          { label: "CPU", color: C.cyan, width: 1.4 },
        ]}
        data={() => [metrics.view(metrics.h.gpus[0]?.busy ?? []), metrics.view(metrics.h.cpu.usage)]}
      />
    </Panel>
    <Panel title="Proceso del juego" span={2} accent={C.violet}>
      {#if proc}
        <dl class="kv wide">
          <dt>CPU</dt><dd>{proc.cpu.toFixed(0)} % <small>(de un núcleo)</small></dd>
          <dt>Hilos</dt><dd>{proc.threads}</dd>
          <dt>RAM</dt><dd>{fmtBytes(proc.mem)}</dd>
          <dt>VRAM</dt><dd>{vram ? fmtBytes(vram) : "—"}</dd>
          <dt>GPU</dt><dd>{gpuUse ? `${gpuUse.toFixed(0)} %` : "—"}</dd>
          <dt>Prioridad</dt><dd>nice {proc.nice}</dd>
        </dl>
      {:else}
        <p class="hint">No se ve el proceso {tr.pid} (puede estar en otro espacio de nombres, p. ej. dentro de un contenedor de Steam).</p>
      {/if}
      {#if s.gpus[0]}
        {@const g = s.gpus[0]}
        <dl class="kv wide">
          <dt>Reloj GPU</dt><dd>{g.gfxClock?.toFixed(0) ?? "—"} MHz</dd>
          <dt>Potencia</dt><dd>{g.power?.toFixed(0) ?? "—"} W</dd>
          <dt>Temp. GPU</dt><dd>{(g.tempHotspot ?? g.tempEdge)?.toFixed(0) ?? "—"} °C</dd>
          <dt>Temp. CPU</dt><dd>{s.cpu.temp?.toFixed(0) ?? "—"} °C</dd>
        </dl>
      {/if}
    </Panel>

    <Panel title="Distribución de tiempos" span={2} accent={C.lime}>
      {#snippet extra()}<span>escala logarítmica · rojo = tirón</span>{/snippet}
      {#if hist}<EChart height={220} option={hist} />{/if}
    </Panel>
    <Panel title="FPS por percentil" span={2} accent={C.amber}>
      {#snippet extra()}<span>sesión completa</span>{/snippet}
      {#if pct}<EChart height={220} option={pct} />{/if}
    </Panel>

    {#if games.lastBench && games.lastBench.exe === tr.info.exe}
      {@const b = games.lastBench}
      {@const prev = previous(b)}
      <Panel title="Último benchmark" span={4} accent={C.red}>
        {#snippet extra()}<span>{new Date(b.date).toLocaleString("es")} · {b.seconds.toFixed(0)} s</span>{/snippet}
        <div class="bench">
          {#each [["Media", b.avg, prev?.avg], ["1 % bajo", b.low1, prev?.low1], ["0,1 % bajo", b.low01, prev?.low01], ["Estabilidad", b.stability, prev?.stability]] as [name, v, p]}
            {@const d = delta(v as number, p as number | undefined)}
            <div class="bstat">
              <span>{name}</span>
              <b>{(v as number).toFixed(1)}</b>
              {#if d != null}<em class:up={d > 0} class:down={d < 0}>{d > 0 ? "▲" : "▼"} {Math.abs(d).toFixed(1)} %</em>{/if}
            </div>
          {/each}
          <div class="bstat"><span>Tirones</span><b>{b.stutters}</b><em>{b.hitches} parones</em></div>
          <div class="bstat"><span>GPU / CPU</span><b>{b.gpu.toFixed(0)} / {b.cpu.toFixed(0)} %</b><em>{b.gpuPower.toFixed(0)} W</em></div>
        </div>
        {#if prev}<p class="hint">Comparado con la grabación anterior de este juego ({new Date(prev.date).toLocaleString("es")}).</p>{/if}
      </Panel>
    {/if}

    {@render benchTable()}
    {#if platform.mode !== "web"}
      <Panel title="Medición automática" span={4} accent={C.lime}>
        <FpsAuto />
      </Panel>
    {/if}
  </div>
{/if}

{#snippet benchTable()}
  <Panel title="Historial" span={4} accent={C.cyan}>
    {#snippet extra()}<span>partidas y benchmarks · {games.benchmarks.length} {games.benchmarks.length === 1 ? "registro" : "registros"}</span>{/snippet}
    {#if games.benchmarks.length}
      <!-- un juego por tarjeta: pulsarla filtra sus registros -->
      <div class="glist">
        <button class="gcard" class:on={filterGame === null} onclick={() => (filterGame = null)}>
          <b>Todos</b><span>{games.benchmarks.length} registros</span>
        </button>
        {#each gameGroups as g (g.exe)}
          <button class="gcard" class:on={filterGame === g.exe} onclick={() => (filterGame = filterGame === g.exe ? null : g.exe)}>
            <b title={g.title}>{g.title}</b>
            <span>{g.sessions} {g.sessions === 1 ? "partida" : "partidas"}{g.benches ? ` · ${g.benches} bench.` : ""} · {fmtDuration(g.seconds)}</span>
            <span>mejor media <i>{g.best.toFixed(0)} FPS</i> · {new Date(g.last).toLocaleDateString("es", { day: "2-digit", month: "short" })}</span>
          </button>
        {/each}
      </div>
      <div class="tablewrap">
        <table>
          <thead>
            <tr><th>Juego</th><th>Tipo</th><th>Fecha</th><th>Duración</th><th>Media</th><th>1 %</th><th>0,1 %</th><th>Estab.</th><th>Tirones</th><th>GPU</th><th>CPU</th><th>W</th><th>Máx °C</th><th></th></tr>
          </thead>
          <tbody>
            {#each shownHistory as b (b.id)}
              <tr class="click" onclick={() => (games.viewing = b.id)} title="Ver la partida completa">
                <td class="gname" title={b.exe}>{b.title}</td>
                <td class="kind">{b.kind === "bench" ? "benchmark" : "partida"}</td>
                <td>{new Date(b.date).toLocaleString("es", { dateStyle: "short", timeStyle: "short" })}</td>
                <td>{fmtDuration(b.seconds)}</td>
                <td class="hl">{b.avg.toFixed(1)}</td>
                <td>{b.low1.toFixed(1)}</td>
                <td>{b.low01.toFixed(1)}</td>
                <td style:color={C[stabilityLabel(b.stability, b.frames).color]}>{b.stability.toFixed(0)}</td>
                <td>{b.stutters}</td>
                <td>{b.gpu.toFixed(0)} %</td>
                <td>{b.cpu.toFixed(0)} %</td>
                <td>{b.gpuPower.toFixed(0)}</td>
                <td>{b.cpuTempMax.toFixed(0)} / {b.gpuTempMax.toFixed(0)}</td>
                <td class="btns">
                  {#if b.kind === "bench"}<button onclick={(e) => (e.stopPropagation(), exportBench(b))} title="Exportar tiempos de fotograma (CSV)">CSV</button>{/if}
                  {#if platform.desktop || platform.webActions}
                    <button onclick={(e) => (e.stopPropagation(), confirm(`¿Borrar este registro de ${b.title}?`) && games.remove(b.id))} title="Borrar">✕</button>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if exportMsg}<p class="hint">{exportMsg}</p>{/if}
      <p class="hint">
        Pulsa un registro para ver la partida completa: FPS durante toda la partida, tirones, CPU, GPU, temperaturas, distribución de tiempos y la
        comparación con tus otras partidas. Se guarda en este equipo (las partidas, solas al cerrar el juego si duran más de 10 s).
      </p>
    {:else}
      <p class="hint">Aún no hay nada. Cada partida se guarda sola al cerrar el juego; para una medición controlada, pulsa «Grabar benchmark».</p>
    {/if}
  </Panel>
{/snippet}

<style>
  .autobox {
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--lime) 30%, transparent);
    background: color-mix(in srgb, var(--lime) 5%, transparent);
  }
  .manual {
    margin: 4px 0 0;
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: 10px;
  }
  .picker {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .picker button {
    display: grid;
    text-align: left;
    gap: 2px;
    font: inherit;
    padding: 6px 12px;
    background: transparent;
    color: var(--text);
    border: 1px solid color-mix(in srgb, var(--lime) 30%, transparent);
    cursor: pointer;
  }
  .picker button.on {
    background: color-mix(in srgb, var(--lime) 14%, transparent);
    border-color: var(--lime);
  }
  .picker span {
    font-size: 10px;
    color: var(--muted);
  }
  .gamehead {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
  }
  .gamehead h2 {
    margin: 0;
    font-family: var(--display);
    font-size: 22px;
    color: var(--lime);
    text-shadow: 0 0 12px color-mix(in srgb, var(--lime) calc(var(--glow-a) * 0.6), transparent);
  }
  .gamehead .title span {
    font-size: 11px;
    color: var(--muted);
  }
  .ended {
    color: var(--cyan);
    margin-left: 8px;
    letter-spacing: 0.1em;
  }
  .endnote {
    font-size: 11px;
    color: var(--muted);
  }
  td.kind {
    text-align: left;
    color: var(--muted);
  }
  .paused {
    color: var(--amber);
    margin-left: 8px;
    letter-spacing: 0.1em;
  }
  .actions {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }
  .actions button,
  .actions select,
  .btns button {
    font: inherit;
    font-size: 12px;
    padding: 6px 12px;
    background: transparent;
    color: var(--cyan);
    border: 1px solid color-mix(in srgb, var(--cyan) 40%, transparent);
    cursor: pointer;
  }
  .actions select {
    background: var(--bg);
  }
  .actions .rec {
    color: var(--red);
    border-color: color-mix(in srgb, var(--red) 60%, transparent);
  }
  .actions .stop {
    background: var(--red);
    color: var(--bg);
    border-color: var(--red);
  }
  .recording {
    color: var(--red);
    font-size: 12px;
    letter-spacing: 0.08em;
    animation: blink 1.2s steps(2) infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0.45;
    }
  }
  .verdict {
    font-family: var(--display);
    font-size: 15px;
    margin: 0 0 4px;
  }
  .kv {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 5px 12px;
    margin: 6px 0 0;
    font-size: 12px;
  }
  .kv.wide {
    grid-template-columns: auto 1fr auto 1fr;
  }
  .kv dt {
    color: var(--muted);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    align-self: center;
  }
  .kv dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  small {
    color: var(--muted);
  }
  .hint {
    font-size: 11px;
    color: var(--muted);
    margin: 8px 0 0;
    line-height: 1.6;
  }
  .setup p {
    line-height: 1.6;
    margin: 0 0 10px;
  }
  .how {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 14px;
    margin: 12px 0;
  }
  .how h4 {
    margin: 0 0 6px;
    color: var(--lime);
    font-family: var(--display);
    font-size: 12px;
    letter-spacing: 0.1em;
  }
  .how p {
    font-size: 12px;
    color: var(--muted);
    margin: 0 0 6px;
  }
  code {
    color: var(--cyan);
  }
  .cmd {
    display: block;
    padding: 8px 10px;
    background: color-mix(in srgb, var(--cyan) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--cyan) 30%, transparent);
    font-size: 13px;
    user-select: all;
  }
  .status .ok {
    color: var(--lime);
  }
  .status .warn {
    color: var(--amber);
  }
  .bench {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
    gap: 12px;
  }
  .bstat {
    display: grid;
    gap: 2px;
  }
  .bstat span {
    font-size: 10px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }
  .bstat b {
    font-family: var(--display);
    font-size: 20px;
  }
  .bstat em {
    font-style: normal;
    font-size: 11px;
    color: var(--muted);
  }
  .bstat em.up {
    color: var(--lime);
  }
  .bstat em.down {
    color: var(--red);
  }
  .glist {
    display: flex;
    gap: 8px;
    overflow-x: auto;
    padding-bottom: 6px;
    margin-bottom: 8px;
  }
  .gcard {
    display: grid;
    gap: 2px;
    text-align: left;
    flex: 0 0 auto;
    min-width: 150px;
    max-width: 230px;
    font: inherit;
    padding: 7px 12px;
    background: color-mix(in srgb, var(--cyan) 4%, transparent);
    color: var(--text);
    border: 1px solid color-mix(in srgb, var(--cyan) 22%, transparent);
    cursor: pointer;
  }
  .gcard.on {
    border-color: var(--cyan);
    background: color-mix(in srgb, var(--cyan) 14%, transparent);
  }
  .gcard b {
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .gcard span {
    font-size: 10px;
    color: var(--muted);
  }
  .gcard i {
    font-style: normal;
    color: var(--lime);
  }
  tr.click {
    cursor: pointer;
  }
  tr.click:hover td {
    background: color-mix(in srgb, var(--cyan) 7%, transparent);
  }
  .tablewrap {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  th {
    text-align: right;
    color: var(--muted);
    font-weight: normal;
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    padding: 0 8px 6px;
  }
  td {
    text-align: right;
    padding: 4px 8px;
    border-top: 1px solid color-mix(in srgb, var(--cyan) 8%, transparent);
  }
  th:first-child,
  td:first-child,
  th:nth-child(2),
  td:nth-child(2),
  th:nth-child(3),
  td:nth-child(3) {
    text-align: left;
  }
  .gname {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  td.hl {
    color: var(--lime);
  }
  .btns {
    display: flex;
    gap: 4px;
    justify-content: flex-end;
  }
  .btns button {
    padding: 1px 7px;
    font-size: 10px;
  }
  @media (max-width: 700px) {
    .kv.wide {
      grid-template-columns: auto 1fr;
    }
  }
</style>
