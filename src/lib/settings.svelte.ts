import { invoke } from "@tauri-apps/api/core";
import { platform } from "./platform.svelte";
import { SYSTEM_THEME, THEMES, applyTheme, resolveTheme, themeFx } from "./theme";

const KEY = "neontop.settings";

/** Ajustes de usuario, persistidos en localStorage. `windowSec = Infinity` es la ventana ilimitada. */
class Settings {
  theme = $state("neon");
  intervalMs = $state(1000);
  windowSec = $state(120);
  paused = $state(false);
  scanlines = $state(true);
  glow = $state(true);
  /** Última pestaña abierta. */
  tab = $state("overview");
  /** Cerrar la ventana la deja en la bandeja del sistema. */
  closeToTray = $state(false);
  /** Repetir como root (con contraseña, vía polkit) las acciones que lo necesiten. */
  adminActions = $state(false);
  /**
   * Id del tema realmente aplicado: coincide con `theme` salvo con "sistema", que se
   * resuelve a Claro u Oscuro según el escritorio. La UI se remonta cuando cambia.
   */
  resolvedTheme = $state("neon");
  /** Efectos que el tema activo utiliza (los ajustes del usuario pueden apagarlos). */
  private fx = $state({ glow: true, scanlines: true });

  /** Brillo neón efectivo: el tema lo usa y el usuario no lo ha apagado. */
  get glowOn() {
    return this.glow && this.fx.glow;
  }
  get scanlinesOn() {
    return this.scanlines && this.fx.scanlines;
  }

  private dark = typeof matchMedia !== "undefined" ? matchMedia("(prefers-color-scheme: dark)") : null;

  /** Aplica el tema elegido (resolviendo "sistema"). */
  private apply() {
    const t = resolveTheme(this.theme, this.dark?.matches ?? true);
    applyTheme(t);
    this.fx = { glow: themeFx.glow, scanlines: themeFx.scanlines };
    this.resolvedTheme = t.id;
  }

  load() {
    try {
      const s = JSON.parse(localStorage.getItem(KEY) ?? "{}");
      if (typeof s.theme === "string") this.theme = s.theme;
      if (typeof s.intervalMs === "number") this.intervalMs = s.intervalMs;
      // JSON no admite Infinity: 0 significa ilimitada
      if (typeof s.windowSec === "number") this.windowSec = s.windowSec === 0 ? Infinity : s.windowSec;
      if (typeof s.scanlines === "boolean") this.scanlines = s.scanlines;
      if (typeof s.glow === "boolean") this.glow = s.glow;
      if (typeof s.tab === "string") this.tab = s.tab;
      if (typeof s.closeToTray === "boolean") this.closeToTray = s.closeToTray;
      if (typeof s.adminActions === "boolean") this.adminActions = s.adminActions;
    } catch {
      /* ajustes corruptos: se usan los valores por defecto */
    }
    this.apply();
    // con el tema "sistema", seguir los cambios de modo claro/oscuro del escritorio
    this.dark?.addEventListener("change", () => this.theme === SYSTEM_THEME && this.apply());
    // en la web el muestreo lo decide el equipo (llega con cada consulta)
    if (platform.desktop) {
      invoke("set_interval", { ms: this.intervalMs });
      invoke("set_close_to_tray", { enabled: this.closeToTray });
    }
  }

  save() {
    localStorage.setItem(
      KEY,
      JSON.stringify({
        theme: this.theme,
        intervalMs: this.intervalMs,
        windowSec: Number.isFinite(this.windowSec) ? this.windowSec : 0,
        scanlines: this.scanlines,
        glow: this.glow,
        tab: this.tab,
        closeToTray: this.closeToTray,
        adminActions: this.adminActions,
      }),
    );
  }

  setTheme(id: string) {
    if (id !== SYSTEM_THEME && !THEMES.some((x) => x.id === id)) return;
    this.theme = id;
    this.apply();
    this.save();
  }

  setInterval(ms: number) {
    this.intervalMs = Math.round(Math.min(60_000, Math.max(100, ms)));
    // desde la web puede estar prohibido: la siguiente consulta restablece el valor real
    invoke("set_interval", { ms: this.intervalMs }).catch(() => {});
    this.save();
  }

  setWindow(sec: number) {
    this.windowSec = sec;
    this.save();
  }

  togglePause() {
    this.paused = !this.paused;
    invoke("set_paused", { paused: this.paused }).catch(() => {});
  }

  setAdminActions(on: boolean) {
    this.adminActions = on;
    this.save();
  }

  setCloseToTray(on: boolean) {
    this.closeToTray = on;
    invoke("set_close_to_tray", { enabled: on });
    this.save();
  }

  setTab(tab: string) {
    this.tab = tab;
    this.save();
  }

  setEffect(key: "scanlines" | "glow", on: boolean) {
    this[key] = on;
    this.save();
  }
}

export const settings = new Settings();

export function fmtInterval(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const s = ms / 1000;
  return s < 60 ? `${Number.isInteger(s) ? s : s.toFixed(1)}s` : `${(s / 60).toFixed(1)}min`;
}

export function fmtWindow(sec: number): string {
  if (!Number.isFinite(sec)) return "∞";
  if (sec < 3600) return `${sec / 60}m`;
  return `${sec / 3600}h`;
}
