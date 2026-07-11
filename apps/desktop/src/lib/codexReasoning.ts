export function normalizeCodexReasoningLevels(payload: unknown): string[] | null {
  if (!Array.isArray(payload)) return null;

  const levels: string[] = [];
  const seen = new Set<string>();
  for (const value of payload) {
    if (typeof value !== "string") return null;
    const level = value.trim();
    if (!level) return null;
    if (!seen.has(level)) {
      seen.add(level);
      levels.push(level);
    }
  }
  return levels;
}
