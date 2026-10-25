import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { metrics } from "./store.svelte";
import { platform } from "./platform.svelte";
import type { Series } from "./series";

/**
 * Exporta el historial a CSV (valores sin reducir, en unidades base: %, bytes, bytes/s, °C,
 * MHz, W). `range = "window"` exporta solo la ventana visible.
 */
export async function exportCsv(range: "window" | "all"): Promise<string | null> {
  const h = metrics.h;
  const s = metrics.snap;
  if (!s || !h.t.length) throw new Error("Todavía no hay datos que exportar");

  const cols: { name: string; data: Series }[] = [];
  const add = (name: string, data: Series | undefined) => data && cols.push({ name, data });

  for (const k of ["usage", "user", "system", "iowait", "irq", "softirq", "steal", "nice"] as const) add(`cpu_${k}_pct`, h.cpu[k]);
  add("cpu_temp_c", h.temp);
  add("cpu_freq_media_mhz", h.freqAvg);
  h.load.forEach((l, i) => add(`carga_${[1, 5, 15][i]}m`, l));
  h.cores.forEach((c, i) => add(`nucleo${i}_pct`, c));
  for (const k of ["apps", "shmem", "buffers", "cache", "free"] as const) add(`ram_${k}_bytes`, h.mem[k]);
  add("swap_usada_bytes", h.swap);
  add("psi_cpu_pct", h.psi.cpu);
  add("psi_mem_pct", h.psi.mem);
  add("psi_io_pct", h.psi.io);
  for (const i of s.net.ifaces) {
    if (i.kind !== "ethernet" && i.kind !== "wifi") continue;
    const nh = h.net.get(i.name);
    add(`red_${i.name}_bajada_bps`, nh?.rx);
    add(`red_${i.name}_subida_bps`, nh?.tx);
  }
  for (const d of s.disks.disks) {
    if (d.kind === "virtual") continue;
    const dh = h.disks.get(d.name);
    add(`disco_${d.name}_lectura_bps`, dh?.r);
    add(`disco_${d.name}_escritura_bps`, dh?.w);
    add(`disco_${d.name}_ocupado_pct`, dh?.util);
  }
  s.gpus.forEach((g, i) => {
    const gh = h.gpus[i];
    if (!gh) return;
    add(`gpu${i}_uso_pct`, gh.busy);
    add(`gpu${i}_vram_bytes`, gh.vramUsed);
    add(`gpu${i}_trafico_vram_bps`, gh.vramBw);
    add(`gpu${i}_potencia_w`, gh.power);
    add(`gpu${i}_temp_c`, gh.tempEdge);
  });
  for (const c of s.sensors.chips) {
    for (const r of c.readings) {
      if (r.kind !== "temp" && r.kind !== "fan") continue;
      const slug = `${c.label}_${r.label}`.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "");
      add(`sensor_${slug}_${r.kind === "temp" ? "c" : "rpm"}`, h.sensors.get(r.id));
    }
  }

  const start = range === "window" ? metrics.startIdx : 0;
  const t = h.t.view();
  const views = cols.map((c) => c.data.view());
  const lines = ["fecha," + cols.map((c) => c.name).join(",")];
  for (let i = start; i < h.t.length; i++) {
    const row = [new Date(t[i] * 1000).toISOString()];
    for (const v of views) row.push(Number.isFinite(v[i]) ? String(Math.round(v[i] * 1000) / 1000) : "");
    lines.push(row.join(","));
  }

  const stamp = new Date().toISOString().slice(0, 16).replace(/[-:T]/g, "");
  return saveTextFile(`neontop-${stamp}.csv`, lines.join("\n") + "\n");
}

/**
 * Guarda un fichero de texto: en la app, con el diálogo de guardar del sistema; en el
 * navegador, como descarga. Devuelve la ruta (o el nombre) o null si se canceló.
 */
export async function saveTextFile(name: string, contents: string): Promise<string | null> {
  if (!platform.desktop) {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([contents], { type: "text/csv" }));
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 5000);
    return name;
  }
  const path = await save({ defaultPath: name, filters: [{ name: "CSV", extensions: ["csv"] }] });
  if (!path) return null;
  await invoke("write_text_file", { path, contents });
  return path;
}
