const UNITS = ["B", "KiB", "MiB", "GiB", "TiB"];

export function fmtBytes(b: number, digits = 1): string {
  let i = 0;
  let v = Math.abs(b);
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  return `${(b < 0 ? -v : v).toFixed(i === 0 ? 0 : digits)} ${UNITS[i]}`;
}

/** Velocidad en bytes/s. */
export function fmtRate(bps: number): string {
  return `${fmtBytes(bps)}/s`;
}

/** Velocidad en bits/s (lo habitual para enlaces de red). */
export function fmtBits(bps: number): string {
  const bits = Math.abs(bps) * 8;
  const u = ["b", "Kb", "Mb", "Gb"];
  let i = 0;
  let v = bits;
  while (v >= 1000 && i < u.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v.toFixed(i === 0 ? 0 : 1)} ${u[i]}ps`;
}

export const fmtPct = (v: number, d = 0) => `${v.toFixed(d)}%`;

export function fmtFreq(mhz: number): string {
  return mhz >= 1000 ? `${(mhz / 1000).toFixed(2)} GHz` : `${mhz.toFixed(0)} MHz`;
}

export function fmtNum(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}k`;
  return n.toFixed(0);
}

export function fmtDuration(sec: number): string {
  const d = Math.floor(sec / 86400);
  const h = Math.floor((sec % 86400) / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = Math.floor(sec % 60);
  const hms = `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  return d > 0 ? `${d}d ${hms}` : hms;
}

/** "usado / total" compacto: si comparten unidad, esta aparece una sola vez ("11.7 / 32.0 GiB"). */
export function fmtBytesOf(used: number, total: number): string {
  // sin decimales a partir de 100 ("512 MiB"): el texto tiene que caber dentro de un velocímetro
  const short = (b: number) => {
    const [v, unit] = fmtBytes(b).split(" ");
    return [Number(v) >= 100 ? String(Math.round(Number(v))) : v, unit];
  };
  const [u, uu] = short(used);
  const [t, tu] = short(total);
  return uu === tu ? `${u} / ${t} ${tu}` : `${u} ${uu} / ${t} ${tu}`;
}

/** Ancho de banda en unidades decimales (como los fabricantes y el test de RAM): GB/s o MB/s. */
export function fmtGBs(bps: number, digits = 1): string {
  return bps >= 1e9 ? `${(bps / 1e9).toFixed(digits)} GB/s` : `${(bps / 1e6).toFixed(0)} MB/s`;
}
