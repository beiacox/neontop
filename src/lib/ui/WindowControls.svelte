<script lang="ts">
  /**
   * Botones de la ventana (minimizar, maximizar/restaurar, cerrar). La ventana no lleva
   * decoración del sistema: en KDE Plasma con Wayland la barra de título que dibuja GTK para las
   * ventanas de WebKit salía sin botones. Arrastrar y el doble clic para maximizar los da el
   * atributo `data-tauri-drag-region` de la cabecera.
   */
  import { getCurrentWindow } from "@tauri-apps/api/window";

  const win = getCurrentWindow();
  let maximized = $state(false);

  $effect(() => {
    let off: (() => void) | undefined;
    const sync = () => win.isMaximized().then((m) => (maximized = m));
    sync();
    win.onResized(sync).then((u) => (off = u));
    return () => off?.();
  });
</script>

<div class="wc">
  <button title="Minimizar" aria-label="Minimizar" onclick={() => win.minimize()}>
    <svg viewBox="0 0 10 10"><path d="M1 5.5h8" /></svg>
  </button>
  <button title={maximized ? "Restaurar" : "Maximizar"} aria-label={maximized ? "Restaurar" : "Maximizar"} onclick={() => win.toggleMaximize()}>
    {#if maximized}
      <svg viewBox="0 0 10 10"><path d="M2.5 3.5h4v4h-4z M3.5 3.5V2h4.5v4.5H6.5" /></svg>
    {:else}
      <svg viewBox="0 0 10 10"><path d="M1.5 1.5h7v7h-7z" /></svg>
    {/if}
  </button>
  <button class="close" title="Cerrar" aria-label="Cerrar" onclick={() => win.close()}>
    <svg viewBox="0 0 10 10"><path d="M2 2l6 6M8 2l-6 6" /></svg>
  </button>
</div>

<style>
  .wc {
    display: flex;
    gap: 4px;
    margin-left: 4px;
  }
  button {
    width: 30px;
    height: 26px;
    display: grid;
    place-items: center;
    background: transparent;
    border: 1px solid color-mix(in srgb, var(--cyan) 20%, transparent);
    color: var(--muted);
    cursor: pointer;
    padding: 0;
  }
  button:hover {
    color: var(--cyan);
    background: color-mix(in srgb, var(--cyan) 12%, transparent);
  }
  button.close:hover {
    color: var(--bg);
    background: var(--red);
    border-color: var(--red);
  }
  svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.2;
  }
</style>
