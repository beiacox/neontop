/**
 * En la rejilla de dos columnas (≤ 1200 px), un panel estrecho que se queda solo en su fila
 * (número impar de paneles de una columna) deja media fila vacía. CSS no puede detectarlo, así
 * que aquí se marca con `data-fill` para que ocupe la fila entera (ver app.css).
 */
export function fillRows(root: HTMLElement): () => void {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const fix = () => {
    timer = null;
    const narrow = innerWidth <= 1200;
    for (const grid of root.querySelectorAll<HTMLElement>(":scope > .grid")) {
      // solo interesa que aparezcan o desaparezcan paneles, no lo que cambie dentro de ellos
      mo.observe(grid, { childList: true });
      const items = [...grid.children] as HTMLElement[];
      // se mide sin marcas para ver la colocación natural
      for (const el of items) el.removeAttribute("data-fill");
      if (!narrow) continue;
      const gw = grid.getBoundingClientRect().width;
      const rows = new Map<number, HTMLElement[]>();
      for (const el of items) {
        const r = el.getBoundingClientRect();
        if (!r.width) continue;
        const k = Math.round(r.top);
        rows.set(k, [...(rows.get(k) ?? []), el]);
      }
      for (const row of rows.values()) {
        if (row.length === 1 && row[0].getBoundingClientRect().width < gw * 0.7) row[0].setAttribute("data-fill", "");
      }
    }
  };
  // temporizador y no requestAnimationFrame: este no se ejecuta con la pestaña en segundo plano
  const schedule = () => {
    if (!timer) timer = setTimeout(fix, 30);
  };
  const ro = new ResizeObserver(schedule);
  ro.observe(root);
  // al cambiar de pestaña cambia la rejilla; al aparecer/desaparecer paneles, sus hijos
  const mo = new MutationObserver(schedule);
  mo.observe(root, { childList: true });
  schedule();
  return () => {
    ro.disconnect();
    mo.disconnect();
    if (timer) clearTimeout(timer);
  };
}
