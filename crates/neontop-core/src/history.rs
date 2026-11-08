//! Historial de métricas para las gráficas.
//!
//! Cada serie es un `Vec<f32>` alineado con el vector de instantes `t`: la muestra `i` de cada
//! serie corresponde a `t[i]`. Las series se crean al ver un valor por primera vez (un núcleo,
//! una interfaz, un disco USB que se conecta) rellenas con ceros hasta ese momento, y las que no
//! reciben valor en una muestra (el disco se desconectó) reciben un cero, así siguen alineadas.
//!
//! Al llegar a `MAX_POINTS` muestras la mitad más antigua se compacta a la mitad de resolución
//! (media de pares), como una base de datos RRD: el historial abarca un tiempo ilimitado con
//! memoria acotada; lo reciente se conserva completo y lo antiguo se va haciendo más grueso.

use std::collections::HashMap;

use crate::sampler::Snapshot;

/// Tope de muestras por serie antes de compactar.
pub const MAX_POINTS: usize = 40_000;
/// Máximo de puntos que se dibujan por serie; por encima se agrupan en cubos.
pub const MAX_DRAW: usize = 1600;

/// Tramo visible de las series: desde `start`, agrupando de `stride` en `stride`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub start: usize,
    pub stride: usize,
}

#[derive(Default)]
pub struct History {
    t: Vec<f64>,
    series: Vec<Vec<f32>>,
    index: HashMap<String, usize>,
    /// Nº de compactaciones: si es > 0, lo más antiguo está a resolución reducida.
    pub compactions: u32,
}

/// Reparto de la RAM al estilo htop; los segmentos más `free` suman `total`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MemSegments {
    pub apps: u64,
    pub shmem: u64,
    pub buffers: u64,
    pub cache: u64,
    pub free: u64,
    pub used: u64,
}

