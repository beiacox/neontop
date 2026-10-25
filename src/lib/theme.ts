/**
 * Temas. Un tema tiene dos partes:
 *  - `colors`: la paleta. Los nombres de los "slots" vienen del tema Neón original
 *    (cyan = primario, magenta = secundario…); cada tema los rellena con los suyos.
 *  - `style`: el carácter visual (tipografías, esquinas, fondo, brillo, líneas CRT…).
 * `C` se muta en sitio al cambiar de tema y la UI se remonta para recoger los nuevos valores.
 */
export interface Palette {
  bg: string;
  panel: string;
  /** Relleno neutro (p. ej. memoria libre). */
  dim: string;
  cyan: string;
  magenta: string;
  lime: string;
  amber: string;
  violet: string;
  red: string;
  blue: string;
  teal: string;
  orange: string;
  text: string;
  muted: string;
}

/** Forma de los paneles. */
export type Corners = "cut" | "round" | "square";
/** Fondo de la ventana. */
export type Backdrop = "grid" | "plain" | "aurora" | "dots" | "paper" | "lines";

export interface ThemeStyle {
  /** Fuente de títulos y cifras grandes. */
  display: string;
  /** Fuente general de la interfaz. */
  body: string;
  /** Fuente de ejes, tablas y valores numéricos. */
  mono: string;
  corners: Corners;
  /** Radio de los paneles (px) cuando `corners` es "round". */
  radius: number;
  backdrop: Backdrop;
  /** El tema usa brillo neón (el usuario puede apagarlo en Ajustes). */
  glow: boolean;
  /** El tema usa líneas de escaneo CRT. */
  scanlines: boolean;
  /** Títulos en mayúsculas espaciadas (estilo HUD) o normales. */
  titleCase: "upper" | "normal";
  /** Opacidad del fondo de los paneles. */
  panelAlpha: number;
  /** Intensidad del borde de los paneles (% de color de acento). */
  border: number;
}

export type ThemeCategory = "clasico" | "neon" | "terminal" | "editor" | "tematico";

export const CATEGORIES: { id: ThemeCategory; label: string }[] = [
  { id: "clasico", label: "Clásicos" },
  { id: "neon", label: "Neón" },
  { id: "terminal", label: "Terminal y retro" },
  { id: "editor", label: "De editores de código" },
  { id: "tematico", label: "Temáticos" },
];

export interface Theme {
  id: string;
  name: string;
  category: ThemeCategory;
  /** Tema claro (fondo luminoso). */
  light: boolean;
  colors: Palette;
  /** 6 paradas del mapa de calor, de 0% a 100%. */
  heat: [string, string, string, string, string, string];
  style: ThemeStyle;
}

// ------------------------------------------------------------------ tipografías

const F = {
  orbitron: 'Orbitron, "JetBrains Mono", sans-serif',
  jetbrains: '"JetBrains Mono", ui-monospace, monospace',
  inter: 'Inter, "Noto Sans", system-ui, sans-serif',
  grotesk: '"Space Grotesk", Inter, sans-serif',
  vt: 'VT323, "JetBrains Mono", monospace',
  lora: 'Lora, Georgia, serif',
  plex: '"IBM Plex Mono", "JetBrains Mono", monospace',
};

// ------------------------------------------------------------------ estilos base

const NEON: ThemeStyle = {
  display: F.orbitron,
  body: F.jetbrains,
  mono: F.jetbrains,
  corners: "cut",
  radius: 0,
  backdrop: "grid",
  glow: true,
  scanlines: true,
  titleCase: "upper",
  panelAlpha: 0.8,
  border: 28,
};

const CLEAN: ThemeStyle = {
  display: F.inter,
  body: F.inter,
  mono: F.jetbrains,
  corners: "round",
  radius: 10,
  backdrop: "plain",
  glow: false,
  scanlines: false,
  titleCase: "normal",
  panelAlpha: 1,
  border: 14,
};

const TERMINAL: ThemeStyle = {
  display: F.vt,
  body: F.jetbrains,
  mono: F.jetbrains,
  corners: "square",
  radius: 0,
  backdrop: "lines",
  glow: true,
  scanlines: true,
  titleCase: "upper",
  panelAlpha: 0.9,
  border: 40,
};

