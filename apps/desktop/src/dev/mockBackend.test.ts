import { describe, expect, it } from "vitest";
import { mockInvoke } from "./mockBackend";

describe("mockInvoke fetch_codex_reasoning_levels", () => {
  it("returns all six levels for gpt-5.6 sol and terra", async () => {
    await expect(
      mockInvoke<string[]>("fetch_codex_reasoning_levels", { model: "gpt-5.6-sol" }),
    ).resolves.toEqual(["low", "medium", "high", "xhigh", "max", "ultra"]);
    await expect(
      mockInvoke<string[]>("fetch_codex_reasoning_levels", { model: "gpt-5.6-terra" }),
    ).resolves.toEqual(["low", "medium", "high", "xhigh", "max", "ultra"]);
  });

  it("returns levels through max for gpt-5.6 luna", async () => {
    await expect(
      mockInvoke<string[]>("fetch_codex_reasoning_levels", { model: "gpt-5.6-luna" }),
    ).resolves.toEqual(["low", "medium", "high", "xhigh", "max"]);
  });

  it("uses four levels for a known older model and none for a custom model", async () => {
    await expect(
      mockInvoke<string[]>("fetch_codex_reasoning_levels", { model: "gpt-5.5" }),
    ).resolves.toEqual(["low", "medium", "high", "xhigh"]);
    await expect(
      mockInvoke<string[]>("fetch_codex_reasoning_levels", { model: "vendor/gpt-5.6-sol" }),
    ).resolves.toEqual([]);
  });
});
