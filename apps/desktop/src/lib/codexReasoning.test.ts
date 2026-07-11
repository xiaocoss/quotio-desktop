import { describe, expect, it } from "vitest";
import { normalizeCodexReasoningLevels } from "./codexReasoning";

describe("normalizeCodexReasoningLevels", () => {
  it("accepts an empty catalog for an unknown model", () => {
    expect(normalizeCodexReasoningLevels([])).toEqual([]);
  });

  it("rejects non-array payloads", () => {
    expect(normalizeCodexReasoningLevels({ levels: ["low"] })).toBeNull();
  });

  it("trims levels and removes duplicates without changing order", () => {
    expect(
      normalizeCodexReasoningLevels([" low ", "medium", "low", " high ", "medium"]),
    ).toEqual(["low", "medium", "high"]);
  });

  it("rejects non-string and blank entries", () => {
    expect(normalizeCodexReasoningLevels(["low", 1])).toBeNull();
    expect(normalizeCodexReasoningLevels(["low", "   "])).toBeNull();
  });
});
