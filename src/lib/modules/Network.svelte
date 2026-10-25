<script lang="ts">
  import EChart from "$lib/charts/EChart.svelte";
  import TimeChart from "$lib/charts/TimeChart.svelte";
  import Sparkline from "$lib/charts/Sparkline.svelte";
  import { tooltipBase } from "$lib/charts/echarts";
  import Panel from "$lib/ui/Panel.svelte";
  import Stat from "$lib/ui/Stat.svelte";
  import { metrics } from "$lib/store.svelte";
  import { C, MONO, rgba } from "$lib/theme";
  import { fmtBits, fmtBytes, fmtNum, fmtRate } from "$lib/format";
  import type { IfaceSnapshot } from "$lib/types";
  import type { SeriesLike } from "$lib/series";

  let s = $derived(metrics.snap!);
  let showVirtual = $state(false);

  const KIND_ORDER = { ethernet: 0, wifi: 1, virtual: 2, loopback: 3 };
  const KIND_ICON = { ethernet: "⌬", wifi: "◉", virtual: "◇", loopback: "↻" };

  let ifaces = $derived(
    s.net.ifaces
      .filter((i) => showVirtual || i.kind === "ethernet" || i.kind === "wifi")
      .toSorted((a, b) => KIND_ORDER[a.kind] - KIND_ORDER[b.kind] || (b.state === "up" ? 1 : 0) - (a.state === "up" ? 1 : 0) || a.name.localeCompare(b.name)),
  );

  let selectedName = $state<string | null>(null);
  // Por defecto, la interfaz física con más tráfico acumulado.
  let sel = $derived<IfaceSnapshot | undefined>(
    ifaces.find((i) => i.name === selectedName) ?? ifaces.toSorted((a, b) => b.rxTotal + b.txTotal - (a.rxTotal + a.txTotal))[0],
  );
  let hist = $derived(sel ? metrics.h.net.get(sel.name) : undefined);

  function series(key: "rx" | "tx" | "rxp" | "txp") {
    return () => {
      const nh = sel && metrics.h.net.get(sel.name);
      return nh ? metrics.view(nh[key]) : [];
    };
  }
  const rx = series("rx");
  const tx = series("tx");
  const rxp = series("rxp");
  const txp = series("txp");

  function peak(arr: SeriesLike | undefined) {
    return arr ? metrics.raw(arr).reduce((a, b) => Math.max(a, b), 0) : 0;
  }

  // Rosa de Nightingale: tráfico de la sesión por interfaz.
  let rose = $derived({
    tooltip: { ...tooltipBase, formatter: (p: any) => `${p.name}<br><b>${fmtBytes(p.value)}</b> (${p.percent.toFixed(1)}%)` },
    series: [
      {
        type: "pie",
        roseType: "radius",
        radius: ["14%", "80%"],
        center: ["50%", "52%"],
        itemStyle: { borderRadius: 4, borderColor: C.bg, borderWidth: 2 },
        label: { color: C.muted, fontFamily: MONO, fontSize: 10 },
        labelLine: { lineStyle: { color: rgba(C.cyan, 0.3) } },
        data: s.net.ifaces
          .filter((i) => i.kind !== "loopback")
          .map((i, k) => {
            const nh = metrics.h.net.get(i.name);
            const v = nh ? i.rxTotal - nh.baseRx + (i.txTotal - nh.baseTx) : 0;
            const color = [C.cyan, C.magenta, C.lime, C.amber, C.violet, C.teal, C.blue, C.orange][k % 8];
            return { name: i.name, value: v, itemStyle: { color, shadowColor: color, shadowBlur: 10 } };
          })
          .filter((d) => d.value > 0),
      },
    ],
    animationDurationUpdate: 400,
  });

  // Anillos concéntricos con el uso del enlace (si el driver informa velocidad).
  let linkUse = $derived.by(() => {
    const cap = sel?.speed ? (sel.speed * 1e6) / 8 : null;
    const pct = (v: number) => (cap ? Math.min(100, (v / cap) * 100) : 0);
    const ring = (name: string, v: number, color: string, r: [string, string]) => ({
      type: "pie",
      radius: r,
      startAngle: 90,
      silent: true,
      label: { show: false },
      data: [
        { value: pct(v), name, itemStyle: { color, shadowColor: color, shadowBlur: 10 } },
        { value: 100 - pct(v), itemStyle: { color: rgba(color, 0.08) } },
      ],
    });
    return {
      graphic: [
        { type: "text", left: "center", top: "42%", style: { text: cap ? `${sel!.speed} Mbps` : "velocidad N/D", fill: C.text, font: `12px ${MONO}` } },
        {
          type: "text",
          left: "center",
          top: "52%",
          style: { text: cap ? `▼${pct(sel!.rxRate).toFixed(1)}% ▲${pct(sel!.txRate).toFixed(1)}%` : "", fill: C.muted, font: `10px ${MONO}` },
        },
      ],
      series: [ring("bajada", sel?.rxRate ?? 0, C.cyan, ["70%", "80%"]), ring("subida", sel?.txRate ?? 0, C.magenta, ["56%", "66%"])],
      animationDurationUpdate: 300,
    };
  });
