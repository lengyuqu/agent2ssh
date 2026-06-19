import type { ITheme } from "@xterm/xterm";
import type { Theme as AppTheme } from "./theme";

export type TerminalThemeId =
  | "app"
  | "github-dark"
  | "tokyo-night"
  | "dracula"
  | "nord"
  | "solarized-light"
  | "high-contrast"
  | "amber";

type TerminalPalette = {
  id: Exclude<TerminalThemeId, "app">;
  label: string;
  swatch: string;
  theme: ITheme;
};

export const TERMINAL_THEME_STORAGE_KEY = "agent2ssh.terminalTheme";

export const TERMINAL_THEME_OPTIONS: { id: TerminalThemeId; label: string; swatch: string }[] = [
  { id: "app", label: "Match app", swatch: "linear-gradient(135deg, #f5f7f9 0 50%, #1a1d23 50% 100%)" },
  { id: "github-dark", label: "GitHub Dark", swatch: "#58a6ff" },
  { id: "tokyo-night", label: "Tokyo Night", swatch: "#7aa2f7" },
  { id: "dracula", label: "Dracula", swatch: "#bd93f9" },
  { id: "nord", label: "Nord", swatch: "#88c0d0" },
  { id: "solarized-light", label: "Solarized Light", swatch: "#268bd2" },
  { id: "high-contrast", label: "High Contrast", swatch: "#ffd400" },
  { id: "amber", label: "Amber", swatch: "#ffb86c" },
];

const githubDark: ITheme = {
  background: "#0d1117",
  foreground: "#c9d1d9",
  cursor: "#58a6ff",
  cursorAccent: "#0d1117",
  selectionBackground: "#264f78",
  selectionForeground: "#ffffff",
  selectionInactiveBackground: "#1f3349",
  scrollbarSliderBackground: "#6e768166",
  scrollbarSliderHoverBackground: "#8b949e88",
  black: "#484f58",
  red: "#ff7b72",
  green: "#3fb950",
  yellow: "#d29922",
  blue: "#58a6ff",
  magenta: "#bc8cff",
  cyan: "#39c5cf",
  white: "#b1bac4",
  brightBlack: "#6e7681",
  brightRed: "#ffa198",
  brightGreen: "#56d364",
  brightYellow: "#e3b341",
  brightBlue: "#79c0ff",
  brightMagenta: "#d2a8ff",
  brightCyan: "#56d4dd",
  brightWhite: "#f0f6fc",
};

const tokyoNight: ITheme = {
  background: "#1a1b26",
  foreground: "#c0caf5",
  cursor: "#c0caf5",
  cursorAccent: "#1a1b26",
  selectionBackground: "#33467c",
  selectionForeground: "#ffffff",
  selectionInactiveBackground: "#283457",
  scrollbarSliderBackground: "#565f8966",
  scrollbarSliderHoverBackground: "#737aa288",
  black: "#15161e",
  red: "#f7768e",
  green: "#9ece6a",
  yellow: "#e0af68",
  blue: "#7aa2f7",
  magenta: "#bb9af7",
  cyan: "#7dcfff",
  white: "#a9b1d6",
  brightBlack: "#414868",
  brightRed: "#ff899d",
  brightGreen: "#9fe044",
  brightYellow: "#faba4a",
  brightBlue: "#8db0ff",
  brightMagenta: "#c7a9ff",
  brightCyan: "#a4daff",
  brightWhite: "#c0caf5",
};

const dracula: ITheme = {
  background: "#282a36",
  foreground: "#f8f8f2",
  cursor: "#f8f8f2",
  cursorAccent: "#282a36",
  selectionBackground: "#44475a",
  selectionForeground: "#ffffff",
  selectionInactiveBackground: "#3a3d4d",
  scrollbarSliderBackground: "#6272a466",
  scrollbarSliderHoverBackground: "#6272a488",
  black: "#21222c",
  red: "#ff5555",
  green: "#50fa7b",
  yellow: "#f1fa8c",
  blue: "#8be9fd",
  magenta: "#ff79c6",
  cyan: "#8be9fd",
  white: "#f8f8f2",
  brightBlack: "#6272a4",
  brightRed: "#ff6e6e",
  brightGreen: "#69ff94",
  brightYellow: "#ffffa5",
  brightBlue: "#d6acff",
  brightMagenta: "#ff92df",
  brightCyan: "#a4ffff",
  brightWhite: "#ffffff",
};

const nord: ITheme = {
  background: "#2e3440",
  foreground: "#d8dee9",
  cursor: "#88c0d0",
  cursorAccent: "#2e3440",
  selectionBackground: "#4c566a",
  selectionForeground: "#eceff4",
  selectionInactiveBackground: "#434c5e",
  scrollbarSliderBackground: "#66758f66",
  scrollbarSliderHoverBackground: "#8190aa88",
  black: "#3b4252",
  red: "#bf616a",
  green: "#a3be8c",
  yellow: "#ebcb8b",
  blue: "#81a1c1",
  magenta: "#b48ead",
  cyan: "#88c0d0",
  white: "#e5e9f0",
  brightBlack: "#4c566a",
  brightRed: "#d06f79",
  brightGreen: "#b1d196",
  brightYellow: "#f0d399",
  brightBlue: "#8fbcbb",
  brightMagenta: "#c895bf",
  brightCyan: "#8fbcbb",
  brightWhite: "#eceff4",
};

