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

// Mask an email/identifier for the privacy-conscious UI, keeping the first 6
// characters visible (e.g. "aurora.b@gmail.com" -> "aurora•••@•••••.com").
// Returns the value unchanged when the privacy toggle is off. Falls back
// gracefully for non-email values.
const MASK_VISIBLE_PREFIX = 6;

export function maskEmail(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return trimmed;
  if (!isHideSensitiveEnabled()) return trimmed;

  const at = trimmed.indexOf("@");
  if (at <= 0) {
    // Non-email identifier: show the first 6 chars, mask the rest.
    return trimmed.length <= MASK_VISIBLE_PREFIX ? trimmed : `${trimmed.slice(0, MASK_VISIBLE_PREFIX)}${"•".repeat(3)}`;
  }

  const local = trimmed.slice(0, at);
  const domain = trimmed.slice(at + 1);
  const dot = domain.lastIndexOf(".");
  const tld = dot >= 0 ? domain.slice(dot) : "";

  const visible = local.slice(0, MASK_VISIBLE_PREFIX);
  const maskedLocal = local.length > MASK_VISIBLE_PREFIX ? `${visible}${"•".repeat(3)}` : visible;
  return `${maskedLocal}@${"•".repeat(5)}${tld}`;
}

// Tone for a "remaining quota" percentage, matching the mock's color coding.
export function quotaTone(remainingPercent: number): "good" | "warn" | "bad" {
  if (remainingPercent <= 10) return "bad";
  if (remainingPercent <= 50) return "warn";
  return "good";
}

// Extract the subscription plan from an AccountQuota status_message, which the
// Codex / Copilot fetchers encode as "plan: <tier> | until: <date>".
export function parsePlan(statusMessage: string | null | undefined): string | null {
  if (!statusMessage) return null;
  return statusMessage.match(/plan:\s*([^|]+)/i)?.[1]?.trim() || null;
}

export type PlanTier = "free" | "plus" | "pro" | "team" | "business";

// Map a plan name to a tier key used for badge coloring (shared by the Quota
// page and the menu-bar panel so colors stay consistent).
export function planTier(plan: string): PlanTier {
  const value = plan.toLowerCase();
  if (/pro/.test(value)) return "pro";
  if (/team/.test(value)) return "team";
  if (/business|enterprise|edu/.test(value)) return "business";
  if (/free/.test(value)) return "free";
  return "plus";
}
