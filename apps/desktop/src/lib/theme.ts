import type { ThemeMode } from "../types";

export type EffectiveTheme = Exclude<ThemeMode, "system">;

export function resolveEffectiveTheme(
  theme: ThemeMode,
  prefersDark: boolean,
): EffectiveTheme {
  return theme === "system" ? (prefersDark ? "dark" : "light") : theme;
}

export function applyTheme(root: HTMLElement, theme: EffectiveTheme): void {
  root.setAttribute("data-theme", theme);
  root.style.colorScheme = theme === "dark" ? "dark" : "light";
}
