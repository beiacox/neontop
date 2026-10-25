/**
 * Observa si un elemento está dentro de la zona visible del scroll. Las gráficas fuera de
 * pantalla no se redibujan; al volver a verse se ponen al día de una vez.
 */
export function watchVisible(el: Element, onChange: (visible: boolean) => void): () => void {
  const io = new IntersectionObserver((entries) => onChange(entries[entries.length - 1].isIntersecting), {
    // margen para que la gráfica ya esté al día al hacer scroll
    rootMargin: "200px 0px",
  });
  io.observe(el);
  return () => io.disconnect();
}