</script>

<div class="grid">
  <div class="bar" style="grid-column: span 4">
    {#each ifaces as i (i.name)}
      <button class="chip" class:active={sel?.name === i.name} class:down={i.state !== "up" && i.state !== "unknown"} onclick={() => (selectedName = i.name)}>
        <span class="icon">{KIND_ICON[i.kind]}</span>
        <span class="name">{i.name}</span>
        <span class="rates"><em class="rx">▼{fmtRate(i.rxRate)}</em><em class="tx">▲{fmtRate(i.txRate)}</em></span>
      </button>
    {/each}
    <label class="toggle"><input type="checkbox" bind:checked={showVirtual} /> mostrar virtuales</label>
  </div>

  {#if sel}
    <div class="stats" style="grid-column: span 4">
      <Stat label="Bajada" value={fmtRate(sel.rxRate)} sub={fmtBits(sel.rxRate)} color={C.cyan} history={hist?.rx} />
      <Stat label="Subida" value={fmtRate(sel.txRate)} sub={fmtBits(sel.txRate)} color={C.magenta} history={hist?.tx} />
      <Stat label="Pico bajada" value={fmtRate(peak(hist?.rx))} sub="en la ventana" color={C.cyan} />
      <Stat label="Pico subida" value={fmtRate(peak(hist?.tx))} sub="en la ventana" color={C.magenta} />
      <Stat label="Sesión ▼" value={fmtBytes(sel.rxTotal - (hist?.baseRx ?? sel.rxTotal))} sub={`desde arranque ${fmtBytes(sel.rxTotal)}`} color={C.teal} />
      <Stat label="Sesión ▲" value={fmtBytes(sel.txTotal - (hist?.baseTx ?? sel.txTotal))} sub={`desde arranque ${fmtBytes(sel.txTotal)}`} color={C.violet} />
      <Stat label="Paquetes/s" value={`${fmtNum(sel.rxPktRate)} / ${fmtNum(sel.txPktRate)}`} sub="entrada / salida" color={C.lime} history={hist?.rxp} />
      <Stat label="Errores" value={`${sel.rxErrs + sel.txErrs}`} sub={`descartes ${sel.rxDrop + sel.txDrop}`} color={sel.rxErrs + sel.txErrs > 0 ? C.red : C.muted} />
    </div>

    <Panel title="Ancho de banda · {sel.name}" span={3} accent={C.cyan}>
      {#snippet extra()}<span style:color={C.cyan}>▼ bajada</span><span style:color={C.magenta}>▲ subida</span>{/snippet}
      {#key sel.name}
        <TimeChart
          height={260}
          mirrored
          yFmt={(v) => fmtBytes(v, 0) + "/s"}
          series={[
            { label: "bajada", color: C.cyan, fill: true, width: 2 },
            { label: "subida", color: C.magenta, fill: true, width: 2, negate: true },
          ]}
          data={() => [rx(), tx()]}
        />
      {/key}
    </Panel>
    <Panel title="Interfaz" accent={C.blue}>
      <dl class="info">
        <dt>Tipo</dt><dd>{sel.kind}</dd>
        <dt>Estado</dt><dd class:up={sel.state === "up"}>{sel.state}</dd>
        <dt>MAC</dt><dd>{sel.mac || "—"}</dd>
        <dt>MTU</dt><dd>{sel.mtu}</dd>
        <dt>Enlace</dt><dd>{sel.speed ? `${sel.speed} Mbps` : "—"}</dd>
        <dt>IPs</dt><dd>{#each sel.addrs as a}<div>{a}</div>{:else}—{/each}</dd>
      </dl>
    </Panel>

    <Panel title="Paquetes por segundo" span={2} accent={C.lime}>
      {#key sel.name}
        <TimeChart
          height={200}
          mirrored
          yFmt={(v) => fmtNum(v)}
          series={[
            { label: "entrada", color: C.lime, fill: true },
            { label: "salida", color: C.amber, fill: true, negate: true },
          ]}
          data={() => [rxp(), txp()]}
        />
      {/key}
    </Panel>
    <Panel title="Uso del enlace" accent={C.cyan} half>
      <EChart height={210} option={linkUse} />
    </Panel>
    <Panel title="Tráfico" accent={C.magenta} half>
      {#snippet extra()}<span>de la sesión, por interfaz</span>{/snippet}
      <EChart height={210} option={rose} />
    </Panel>
  {/if}

  <Panel title="Todas las interfaces" span={3} accent={C.teal}>
    <div class="cards">
      {#each ifaces as i (i.name)}
        {@const nh = metrics.h.net.get(i.name)}
        <button class="card" class:active={sel?.name === i.name} onclick={() => (selectedName = i.name)}>
          <div class="top"><b>{KIND_ICON[i.kind]} {i.name}</b><span class:up={i.state === "up"}>{i.state}</span></div>
          {#if nh}
            <Sparkline values={nh.rx} color={C.cyan} height={24} />
            <Sparkline values={nh.tx} color={C.magenta} height={24} />
          {/if}
          <div class="bottom"><span class="rx">▼{fmtRate(i.rxRate)}</span><span class="tx">▲{fmtRate(i.txRate)}</span></div>
        </button>
      {/each}
    </div>
  </Panel>
  <Panel title="Sockets" accent={C.violet}>
    <div class="sockets">
      <Stat label="TCP establecidos" value={String(s.net.tcpEstablished)} color={C.cyan} />
      <Stat label="TCP escuchando" value={String(s.net.tcpListen)} color={C.lime} />
      <Stat label="UDP" value={String(s.net.udpSockets)} color={C.amber} />
    </div>
  </Panel>
</div>

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .chip {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    background: var(--panel);
    border: 1px solid color-mix(in srgb, var(--cyan) 20%, transparent);
    color: var(--text);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    clip-path: polygon(8px 0, 100% 0, 100% calc(100% - 8px), calc(100% - 8px) 100%, 0 100%, 0 8px);
  }
  .chip.active {
    border-color: var(--cyan);
    background: color-mix(in srgb, var(--cyan) 10%, transparent);
    box-shadow: inset 0 0 12px color-mix(in srgb, var(--cyan) 20%, transparent);
  }
  .chip.down {
    opacity: 0.5;
  }
  .icon {
    color: var(--cyan);
  }
  .rates {
    display: flex;
    gap: 8px;
    font-size: 10px;
  }
  em {
    font-style: normal;
  }
  .rx {
    color: var(--cyan);
  }
  .tx {
    color: var(--magenta);
  }
  .toggle {
    margin-left: auto;
    font-size: 11px;
    color: var(--muted);
    display: flex;
    gap: 6px;
    align-items: center;
    cursor: pointer;
  }
  .toggle input {
    accent-color: var(--cyan);
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: 10px;
  }
  .info {
    display: grid;
    grid-template-columns: 70px 1fr;
    gap: 7px 10px;
    margin: 4px 0 0;
    font-size: 12px;
  }
  .info dt {
    color: var(--muted);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }
  .info dd {
    margin: 0;
    word-break: break-all;
  }
  .up {
    color: var(--lime);
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 10px;
  }
  .card {
    text-align: left;
    background: color-mix(in srgb, var(--cyan) 3%, transparent);
    border: 1px solid color-mix(in srgb, var(--cyan) 15%, transparent);
    padding: 8px;
    color: var(--text);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }
  .card.active {
    border-color: var(--cyan);
    box-shadow: 0 0 10px color-mix(in srgb, var(--cyan) 25%, transparent);
  }
  .card .top,
  .card .bottom {
    display: flex;
    justify-content: space-between;
    margin: 2px 0 4px;
  }
  .card .top span {
    color: var(--muted);
  }
  .sockets {
    display: grid;
    gap: 14px;
    padding-top: 6px;
  }
</style>