const SOFT: ThemeStyle = {
  display: F.grotesk,
  body: F.inter,
  mono: F.jetbrains,
  corners: "round",
  radius: 16,
  backdrop: "aurora",
  glow: false,
  scanlines: false,
  titleCase: "normal",
  panelAlpha: 0.72,
  border: 18,
};

// ------------------------------------------------------------------ temas

export const THEMES: Theme[] = [
  // ---- clásicos
  {
    id: "oscuro",
    name: "Oscuro",
    category: "clasico",
    light: false,
    colors: {
      bg: "#0d1117", panel: "#161b22", dim: "#21262d",
      cyan: "#58a6ff", magenta: "#db61a2", lime: "#3fb950", amber: "#d29922", violet: "#a371f7",
      red: "#f85149", blue: "#388bfd", teal: "#39c5cf", orange: "#f0883e", text: "#e6edf3", muted: "#7d8590",
    },
    heat: ["#0d1117", "#132a4a", "#1f6feb", "#a371f7", "#f0883e", "#ffdf5d"],
    style: CLEAN,
  },
  {
    id: "claro",
    name: "Claro",
    category: "clasico",
    light: true,
    colors: {
      bg: "#f3f5f8", panel: "#ffffff", dim: "#e3e7ed",
      cyan: "#0969da", magenta: "#bf3989", lime: "#1a7f37", amber: "#9a6700", violet: "#8250df",
      red: "#cf222e", blue: "#0550ae", teal: "#1b7c83", orange: "#bc4c00", text: "#1f2328", muted: "#656d76",
    },
    heat: ["#f3f5f8", "#cfe1fb", "#54aeff", "#8250df", "#cf222e", "#6e011a"],
    style: { ...CLEAN, border: 18 },
  },
  {
    id: "contraste",
    name: "Alto contraste",
    category: "clasico",
    light: false,
    colors: {
      bg: "#000000", panel: "#000000", dim: "#333333",
      cyan: "#00ffff", magenta: "#ff4dff", lime: "#00ff00", amber: "#ffff00", violet: "#c7a6ff",
      red: "#ff5050", blue: "#6fb5ff", teal: "#00ffc8", orange: "#ffa31a", text: "#ffffff", muted: "#d0d0d0",
    },
    heat: ["#000000", "#002a66", "#0077ff", "#00ffff", "#ffff00", "#ffffff"],
    style: { ...CLEAN, corners: "square", radius: 0, border: 100, backdrop: "plain" },
  },

  // ---- neón
  {
    id: "neon",
    name: "Neón",
    category: "neon",
    light: false,
    colors: {
      bg: "#07060d", panel: "#0e0b1e", dim: "#1c1a33",
      cyan: "#00f0ff", magenta: "#ff2bd6", lime: "#b6ff00", amber: "#ffb800", violet: "#9d4dff",
      red: "#ff3860", blue: "#3d7bff", teal: "#00ffa3", orange: "#ff7a00", text: "#d8e1ff", muted: "#6b6f9a",
    },
    heat: ["#0a0818", "#23145a", "#9d4dff", "#ff2bd6", "#00f0ff", "#e6ffff"],
    style: NEON,
  },
  {
    id: "synthwave",
    name: "Synthwave",
    category: "neon",
    light: false,
    colors: {
      bg: "#0d0221", panel: "#1a0636", dim: "#2a1348",
      cyan: "#2de2e6", magenta: "#ff3864", lime: "#f9c80e", amber: "#ff6c11", violet: "#b84dff",
      red: "#ff124f", blue: "#6a5cff", teal: "#0cf3c1", orange: "#ff8b3d", text: "#f6e6ff", muted: "#8a6fb0",
    },
    heat: ["#0d0221", "#261447", "#7a1fa2", "#ff3864", "#ff6c11", "#f9c80e"],
    style: NEON,
  },
  {
    id: "vaporwave",
    name: "Vaporwave",
    category: "neon",
    light: false,
    colors: {
      bg: "#150b27", panel: "#221338", dim: "#34224f",
      cyan: "#01cdfe", magenta: "#ff71ce", lime: "#05ffa1", amber: "#fffb96", violet: "#b967ff",
      red: "#ff6b8b", blue: "#7b9cff", teal: "#5ef1e8", orange: "#ffb86c", text: "#f5e9ff", muted: "#9b86c0",
    },
    heat: ["#150b27", "#3b1f6b", "#b967ff", "#ff71ce", "#01cdfe", "#fffb96"],
    style: { ...NEON, display: F.grotesk, scanlines: false, backdrop: "aurora" },
  },
  {
    id: "tron",
    name: "Tron",
    category: "neon",
    light: false,
    colors: {
      bg: "#02060c", panel: "#07111d", dim: "#10202f",
      cyan: "#6fe3ff", magenta: "#ff9e2c", lime: "#c4f4ff", amber: "#ffd166", violet: "#4d8dff",
      red: "#ff4d4d", blue: "#1f6fff", teal: "#36f1cd", orange: "#ff7b00", text: "#e0f4ff", muted: "#5a7890",
    },
    heat: ["#02060c", "#0a2440", "#1f6fff", "#6fe3ff", "#ff9e2c", "#ffffff"],
    style: { ...NEON, scanlines: false },
  },
  {
    id: "bladerunner",
    name: "Blade Runner",
    category: "neon",
    light: false,
    colors: {
      bg: "#0a0604", panel: "#170d07", dim: "#2a1a10",
      cyan: "#ffb000", magenta: "#ff5e3a", lime: "#00e5c7", amber: "#ffd27a", violet: "#c86bfa",
      red: "#ff2e2e", blue: "#3fa9f5", teal: "#00c2a8", orange: "#ff7b00", text: "#ffe8cc", muted: "#8a6d55",
    },
    heat: ["#0a0604", "#3a1208", "#a3240f", "#ff5e3a", "#ffb000", "#fff3d6"],
    style: NEON,
  },

  // ---- terminal y retro
  {
    id: "matrix",
    name: "Fósforo verde",
    category: "terminal",
    light: false,
    colors: {
      bg: "#010802", panel: "#041208", dim: "#0b2412",
      cyan: "#00ff41", magenta: "#b3ff00", lime: "#e2ff7a", amber: "#ffe066", violet: "#00b37a",
      red: "#ff3b3b", blue: "#00d9a3", teal: "#5cffb0", orange: "#d4ff00", text: "#b8ffc8", muted: "#3f7a4f",
    },
    heat: ["#010802", "#032b0f", "#00802a", "#00ff41", "#b3ff00", "#f0fff0"],
    style: TERMINAL,
  },
  {
    id: "ambar",
    name: "Terminal ámbar",
    category: "terminal",
    light: false,
    colors: {
      bg: "#0c0700", panel: "#140c00", dim: "#2b1a00",
      cyan: "#ffb000", magenta: "#ff7a00", lime: "#ffd27a", amber: "#ffe0a0", violet: "#ff9f40",
      red: "#ff4d1a", blue: "#e0a040", teal: "#ffc864", orange: "#ff8c00", text: "#ffc766", muted: "#8a6420",
    },
    heat: ["#0c0700", "#3a2200", "#8a5200", "#ffb000", "#ffd27a", "#fff4d6"],
    style: TERMINAL,
  },
  {
    id: "btop",
    name: "btop clásico",
    category: "terminal",
    light: false,
    colors: {
      bg: "#0b0b0b", panel: "#141414", dim: "#262626",
      cyan: "#4fd64f", magenta: "#e05a5a", lime: "#b4e04f", amber: "#e0c34f", violet: "#8d7cd6",
      red: "#ff3b3b", blue: "#5fa8e0", teal: "#4fd6b4", orange: "#e0934f", text: "#e0e0e0", muted: "#7a7a7a",
    },
    heat: ["#0b0b0b", "#1e3a1e", "#4fd64f", "#e0c34f", "#e05a5a", "#ffffff"],
    style: { ...TERMINAL, display: F.jetbrains, glow: false, scanlines: false, backdrop: "plain", corners: "round", radius: 6, border: 30 },
  },

  // ---- de editores de código
  {
    id: "dracula",
    name: "Dracula",
    category: "editor",
    light: false,
    colors: {
      bg: "#1e1f29", panel: "#282a36", dim: "#44475a",
      cyan: "#8be9fd", magenta: "#ff79c6", lime: "#50fa7b", amber: "#f1fa8c", violet: "#bd93f9",
      red: "#ff5555", blue: "#82aaff", teal: "#69ffc3", orange: "#ffb86c", text: "#f8f8f2", muted: "#7a7fa6",
    },
    heat: ["#1e1f29", "#44475a", "#6272a4", "#bd93f9", "#ff79c6", "#f8f8f2"],
    style: CLEAN,
  },
  {
    id: "nord",
    name: "Nord",
    category: "editor",
    light: false,
    colors: {
      bg: "#2e3440", panel: "#3b4252", dim: "#434c5e",
      cyan: "#88c0d0", magenta: "#b48ead", lime: "#a3be8c", amber: "#ebcb8b", violet: "#5e81ac",
      red: "#bf616a", blue: "#81a1c1", teal: "#8fbcbb", orange: "#d08770", text: "#eceff4", muted: "#8a96ad",
    },
    heat: ["#2e3440", "#3b4f6b", "#5e81ac", "#88c0d0", "#ebcb8b", "#eceff4"],
    style: { ...CLEAN, radius: 8 },
  },
  {
    id: "gruvbox",
    name: "Gruvbox",
    category: "editor",
    light: false,
    colors: {
      bg: "#1d2021", panel: "#282828", dim: "#3c3836",
      cyan: "#83a598", magenta: "#d3869b", lime: "#b8bb26", amber: "#fabd2f", violet: "#b16286",
      red: "#fb4934", blue: "#458588", teal: "#8ec07c", orange: "#fe8019", text: "#ebdbb2", muted: "#928374",
    },
    heat: ["#1d2021", "#3c3836", "#458588", "#b8bb26", "#fabd2f", "#fb4934"],
    style: { ...CLEAN, radius: 4, mono: F.plex },
  },
  {
    id: "mocha",
    name: "Catppuccin Mocha",
    category: "editor",
    light: false,
    colors: {
      bg: "#181825", panel: "#1e1e2e", dim: "#313244",
      cyan: "#89dceb", magenta: "#f5c2e7", lime: "#a6e3a1", amber: "#f9e2af", violet: "#cba6f7",
      red: "#f38ba8", blue: "#89b4fa", teal: "#94e2d5", orange: "#fab387", text: "#cdd6f4", muted: "#7f849c",
    },
    heat: ["#181825", "#313244", "#89b4fa", "#cba6f7", "#f38ba8", "#f9e2af"],
    style: { ...CLEAN, radius: 14 },
  },
  {
    id: "latte",
    name: "Catppuccin Latte",
    category: "editor",
    light: true,
    colors: {
      bg: "#e6e9ef", panel: "#eff1f5", dim: "#ccd0da",
      cyan: "#1e66f5", magenta: "#ea76cb", lime: "#40a02b", amber: "#df8e1d", violet: "#8839ef",
      red: "#d20f39", blue: "#209fb5", teal: "#179299", orange: "#fe640b", text: "#4c4f69", muted: "#8c8fa1",
    },
    heat: ["#e6e9ef", "#bcc0cc", "#1e66f5", "#8839ef", "#d20f39", "#4c4f69"],
    style: { ...CLEAN, radius: 14, border: 20 },
  },
  {
    id: "tokyo",
    name: "Tokyo Night",
    category: "editor",
    light: false,
    colors: {
      bg: "#16161e", panel: "#1a1b26", dim: "#292e42",
      cyan: "#7aa2f7", magenta: "#bb9af7", lime: "#9ece6a", amber: "#e0af68", violet: "#9d7cd8",
      red: "#f7768e", blue: "#7dcfff", teal: "#73daca", orange: "#ff9e64", text: "#c0caf5", muted: "#6b7394",
    },
    heat: ["#16161e", "#292e42", "#7aa2f7", "#bb9af7", "#f7768e", "#e0af68"],
    style: CLEAN,
  },
  {
    id: "solarized-oscuro",
    name: "Solarized oscuro",
    category: "editor",
    light: false,
    colors: {
      bg: "#002b36", panel: "#073642", dim: "#0e4552",
      cyan: "#2aa198", magenta: "#d33682", lime: "#859900", amber: "#b58900", violet: "#6c71c4",
      red: "#dc322f", blue: "#268bd2", teal: "#3fb8a9", orange: "#cb4b16", text: "#93a1a1", muted: "#657b83",
    },
    heat: ["#002b36", "#0e4552", "#268bd2", "#2aa198", "#b58900", "#fdf6e3"],
    style: { ...CLEAN, radius: 6, mono: F.plex },
  },
  {
    id: "solarized-claro",
    name: "Solarized claro",
    category: "editor",
    light: true,
    colors: {
      bg: "#eee8d5", panel: "#fdf6e3", dim: "#e4ddc8",
      cyan: "#268bd2", magenta: "#d33682", lime: "#859900", amber: "#b58900", violet: "#6c71c4",
      red: "#dc322f", blue: "#2aa198", teal: "#3fb8a9", orange: "#cb4b16", text: "#586e75", muted: "#93a1a1",
    },
    heat: ["#eee8d5", "#d9d2bc", "#268bd2", "#2aa198", "#b58900", "#dc322f"],
    style: { ...CLEAN, radius: 6, mono: F.plex, border: 22 },
  },

  // ---- temáticos
  {
    id: "papel",
    name: "Papel",
    category: "tematico",
    light: true,
    colors: {
      bg: "#f1e8d2", panel: "#fbf6e9", dim: "#e3d8bd",
      cyan: "#2f5d7c", magenta: "#9c3d54", lime: "#5b7a3a", amber: "#b0772b", violet: "#6b4f8a",
      red: "#a8322d", blue: "#3d6f9e", teal: "#3b7d74", orange: "#b8612a", text: "#3b3226", muted: "#8a7c66",
    },
    heat: ["#f1e8d2", "#e2d3ad", "#b0772b", "#9c3d54", "#5b2f3a", "#2b2218"],
    style: { ...CLEAN, display: F.lora, body: F.lora, mono: F.plex, corners: "square", radius: 2, backdrop: "paper", border: 30 },
  },
  {
    id: "bosque",
    name: "Bosque",
    category: "tematico",
    light: false,
    colors: {
      bg: "#0f1a14", panel: "#16241c", dim: "#22362a",
      cyan: "#7cc68d", magenta: "#d98e73", lime: "#b5d86b", amber: "#e3c16f", violet: "#9aa7d9",
      red: "#e06c5a", blue: "#6fa8c7", teal: "#5fb8a5", orange: "#d9975a", text: "#e4eadf", muted: "#7f9585",
    },
    heat: ["#0f1a14", "#1f3a2a", "#3f7a4f", "#7cc68d", "#e3c16f", "#f5f0d8"],
    style: SOFT,
  },
  {
    id: "oceano",
    name: "Océano",
    category: "tematico",
    light: false,
    colors: {
      bg: "#06141f", panel: "#0b2030", dim: "#123049",
      cyan: "#3ecfe0", magenta: "#7a8cff", lime: "#4fe0b0", amber: "#ffd479", violet: "#9d8cff",
      red: "#ff6b7d", blue: "#3d8bff", teal: "#2bb5a8", orange: "#ff9f6b", text: "#dcefff", muted: "#6f8ba3",
    },
    heat: ["#06141f", "#0e3050", "#1f6fb2", "#3ecfe0", "#b8f3ff", "#ffffff"],
    style: SOFT,
  },
  {
    id: "atardecer",
    name: "Atardecer",
    category: "tematico",
    light: false,
    colors: {
      bg: "#1b1024", panel: "#26162f", dim: "#3a2244",
      cyan: "#ff9a5c", magenta: "#ff5c8a", lime: "#ffd166", amber: "#ffb347", violet: "#b07cff",
      red: "#ff4757", blue: "#6c8dff", teal: "#5fd4c0", orange: "#ff7043", text: "#fbe4d8", muted: "#a08195",
    },
    heat: ["#1b1024", "#4a1f4f", "#b0306a", "#ff5c8a", "#ff9a5c", "#ffe29a"],
    style: { ...SOFT, radius: 12 },
  },
  {
    id: "sakura",
    name: "Sakura",
    category: "tematico",
    light: true,
    colors: {
      bg: "#fbeff3", panel: "#ffffff", dim: "#f3dbe3",
      cyan: "#d6557f", magenta: "#8e5bb5", lime: "#4f8f5f", amber: "#b87a1f", violet: "#7a6bd1",
      red: "#c4364f", blue: "#4f7fc9", teal: "#3f9a96", orange: "#d9784a", text: "#4a2f3a", muted: "#9c7f89",
    },
    heat: ["#fbeff3", "#f7cfdc", "#e88aa9", "#d6557f", "#8e5bb5", "#3d2350"],
    style: { ...SOFT, body: F.inter, display: F.inter, radius: 18, backdrop: "dots", panelAlpha: 0.9, border: 22 },
  },
];

