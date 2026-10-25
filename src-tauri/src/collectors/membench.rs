//! Test de ancho de banda y latencia de la RAM, bajo demanda.
//!
//! Linux no expone el tráfico de memoria en vivo sin privilegios (haría falta leer los
//! contadores PMU del controlador de memoria), así que se mide el máximo alcanzable.

use serde::Serialize;
use std::hint::black_box;
use std::thread;
use std::time::Instant;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MemBench {
    /// Bytes/s con todos los hilos.
    pub read: f64,
    pub write: f64,
    pub copy: f64,
    /// Lectura con un solo hilo.
    pub read_single: f64,
    /// Nanosegundos por acceso aleatorio (persecución de punteros).
    pub latency_ns: f64,
    pub threads: usize,
    pub buffer_bytes: u64,
    pub duration_ms: f64,
    /// ms desde epoch
    pub ts: u64,
}

/// Tamaño del búfer: muy por encima de cualquier caché L3 de escritorio.
const BUF_BYTES: usize = 512 << 20;
const REPEATS: usize = 3;

fn split<T>(buf: &mut [T], parts: usize) -> Vec<&mut [T]> {
    let chunk = buf.len().div_ceil(parts);
    buf.chunks_mut(chunk).collect()
}

fn sum(chunk: &[u64]) -> u64 {
    // cuatro acumuladores para que el compilador vectorice y no dependa de un solo registro
    let mut acc = [0u64; 4];
    let mut it = chunk.chunks_exact(4);
    for c in &mut it {
        acc[0] = acc[0].wrapping_add(c[0]);
        acc[1] = acc[1].wrapping_add(c[1]);
        acc[2] = acc[2].wrapping_add(c[2]);
        acc[3] = acc[3].wrapping_add(c[3]);
    }
    it.remainder().iter().fold(acc.iter().fold(0, |a, b| a.wrapping_add(*b)), |a, b| a.wrapping_add(*b))
}

/// Mejor de REPEATS ejecuciones, en bytes/s.
fn best(bytes: usize, mut f: impl FnMut()) -> f64 {
    (0..REPEATS)
        .map(|_| {
            let t = Instant::now();
            f();
            bytes as f64 / t.elapsed().as_secs_f64()
        })
        .fold(0.0, f64::max)
}

fn latency_ns() -> f64 {
    // ciclo aleatorio único (Sattolo) sobre 128 MiB de índices: cada salto es un fallo de caché
    const N: usize = 32 << 20;
    const STEPS: usize = 4_000_000;
    let mut next: Vec<u32> = (0..N as u32).collect();
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for i in (1..N).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let j = (x % i as u64) as usize;
        next.swap(i, j);
    }
    let mut p = 0usize;
    let t = Instant::now();
    for _ in 0..STEPS {
        p = next[p] as usize;
    }
    black_box(p);
    t.elapsed().as_nanos() as f64 / STEPS as f64
}

pub fn run() -> MemBench {
    let started = Instant::now();
    let threads = thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let words = BUF_BYTES / 8;
    let mut src = vec![0u64; words];
    let mut dst = vec![0u64; words];

    let par_fill = |buf: &mut [u64], v: u64| {
        thread::scope(|s| {
            for c in split(buf, threads) {
                s.spawn(move || c.fill(v));
            }
        })
    };
    // primer contacto: fuerza a que el kernel asigne las páginas antes de medir
    par_fill(&mut src, 1);
    par_fill(&mut dst, 2);

    let write = best(BUF_BYTES, || par_fill(&mut src, 3));

    let read = best(BUF_BYTES, || {
        thread::scope(|s| {
            for c in src.chunks(words.div_ceil(threads)) {
                s.spawn(move || black_box(sum(c)));
            }
        })
    });

    let copy = best(BUF_BYTES, || {
        thread::scope(|s| {
            for (d, sc) in split(&mut dst, threads).into_iter().zip(src.chunks(words.div_ceil(threads))) {
                s.spawn(move || d.copy_from_slice(sc));
            }
        })
    });

    let read_single = best(BUF_BYTES, || {
        black_box(sum(&src));
    });

    drop(src);
    drop(dst);
    let latency_ns = latency_ns();

    MemBench {
        read,
        write,
        copy,
        read_single,
        latency_ns,
        threads,
        buffer_bytes: BUF_BYTES as u64,
        duration_ms: started.elapsed().as_secs_f64() * 1000.0,
        ts: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "tarda unos segundos; ejecutar con --ignored"]
    fn bench() {
        let r = super::run();
        eprintln!(
            "lectura {:.1} GB/s · escritura {:.1} · copia {:.1} · 1 hilo {:.1} · latencia {:.1} ns · {} hilos · {:.0} ms",
            r.read / 1e9,
            r.write / 1e9,
            r.copy / 1e9,
            r.read_single / 1e9,
            r.latency_ns,
            r.threads,
            r.duration_ms
        );
    }
}
