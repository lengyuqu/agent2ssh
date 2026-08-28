import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from "react";

export type Theme = "midnight-ops" | "system" | "light" | "dark" | "dracula" | "nord" | "solarized-light";

/** Theme options shown in the picker. `swatch` is a representative color dot. */
export const THEMES: { id: Theme; label: string; swatch: string }[] = [
  { id: "midnight-ops", label: "Midnight Ops", swatch: "#2DD4BF" },
  { id: "system", label: "System", swatch: "#2DD4BF" },
  { id: "light", label: "Light", swatch: "#17857c" },
  { id: "dark", label: "Dark", swatch: "#5ac8a6" },
  { id: "dracula", label: "Dracula", swatch: "#bd93f9" },
  { id: "nord", label: "Nord", swatch: "#88c0d0" },
  { id: "solarized-light", label: "Solarized Light", swatch: "#2a7fb8" },
];

const STORAGE_KEY = "agent2ssh.theme";
const VALID = new Set<Theme>(THEMES.map((x) => x.id));

type ThemeContextValue = {
  theme: Theme;
  setTheme: (theme: Theme) => void;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);

/** Apply the theme to <html>. Midnight Ops and System both remove the
 * attribute so they fall through to the :root ops-bridge skin (System no
 * longer follows the OS — it IS Midnight Ops, see plan §5 risk #1). */
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  if (theme === "system" || theme === "midnight-ops") delete root.dataset.theme;
  else root.dataset.theme = theme;
}

function initialTheme(): Theme {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved && VALID.has(saved as Theme)) return saved as Theme;
  } catch {
    // localStorage may be unavailable
  }
  return "midnight-ops";
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(() => initialTheme());

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, theme);
    } catch {
      // ignore persistence failures
    }
    applyTheme(theme);
  }, [theme]);

  const value = useMemo<ThemeContextValue>(() => ({ theme, setTheme }), [theme]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider");
  return ctx;
}
