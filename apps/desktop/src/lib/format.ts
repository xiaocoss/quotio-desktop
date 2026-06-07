// Small shared display helpers.

const HIDE_SENSITIVE_KEY = "quotio.hideSensitive";

// Whether sensitive values (emails, account names) should be masked in the UI.
// Controlled by the Settings > Privacy toggle, persisted in localStorage and
// defaulting to ON.
export function isHideSensitiveEnabled(): boolean {
  try {
    return localStorage.getItem(HIDE_SENSITIVE_KEY) !== "false";
  } catch {
    return true;
  }
}

export function setHideSensitiveEnabled(enabled: boolean): void {
  try {
    localStorage.setItem(HIDE_SENSITIVE_KEY, enabled ? "true" : "false");
  } catch {
    // ignore (e.g. storage unavailable)
  }
}

// Mask an email/identifier for the privacy-conscious UI (matches the mockups,
// e.g. "aurora@gmail.com" -> "a•••••@•••••.com"). Returns the value unchanged
// when the privacy toggle is off. Falls back gracefully for non-email values.
export function maskEmail(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return trimmed;
  if (!isHideSensitiveEnabled()) return trimmed;

  const at = trimmed.indexOf("@");
  if (at <= 0) {
    const head = trimmed.slice(0, 1);
    return `${head}${"•".repeat(Math.max(3, trimmed.length - 1))}`;
  }

  const local = trimmed.slice(0, at);
  const domain = trimmed.slice(at + 1);
  const dot = domain.lastIndexOf(".");
  const tld = dot >= 0 ? domain.slice(dot) : "";

  const maskedLocal = `${local.slice(0, 1)}${"•".repeat(5)}`;
  return `${maskedLocal}@${"•".repeat(5)}${tld}`;
}

// Tone for a "remaining quota" percentage, matching the mock's color coding.
export function quotaTone(remainingPercent: number): "good" | "warn" | "bad" {
  if (remainingPercent <= 10) return "bad";
  if (remainingPercent <= 50) return "warn";
  return "good";
}
