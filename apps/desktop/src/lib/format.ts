// Small shared display helpers.
import type { AccountQuota, AuthFile } from "../types";

const HIDE_SENSITIVE_KEY = "quotio.hideSensitive";

// Whether sensitive values (emails, account names) should be masked in the UI.
// Controlled by the Settings > Privacy toggle, persisted in localStorage and
// defaulting to ON.
//
// maskEmail() is called once per row (80+ accounts) on every poll tick, so the
// localStorage read is cached: synchronous localStorage access in render adds up.
// The cache is invalidated on local toggle (setHideSensitiveEnabled) and on the
// cross-window `storage` event.
let hideSensitiveCache: boolean | null = null;

function readHideSensitive(): boolean {
  try {
    return localStorage.getItem(HIDE_SENSITIVE_KEY) !== "false";
  } catch {
    return true;
  }
}

if (typeof window !== "undefined") {
  window.addEventListener("storage", (event) => {
    if (event.key === HIDE_SENSITIVE_KEY) hideSensitiveCache = null;
  });
}

export function isHideSensitiveEnabled(): boolean {
  if (hideSensitiveCache === null) hideSensitiveCache = readHideSensitive();
  return hideSensitiveCache;
}

export function setHideSensitiveEnabled(enabled: boolean): void {
  hideSensitiveCache = enabled;
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

// Force en-US compact units (K/M/B/T) so token/request counts read as "473.2M"
// rather than the locale's 万/亿 grouping. Shared by the dashboard KPI cards and
// the account summary table.
export function formatCompactNumber(value: number): string {
  return new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 }).format(
    Number.isFinite(value) ? value : 0,
  );
}

// Format an estimated cost in USD, or "--" when unavailable (no prices set).
export function formatCost(value: number | null | undefined): string {
  if (value === null || value === undefined) return "--";
  return `$${value.toFixed(value !== 0 && Math.abs(value) < 1 ? 4 : 2)}`;
}

// Short relative time ("3分钟前" / "刚刚") from a unix-ms timestamp; falls back
// to "--" for missing/zero values.
export function formatRelativeTime(ms: number): string {
  if (!ms || ms <= 0) return "--";
  const diff = Date.now() - ms;
  if (diff < 60_000) return "刚刚";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}小时前`;
  const days = Math.floor(diff / 86_400_000);
  if (days < 30) return `${days}天前`;
  return new Date(ms).toLocaleDateString();
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

// Codex encodes its "主动重置次数" (manual rate-limit reset credits) into the
// same status_message as "... | resets: <N>". Returns the count, or null when
// the account isn't Codex / the field wasn't present.
export function parseResetCredits(statusMessage: string | null | undefined): number | null {
  if (!statusMessage) return null;
  const match = statusMessage.match(/resets:\s*(-?\d+)/i);
  if (!match) return null;
  const value = Number.parseInt(match[1], 10);
  return Number.isFinite(value) ? value : null;
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

// Match a quota account to a proxy auth-file (same provider, then email, then
// exact filename stem). Provider-scoping avoids cross-provider email collisions
// (e.g. the same email on Codex + Trae). Shared by gating and the health view.
export function matchAuthFile(quota: AccountQuota, authFiles: AuthFile[]): AuthFile | null {
  const provider = quota.provider_id.trim().toLowerCase();
  const candidates = authFiles.filter((file) => {
    const fp = (file.provider ?? "").trim().toLowerCase();
    return fp === provider || fp.includes(provider) || provider.includes(fp);
  });
  if (candidates.length === 0) return null;

  const email = quota.account_label?.trim().toLowerCase();
  if (email && email.includes("@")) {
    const byEmail = candidates.find((file) => (file.email ?? "").trim().toLowerCase() === email);
    if (byEmail) return byEmail;
  }
  const key = quota.account_key?.trim().toLowerCase();
  if (key) {
    const prefixed = `${provider}-${key}`;
    const byKey = candidates.find((file) => {
      const stem = file.name.toLowerCase().replace(/\.json$/, "");
      return stem === key || stem === prefixed;
    });
    if (byKey) return byKey;
  }
  return null;
}

// 当前真正在「服务」的号 ≈ 代理 fill-first 实际会先用的号:优先级最高、且当前健康可用的那个。
// `orderFileNames` 必须已按优先级排序(后端 order 即是),这里按序找**第一个「启用 + 近期成功 >
// 失败」**的号。比「累计成功最多」更跟手——刚恢复 / 刚切号时不会被旧号的历史成功数带偏(否则旧号
// 累计 √ 仍高、会把「主用」误标到它身上);也跳过待命 / 隔离 / 用户禁用的号(不在代理池里)。全都
// 不健康 / 无流量时返回 null,让调用方回退到后端给的 active。
export function servingFile(orderFileNames: string[], authFiles: AuthFile[]): string | null {
  for (const name of orderFileNames) {
    const file = authFiles.find((f) => f.name === name);
    if (!file || file.disabled) continue;
    const success = file.success ?? 0;
    const failed = file.failed ?? 0;
    if (success > 0 && success > failed) return name;
  }
  return null;
}

// Tone for one recent-request health bucket: green (all ok), amber (mixed),
// red (all failed), gray (idle / no traffic).
export function healthTone(bucket: { success: number; failed: number }): "good" | "warn" | "bad" | "idle" {
  if (bucket.failed > 0) return bucket.success > 0 ? "warn" : "bad";
  return bucket.success > 0 ? "good" : "idle";
}