pub fn mem_segments(m: &crate::collectors::memory::MemSnapshot) -> MemSegments {
    let cache = (m.cached + m.s_reclaimable).saturating_sub(m.shmem);
    let used = m.total.saturating_sub(m.free + m.buffers + cache);
    let apps = used.saturating_sub(m.shmem);
    MemSegments { apps, shmem: m.shmem, buffers: m.buffers, cache, free: m.free, used }
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Empieza una muestra nueva en el instante `ts` (segundos).
    pub fn begin(&mut self, ts: f64) {
        self.t.push(ts);
    }

    /// Valor de la serie `key` en la muestra actual.
    pub fn push(&mut self, key: &str, v: f32) {
        let n = self.t.len();
        // sin `begin` no hay muestra a la que asignar el valor
        if n == 0 {
            return;
        }
        let i = match self.index.get(key) {
            Some(&i) => i,
            None => {
                // serie nueva: ceros hasta la muestra anterior
                self.series.push(vec![0.0; n.saturating_sub(1)]);
                self.index.insert(key.to_string(), self.series.len() - 1);
                self.series.len() - 1
            }
        };
        let s = &mut self.series[i];
        // dos valores en la misma muestra: vale el último
        if s.len() == n {
            *s.last_mut().unwrap() = v;
        } else {
            s.push(v);
        }
    }

    /// Termina la muestra: las series sin valor reciben 0 y, si toca, se compacta.
    pub fn end(&mut self) {
        let n = self.t.len();
        for s in &mut self.series {
            s.resize(n, 0.0);
        }
        if n >= MAX_POINTS {
            self.compact((n / 2) & !1);
        }
    }

    fn compact(&mut self, half: usize) {
        fn pairs<T: Copy + Into<f64>>(v: &mut Vec<T>, half: usize, from: impl Fn(f64) -> T) {
            let merged = half / 2;
            for i in 0..merged {
                v[i] = from((v[2 * i].into() + v[2 * i + 1].into()) / 2.0);
            }
            v.drain(merged..merged * 2);
            v.shrink_to(v.len() * 2);
        }
        pairs(&mut self.t, half, |x| x);
        for s in &mut self.series {
            pairs(s, half, |x| x as f32);
        }
        self.compactions += 1;
    }

    pub fn len(&self) -> usize {
        self.t.len()
    }

    pub fn is_empty(&self) -> bool {
        self.t.is_empty()
    }

    pub fn t(&self) -> &[f64] {
        &self.t
    }

    pub fn get(&self, key: &str) -> Option<&[f32]> {
        self.index.get(key).map(|&i| self.series[i].as_slice())
    }

    /// Claves que empiezan por `prefix`, en orden de creación.
    pub fn keys_with_prefix<'a>(&'a self, prefix: &'a str) -> Vec<&'a str> {
        let mut v: Vec<(&str, usize)> = self.index.iter().filter(|(k, _)| k.starts_with(prefix)).map(|(k, &i)| (k.as_str(), i)).collect();
        v.sort_by_key(|&(_, i)| i);
        v.into_iter().map(|(k, _)| k).collect()
    }

    /// Tramo visible para una ventana de `window_sec` segundos (`None` = todo el historial).
    pub fn window(&self, window_sec: Option<f64>) -> Window {
        let start = match (window_sec, self.t.last()) {
            (Some(w), Some(&last)) => self.t.partition_point(|&x| x < last - w),
            _ => 0,
        };
        let stride = (self.t.len() - start).div_ceil(MAX_DRAW).max(1);
        Window { start, stride }
    }

    /// Serie recortada a la ventana y reducida a ≤ MAX_DRAW puntos (media por cubo).
    pub fn view(&self, key: &str, w: Window) -> Vec<f32> {
        self.get(key).map(|s| bucket(&s[w.start.min(s.len())..], w.stride, false)).unwrap_or_default()
    }

    /// Como `view` pero conservando el máximo de cada cubo (picos de temperatura, tirones).
    pub fn view_max(&self, key: &str, w: Window) -> Vec<f32> {
        self.get(key).map(|s| bucket(&s[w.start.min(s.len())..], w.stride, true)).unwrap_or_default()
    }

    /// Instantes de la ventana, reducidos igual que `view`.
    pub fn view_t(&self, w: Window) -> Vec<f64> {
        let t = &self.t[w.start.min(self.t.len())..];
        t.chunks(w.stride).map(|c| c.iter().sum::<f64>() / c.len() as f64).collect()
    }

    /// Serie recortada a la ventana, sin reducir (para picos y medias exactas).
    pub fn raw(&self, key: &str, w: Window) -> &[f32] {
        self.get(key).map(|s| &s[w.start.min(s.len())..]).unwrap_or(&[])
    }

    /// Muestras guardadas, instante más antiguo, bytes reservados y nº de series.
    pub fn stored(&self) -> (usize, f64, usize, usize) {
        let bytes = self.t.capacity() * 8 + self.series.iter().map(|s| s.capacity() * 4).sum::<usize>();
        (self.t.len(), self.t.first().copied().unwrap_or(0.0), bytes, self.series.len())
    }

    /// CSV con los valores sin reducir desde `start` (una columna por serie, en el orden en que
    /// aparecieron), con la hora local en la primera columna.
    pub fn to_csv(&self, start: usize) -> String {
        let mut keys: Vec<(&String, usize)> = self.index.iter().map(|(k, &i)| (k, i)).collect();
        keys.sort_by_key(|&(_, i)| i);
        let mut out = String::from("tiempo");
        for (k, _) in &keys {
            out.push(',');
            out.push_str(k);
        }
        out.push('\n');
        for row in start.min(self.t.len())..self.t.len() {
            let ms = (self.t[row] * 1000.0) as u64;
            out.push_str(&format!("{} {}", crate::format::day_key(ms), crate::format::clock(ms)));
            for (_, i) in &keys {
                let v = self.series[*i].get(row).copied().unwrap_or(0.0);
                out.push(',');
                if v.fract() == 0.0 { out.push_str(&format!("{v:.0}")) } else { out.push_str(&format!("{v}")) }
            }
            out.push('\n');
        }
        out
    }

    pub fn clear(&mut self) {
        self.t.clear();
        for s in &mut self.series {
            s.clear();
        }
        self.compactions = 0;
    }

    /// Añade todas las métricas generales de una muestra.
    pub fn ingest(&mut self, s: &Snapshot) {
        self.begin(s.ts as f64 / 1000.0);
        let c = &s.cpu;
        for (k, v) in [
            ("cpu.usage", c.total.usage),
            ("cpu.user", c.total.user),
            ("cpu.nice", c.total.nice),
            ("cpu.system", c.total.system),
            ("cpu.iowait", c.total.iowait),
            ("cpu.irq", c.total.irq),
            ("cpu.softirq", c.total.softirq),
            ("cpu.steal", c.total.steal),
        ] {
            self.push(k, v);
        }
        for (i, core) in c.cores.iter().enumerate() {
            self.push(&format!("core.{i}"), core.usage);
        }
        for (i, f) in c.freqs.iter().enumerate() {
            self.push(&format!("freq.{i}"), *f);
        }
        self.push("freq.avg", c.freqs.iter().sum::<f32>() / c.freqs.len().max(1) as f32);
        self.push("cpu.temp", c.temp.unwrap_or(0.0));
        for (i, l) in c.load_avg.iter().enumerate() {
            self.push(&format!("load.{i}"), *l);
        }
        self.push("cpu.ctxt", c.ctxt_rate);
        self.push("cpu.intr", c.intr_rate);
        self.push("cpu.fork", c.fork_rate);
        self.push("cpu.running", c.procs_running as f32);

        let m = &s.mem;
        let seg = mem_segments(m);
        self.push("mem.apps", seg.apps as f32);
        self.push("mem.shmem", seg.shmem as f32);
        self.push("mem.buffers", seg.buffers as f32);
        self.push("mem.cache", seg.cache as f32);
        self.push("mem.free", seg.free as f32);
        self.push("mem.swap", m.swap_total.saturating_sub(m.swap_free) as f32);
        self.push("mem.dirty", m.dirty as f32);
        self.push("mem.writeback", m.writeback as f32);
        self.push("membw.total", s.membw.total.unwrap_or(0.0) as f32);
        self.push("membw.read", s.membw.read.unwrap_or(0.0) as f32);
        self.push("membw.write", s.membw.write.unwrap_or(0.0) as f32);

        let p = &s.pressure;
        self.push("psi.cpu", p.cpu.some);
        self.push("psi.mem", p.memory.some);
        self.push("psi.memFull", p.memory.full);
        self.push("psi.io", p.io.some);
        self.push("psi.ioFull", p.io.full);

        for (i, g) in s.gpus.iter().enumerate() {
            let k = |n: &str| format!("gpu.{i}.{n}");
            self.push(&k("busy"), g.busy.unwrap_or(0.0));
            self.push(&k("memBusy"), g.mem_busy.unwrap_or(0.0));
            self.push(&k("vramBw"), g.vram_bw.unwrap_or(0.0) as f32);
            self.push(&k("vramUsed"), g.vram_used as f32);
            self.push(&k("gttUsed"), g.gtt_used as f32);
            self.push(&k("power"), g.power.unwrap_or(0.0));
            self.push(&k("tempEdge"), g.temp_edge.unwrap_or(0.0));
            self.push(&k("tempHotspot"), g.temp_hotspot.unwrap_or(0.0));
            self.push(&k("tempMem"), g.temp_mem.unwrap_or(0.0));
            self.push(&k("gfxClock"), g.gfx_clock.unwrap_or(0.0));
            self.push(&k("memClock"), g.mem_clock.unwrap_or(0.0));
            self.push(&k("pcieRx"), g.pcie_rx.unwrap_or(0.0) as f32);
            self.push(&k("pcieTx"), g.pcie_tx.unwrap_or(0.0) as f32);
            self.push(&k("fanPct"), g.fan_pct.unwrap_or(0.0));
            for e in &g.engines {
                self.push(&format!("gpu.{i}.eng.{}", e.name), e.busy);
            }
        }

        for d in &s.disks.disks {
            let k = |n: &str| format!("disk.{}.{n}", d.name);
            self.push(&k("r"), d.read_rate as f32);
            self.push(&k("w"), d.write_rate as f32);
            self.push(&k("riops"), d.read_iops as f32);
            self.push(&k("wiops"), d.write_iops as f32);
            self.push(&k("util"), d.util);
            self.push(&k("rlat"), d.read_lat);
            self.push(&k("wlat"), d.write_lat);
            self.push(&k("queue"), d.queue);
        }

        for chip in &s.sensors.chips {
            for r in &chip.readings {
                self.push(&format!("sensor.{}", r.id), r.value as f32);
            }
        }

        let e = &s.energy;
        self.push("energy.total", e.total_w as f32);
        if let Some(u) = &e.ups {
            self.push("ups.lineV", u.line_v.unwrap_or(0.0) as f32);
            self.push("ups.battery", u.battery_pct.unwrap_or(0.0) as f32);
            self.push("ups.load", u.load_pct.unwrap_or(0.0) as f32);
        }
        for src in &e.sources {
            self.push(&format!("energy.src.{}", src.id), src.watts.unwrap_or(0.0) as f32);
        }

        for i in &s.net.ifaces {
            let k = |n: &str| format!("net.{}.{n}", i.name);
            self.push(&k("rx"), i.rx_rate as f32);
            self.push(&k("tx"), i.tx_rate as f32);
            self.push(&k("rxp"), i.rx_pkt_rate as f32);
            self.push(&k("txp"), i.tx_pkt_rate as f32);
        }
        // los juegos (FPS y 1 % bajo) los añade `games::Games::ingest` antes de `end`
    }
}

