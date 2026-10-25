/**
 * Serie numérica compacta para el historial. Un `number[]` guarda cada valor como double
 * (8 bytes) más la sobrecarga del array; aquí se usa un Float32Array (4 bytes) que crece
 * por bloques. Los timestamps usan Float64Array: en 32 bits un epoch en segundos perdería
 * precisión a partir de ~2 minutos.
 *
 * Expone la parte de la API de Array que usan los componentes (length, slice, forEach) para
 * que puedan recibir indistintamente una Series o un array normal.
 */
export class Series {
  private buf: Float32Array | Float64Array;
  length = 0;

  constructor(
    len = 0,
    private readonly f64 = false,
  ) {
    this.buf = this.alloc(Math.max(256, len * 2));
    this.length = len; // los buffers nuevos ya vienen a cero
  }

  private alloc(n: number) {
    return this.f64 ? new Float64Array(n) : new Float32Array(n);
  }

  push(v: number) {
    if (this.length === this.buf.length) {
      const next = this.alloc(this.buf.length * 2);
      next.set(this.buf);
      this.buf = next;
    }
    this.buf[this.length++] = v;
  }

  at(i: number): number {
    return this.buf[i < 0 ? this.length + i : i];
  }

  /** Copia como array normal (acepta índices negativos, como Array.prototype.slice). */
  slice(start = 0, end = this.length): number[] {
    const s = start < 0 ? Math.max(0, this.length + start) : Math.min(start, this.length);
    const e = end < 0 ? Math.max(0, this.length + end) : Math.min(end, this.length);
    return Array.from(this.buf.subarray(s, Math.max(s, e)));
  }

  /** Vista sin copia de un tramo (para bucles internos). */
  view(start = 0, end = this.length): Float32Array | Float64Array {
    return this.buf.subarray(start, end);
  }

  forEach(fn: (v: number, i: number) => void) {
    for (let i = 0; i < this.length; i++) fn(this.buf[i], i);
  }

  clear() {
    this.length = 0;
  }

  /** Promedia por pares la primera mitad (`half` muestras) en sitio. */
  compact(half: number) {
    const merged = half >> 1;
    const b = this.buf;
    for (let i = 0; i < merged; i++) b[i] = (b[2 * i] + b[2 * i + 1]) / 2;
    b.copyWithin(merged, merged * 2, this.length);
    this.length -= merged;
    // si sobra mucha capacidad tras compactar, se devuelve memoria
    if (this.buf.length > this.length * 4 && this.buf.length > 1024) {
      const next = this.alloc(this.length * 2);
      next.set(b.subarray(0, this.length));
      this.buf = next;
    }
  }

  /** Bytes reservados (para mostrar el consumo del historial). */
  get bytes(): number {
    return this.buf.byteLength;
  }
}

/** Lo que aceptan los componentes de gráficas: una Series o un array normal. */
export type SeriesLike = Series | number[];
