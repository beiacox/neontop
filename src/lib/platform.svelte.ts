/**
 * Dónde se está ejecutando la interfaz:
 * - "tauri": la ventana de la app de escritorio.
 * - "web":   un navegador (teléfono, otro PC) conectado al servidor web de NeonTop. Las
 *            llamadas a `invoke` y los eventos se redirigen a la API HTTP con `mockIPC`, así
 *            el resto de la interfaz no cambia.
 * - "demo":  un navegador sin backend (`npm run dev`): datos simulados (demo.ts).
 */
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { startDemo } from "./demo";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const TOKEN_KEY = "neontop.webtoken";

class Platform {
  mode = $state<"tauri" | "web" | "demo">(isTauri ? "tauri" : "demo");
  /** Web: la clave guardada no es válida (o no hay): se muestra la pantalla de acceso. */
  needAuth = $state(false);
  /** Web: el equipo permite acciones (pausar, muestreo, señales). */
  webActions = $state(false);
  /** Web: se ha perdido la conexión con el equipo. */
  offline = $state(false);

  get desktop() {
    return this.mode === "tauri";
  }

  /** Guarda una clave introducida a mano y vuelve a conectar. */
  login(token: string) {
    localStorage.setItem(TOKEN_KEY, token.trim());
    location.reload();
  }
}

export const platform = new Platform();

const auth = (): Record<string, string> => {
  const t = localStorage.getItem(TOKEN_KEY);
  return t ? { Authorization: `Bearer ${t}` } : {};
};

/** Detecta el modo y prepara el transporte. Hay que esperarla antes de arrancar la interfaz. */
export async function initPlatform(): Promise<void> {
  if (isTauri) return;
  // la clave llega en el enlace o el QR (?t=…): se guarda y se quita de la barra de direcciones
  const q = new URLSearchParams(location.search);
  const t = q.get("t");
  if (t) {
    localStorage.setItem(TOKEN_KEY, t);
    q.delete("t");
    history.replaceState(null, "", location.pathname + (q.size ? `?${q}` : ""));
  }
  let ping: { neontop?: boolean; auth?: boolean; actions?: boolean } | null = null;
  try {
    const r = await fetch("/api/ping", { headers: auth() });
    ping = await r.json();
  } catch {
    /* sin API: es el servidor de desarrollo */
  }
  if (!ping?.neontop) {
    platform.mode = "demo";
    startDemo();
    return;
  }
  platform.mode = "web";
  platform.webActions = !!ping.actions;
  if (!ping.auth) {
    platform.needAuth = true;
    return;
  }
  startWeb();
}

function startWeb() {
  // id de esta pestaña: el equipo lleva la cuenta de qué recogidas pide cada cliente
  let client = sessionStorage.getItem("neontop.client");
  if (!client) sessionStorage.setItem("neontop.client", (client = Math.random().toString(36).slice(2, 12)));

  mockIPC(
    async (cmd, args) => {
      // plugins de escritorio (notificaciones, diálogos, autoarranque): no aplican en la web
      if (cmd.startsWith("plugin:")) return null;
      const r = await fetch(`/api/invoke/${cmd}?client=${client}`, {
        method: "POST",
        headers: { ...auth(), "Content-Type": "application/json" },
        body: JSON.stringify(args ?? {}),
      });
      const body = await r.json().catch(() => null);
      if (r.status === 401) platform.needAuth = true;
      if (!r.ok) throw body ?? r.statusText;
      return body;
    },
    { shouldMockEvents: true },
  );

  let seq = 0;
  let pseq = 0;
  let paused: boolean | null = null;
  let interval: number | null = null;
  const loop = async () => {
    for (;;) {
      try {
        const r = await fetch(`/api/poll?since=${seq}&psince=${pseq}&client=${client}`, { headers: auth() });
        if (r.status === 401) {
          platform.needAuth = true;
          return;
        }
        const d = await r.json();
        platform.offline = false;
        if (d.paused !== paused) emit("paused-changed", (paused = d.paused));
        if (d.intervalMs !== interval) emit("interval-changed", (interval = d.intervalMs));
        if (d.metrics) {
          seq = d.seq;
          emit("metrics", d.metrics);
        }
        if (d.procs) {
          pseq = d.pseq;
          emit("procs", d.procs);
        }
      } catch {
        platform.offline = true;
        await new Promise((res) => setTimeout(res, 2000));
      }
    }
  };
  startPolling = loop;
}

let startPolling: (() => void) | null = null;

/** La interfaz ya escucha los eventos: empieza a recibir datos del equipo. */
export function platformReady() {
  startPolling?.();
  startPolling = null;
}