/// Agrupa en cubos de `k` (media, o máximo con `max`).
fn bucket(s: &[f32], k: usize, max: bool) -> Vec<f32> {
    if k <= 1 {
        return s.to_vec();
    }
    s.chunks(k)
        .map(|c| if max { c.iter().copied().fold(f32::MIN, f32::max) } else { c.iter().sum::<f32>() / c.len() as f32 })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(h: &mut History, t: f64, vals: &[(&str, f32)]) {
        h.begin(t);
        for (k, v) in vals {
            h.push(k, *v);
        }
        h.end();
    }

    #[test]
    fn series_stay_aligned() {
        let mut h = History::new();
        sample(&mut h, 0.0, &[("a", 1.0)]);
        sample(&mut h, 1.0, &[("a", 2.0), ("b", 5.0)]);
        sample(&mut h, 2.0, &[("b", 6.0)]);
        assert_eq!(h.get("a").unwrap(), &[1.0, 2.0, 0.0]);
        assert_eq!(h.get("b").unwrap(), &[0.0, 5.0, 6.0]);
        assert_eq!(h.len(), 3);
        assert_eq!(h.keys_with_prefix(""), vec!["a", "b"]);
    }

    #[test]
    fn repeated_push_keeps_last() {
        let mut h = History::new();
        sample(&mut h, 0.0, &[("a", 1.0), ("a", 3.0)]);
        assert_eq!(h.get("a").unwrap(), &[3.0]);
    }

    #[test]
    fn window_and_buckets() {
        let mut h = History::new();
        for i in 0..100 {
            sample(&mut h, i as f64, &[("x", i as f32)]);
        }
        let w = h.window(Some(10.0));
        assert_eq!(w, Window { start: 89, stride: 1 });
        assert_eq!(h.view("x", w).len(), 11);
        assert_eq!(h.view("x", w)[0], 89.0);
        let all = h.window(None);
        assert_eq!(all.start, 0);
        // más puntos que MAX_DRAW: se agrupan
        for i in 100..4000 {
            sample(&mut h, i as f64, &[("x", 1.0)]);
        }
        let w = h.window(None);
        assert_eq!(w.stride, 4000usize.div_ceil(MAX_DRAW));
        assert!(h.view("x", w).len() <= MAX_DRAW);
        assert_eq!(h.view("x", w).len(), h.view_t(w).len());
        assert!(h.view_max("x", w).iter().all(|v| *v >= 1.0));
    }

    #[test]
    fn compaction_halves_old_samples() {
        let mut h = History::new();
        for i in 0..MAX_POINTS {
            sample(&mut h, i as f64, &[("x", (i % 2) as f32)]);
        }
        assert_eq!(h.compactions, 1);
        let n = h.len();
        assert_eq!(n, MAX_POINTS - MAX_POINTS / 4);
        // lo compactado es la media de pares (0 y 1 → 0.5); lo reciente queda intacto
        assert_eq!(h.get("x").unwrap()[0], 0.5);
        assert_eq!(h.get("x").unwrap()[n - 1], 1.0);
        assert_eq!(h.t()[0], 0.5);
        assert_eq!(*h.t().last().unwrap(), (MAX_POINTS - 1) as f64);
        assert!(h.t().windows(2).all(|w| w[0] < w[1]), "los instantes siguen ordenados");
    }

    #[test]
    fn csv_export() {
        let mut h = History::new();
        sample(&mut h, 1_700_000_000.0, &[("a", 1.0)]);
        sample(&mut h, 1_700_000_001.0, &[("a", 2.5), ("b", 3.0)]);
        let csv = h.to_csv(0);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "tiempo,a,b");
        assert_eq!(lines.len(), 3);
        assert!(lines[1].ends_with(",1,0"));
        assert!(lines[2].ends_with(",2.5,3"));
        assert_eq!(h.to_csv(1).lines().count(), 2);
    }

    #[test]
    fn clear_resets() {
        let mut h = History::new();
        sample(&mut h, 0.0, &[("a", 1.0)]);
        h.clear();
        assert!(h.is_empty());
        sample(&mut h, 1.0, &[("a", 2.0)]);
        assert_eq!(h.get("a").unwrap(), &[2.0]);
    }

    #[test]
    fn mem_segments_sum_to_total() {
        let m = crate::collectors::memory::MemSnapshot {
            total: 1000,
            free: 100,
            buffers: 50,
            cached: 300,
            s_reclaimable: 20,
            shmem: 40,
            ..Default::default()
        };
        let s = mem_segments(&m);
        assert_eq!(s.cache, 280);
        assert_eq!(s.used, 570);
        assert_eq!(s.apps, 530);
        assert_eq!(s.apps + s.shmem + s.buffers + s.cache + s.free, m.total);
    }
}