/** Pseudo-tema que sigue el modo claro/oscuro del sistema. */
export const SYSTEM_THEME = "sistema";

export function resolveTheme(id: string, prefersDark: boolean): Theme {
  if (id === SYSTEM_THEME) id = prefersDark ? "oscuro" : "claro";
  return THEMES.find((t) => t.id === id) ?? THEMES.find((t) => t.id === "neon")!;
}

// ------------------------------------------------------------------ estado activo

export const C: Palette & { grid: string } = { ...THEMES[0].colors, grid: "" };

export function rgba(hex: string, a: number): string {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
}

function hexRgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** Color según nivel de carga: primario → lima → ámbar → secundario → rojo. */
export function heatColor(pct: number): string {
  if (pct < 30) return C.cyan;
  if (pct < 55) return C.lime;
  if (pct < 75) return C.amber;
  if (pct < 90) return C.magenta;
  return C.red;
}

export function tempColor(c: number): string {
  if (c < 50) return C.cyan;
  if (c < 65) return C.lime;
  if (c < 78) return C.amber;
  if (c < 88) return C.magenta;
  return C.red;
}

/** Tabla de 101 colores (0–100%) para mapas de calor; se regenera con el tema. */
export const HEAT_LUT: string[] = [];

function buildLut(heat: Theme["heat"]) {
  const pos = [0, 0.15, 0.4, 0.65, 0.85, 1];
  const stops = heat.map(hexRgb);
  for (let i = 0; i <= 100; i++) {
    const t = i / 100;
    let k = 0;
    while (k < stops.length - 2 && t > pos[k + 1]) k++;
    const f = (t - pos[k]) / (pos[k + 1] - pos[k]);
    const c = stops[k].map((v, j) => Math.round(v + (stops[k + 1][j] - v) * f));
    HEAT_LUT[i] = `rgb(${c[0]}, ${c[1]}, ${c[2]})`;
  }
}

