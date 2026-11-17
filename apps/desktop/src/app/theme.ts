import { useEffect } from "react";
import type { ThemeMode } from "@/state/editor-store";

export function resolveTheme(theme: ThemeMode): "dark" | "light" {
  if (theme === "system") {
    if (typeof window !== "undefined" && window.matchMedia) {
      return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
    }
    return "dark";
  }
  return theme;
}

/** Applies the selected theme to the document root. */
export function useAppliedTheme(theme: ThemeMode): void {
  useEffect(() => {
    const root = document.documentElement;
    const apply = (): void => {
      const resolved = resolveTheme(theme);
      root.classList.toggle("dark", resolved === "dark");
      root.style.colorScheme = resolved;
    };
    apply();

    if (theme !== "system" || !window.matchMedia) return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
}

/** Applies the persisted UI scale through a CSS variable consumed by the app shell. */
export function useAppliedScale(scale: number): void {
  useEffect(() => {
    document.documentElement.style.setProperty("--app-scale", String(scale));
  }, [scale]);
}
