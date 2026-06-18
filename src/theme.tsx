import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from "react";

export type Theme = "system" | "light" | "dark" | "dracula" | "nord" | "solarized-light";

/** Theme options shown in the picker. `swatch` is a representative color dot. */
export const THEMES: { id: Theme; label: string; swatch: string }[] = [
  { id: "system", label: "System", swatch: "linear-gradient(135deg, #f5f7f9 0 50%, #1a1d23 50% 100%)" },
  { id: "light", label: "Light", swatch: "#ffffff" },
  { id: "dark", label: "Dark", swatch: "#22272e" },
  { id: "dracula", label: "Dracula", swatch: "#bd93f9" },
  { id: "nord", label: "Nord", swatch: "#88c0d0" },
  { id: "solarized-light", label: "Solarized Light", swatch: "#268bd2" },
];

const STORAGE_KEY = "agent2ssh.theme";
const VALID = new Set<Theme>(THEMES.map((x) => x.id));

type ThemeContextValue = {
  theme: Theme;
  setTheme: (theme: Theme) => void;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);

/** Apply the theme to <html> (System = remove the attribute so the OS decides). */
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  if (theme === "system") delete root.dataset.theme;
  else root.dataset.theme = theme;
}

function initialTheme(): Theme {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved && VALID.has(saved as Theme)) return saved as Theme;
  } catch {
    // localStorage may be unavailable
  }
  return "system";
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