const solarizedLight: ITheme = {
  background: "#fdf6e3",
  foreground: "#586e75",
  cursor: "#268bd2",
  cursorAccent: "#fdf6e3",
  selectionBackground: "#d7e8ed",
  selectionForeground: "#073642",
  selectionInactiveBackground: "#eee8d5",
  scrollbarSliderBackground: "#93a1a166",
  scrollbarSliderHoverBackground: "#83949688",
  black: "#073642",
  red: "#dc322f",
  green: "#859900",
  yellow: "#b58900",
  blue: "#268bd2",
  magenta: "#d33682",
  cyan: "#2aa198",
  white: "#eee8d5",
  brightBlack: "#002b36",
  brightRed: "#cb4b16",
  brightGreen: "#586e75",
  brightYellow: "#657b83",
  brightBlue: "#839496",
  brightMagenta: "#6c71c4",
  brightCyan: "#93a1a1",
  brightWhite: "#fdf6e3",
};

const highContrast: ITheme = {
  background: "#000000",
  foreground: "#f5f5f5",
  cursor: "#ffd400",
  cursorAccent: "#000000",
  selectionBackground: "#005fcc",
  selectionForeground: "#ffffff",
  selectionInactiveBackground: "#333333",
  scrollbarSliderBackground: "#ffffff55",
  scrollbarSliderHoverBackground: "#ffffff88",
  black: "#000000",
  red: "#ff5c57",
  green: "#5af78e",
  yellow: "#f3f99d",
  blue: "#57c7ff",
  magenta: "#ff6ac1",
  cyan: "#9aedfe",
  white: "#f1f1f0",
  brightBlack: "#686868",
  brightRed: "#ff5c57",
  brightGreen: "#5af78e",
  brightYellow: "#f3f99d",
  brightBlue: "#57c7ff",
  brightMagenta: "#ff6ac1",
  brightCyan: "#9aedfe",
  brightWhite: "#ffffff",
};

const amber: ITheme = {
  background: "#15110b",
  foreground: "#ffd7a3",
  cursor: "#ffb86c",
  cursorAccent: "#15110b",
  selectionBackground: "#5f3d1f",
  selectionForeground: "#fff4df",
  selectionInactiveBackground: "#352414",
  scrollbarSliderBackground: "#ffb86c55",
  scrollbarSliderHoverBackground: "#ffcc8888",
  black: "#22180f",
  red: "#ff6b5f",
  green: "#8bd66b",
  yellow: "#f6c177",
  blue: "#7fb4ca",
  magenta: "#d98fd9",
  cyan: "#72d6c9",
  white: "#ffd7a3",
  brightBlack: "#7a5b3a",
  brightRed: "#ff8b7f",
  brightGreen: "#a6e989",
  brightYellow: "#ffd08a",
  brightBlue: "#9ccfd8",
  brightMagenta: "#e8a2e8",
  brightCyan: "#92eadf",
  brightWhite: "#fff4df",
};

const PALETTES: Record<Exclude<TerminalThemeId, "app">, TerminalPalette> = {
  "github-dark": { id: "github-dark", label: "GitHub Dark", swatch: "#58a6ff", theme: githubDark },
  "tokyo-night": { id: "tokyo-night", label: "Tokyo Night", swatch: "#7aa2f7", theme: tokyoNight },
  dracula: { id: "dracula", label: "Dracula", swatch: "#bd93f9", theme: dracula },
  nord: { id: "nord", label: "Nord", swatch: "#88c0d0", theme: nord },
  "solarized-light": {
    id: "solarized-light",
    label: "Solarized Light",
    swatch: "#268bd2",
    theme: solarizedLight,
  },
  "high-contrast": {
    id: "high-contrast",
    label: "High Contrast",
    swatch: "#ffd400",
    theme: highContrast,
  },
  amber: { id: "amber", label: "Amber", swatch: "#ffb86c", theme: amber },
};

export function isTerminalThemeId(value: string): value is TerminalThemeId {
  return TERMINAL_THEME_OPTIONS.some((option) => option.id === value);
}

export function resolveTerminalTheme(id: TerminalThemeId, appTheme: AppTheme): ITheme {
  if (id !== "app") return PALETTES[id].theme;
  if (appTheme === "dracula") return dracula;
  if (appTheme === "nord") return nord;
  if (appTheme === "solarized-light" || appTheme === "light") return solarizedLight;
  return githubDark;
}

export function terminalThemeBackground(id: TerminalThemeId, appTheme: AppTheme): string {
  return resolveTerminalTheme(id, appTheme).background ?? githubDark.background ?? "#0d1117";
}
