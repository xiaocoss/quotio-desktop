import { describe, expect, it } from "vitest";
import { applyTheme, resolveEffectiveTheme } from "./theme";

describe("resolveEffectiveTheme", () => {
  it("keeps the rose theme independent of system color scheme", () => {
    expect(resolveEffectiveTheme("rose", true)).toBe("rose");
    expect(resolveEffectiveTheme("rose", false)).toBe("rose");
  });

  it("resolves system to light or dark", () => {
    expect(resolveEffectiveTheme("system", true)).toBe("dark");
    expect(resolveEffectiveTheme("system", false)).toBe("light");
  });
});

describe("applyTheme", () => {
  it("uses a light color scheme for rose without changing the theme marker", () => {
    const attributes = new Map<string, string>();
    const root = {
      setAttribute(name: string, value: string) {
        attributes.set(name, value);
      },
      style: { colorScheme: "dark" },
    } as unknown as HTMLElement;

    applyTheme(root, "rose");

    expect(attributes.get("data-theme")).toBe("rose");
    expect(root.style.colorScheme).toBe("light");
  });
});