// Fuentes activas (enlaces vivos: los módulos que las importan ven el valor actual).
export let MONO = F.jetbrains;
export let DISPLAY = F.orbitron;
export let BODY = F.jetbrains;

/** El tema activo usa brillo / líneas CRT (el usuario puede además apagarlos). */
export const themeFx = { glow: true, scanlines: true, light: false };

export function applyTheme(theme: Theme) {
  const { colors, style } = theme;
  // en temas claros la rejilla de las gráficas sale del color del texto; en oscuros, del primario
  Object.assign(C, colors, { grid: theme.light ? rgba(colors.text, 0.08) : rgba(colors.cyan, 0.07) });
  buildLut(theme.heat);
  MONO = style.mono;
  DISPLAY = style.display;
  BODY = style.body;
  Object.assign(themeFx, { glow: style.glow, scanlines: style.scanlines, light: theme.light });

  if (typeof document === "undefined") return;
  const html = document.documentElement;
  const root = html.style;
  for (const [k, v] of Object.entries(colors)) root.setProperty(`--${k}`, v);
  root.setProperty("--panel", rgba(colors.panel, style.panelAlpha));
  root.setProperty("--display", style.display);
  root.setProperty("--body", style.body);
  root.setProperty("--mono", style.mono);
  root.setProperty("--radius", `${style.corners === "round" ? style.radius : 0}px`);
  root.setProperty("--radius-sm", `${style.corners === "round" ? Math.min(8, Math.round(style.radius / 2)) : 0}px`);
  root.setProperty("--border-mix", `${style.border}%`);
  root.setProperty("--title-transform", style.titleCase === "upper" ? "uppercase" : "none");
  root.setProperty("--title-spacing", style.titleCase === "upper" ? "0.18em" : "0.01em");
  root.colorScheme = theme.light ? "light" : "dark";
  html.dataset.corners = style.corners;
  html.dataset.backdrop = style.backdrop;
  html.dataset.light = String(theme.light);
  html.dataset.hud = String(style.titleCase === "upper");
}

applyTheme(THEMES.find((t) => t.id === "neon")!);
