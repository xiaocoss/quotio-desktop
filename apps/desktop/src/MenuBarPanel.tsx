import { useEffect, useMemo, useState } from "react";
import "./App.css";
import { invoke } from "./lib/tauri";
import { I18nProvider, resolveLocale, useT } from "./i18n";
import { useAppState } from "./state/useAppState";
import { quotaTone, parsePlan, planTier, matchAuthFile, parseResetCredits } from "./lib/format";
import { HealthDots } from "./components/HealthDots";
import type { AccountQuota, AuthFile } from "./types";

function accountOverallRemaining(account: AccountQuota): number | null {
  if (account.models.length === 0) return null;
  return Math.round(
    account.models.reduce((sum, m) => sum + m.remaining_percent, 0) / account.models.length,
  );
}

function formatResetCountdown(resetAtUnix: number): string | null {
  const secs = resetAtUnix - Math.floor(Date.now() / 1000);
  if (secs <= 0) return null;
  const hours = Math.floor(secs / 3600);
  const mins = Math.floor((secs % 3600) / 60);
  if (hours >= 24) return `${Math.floor(hours / 24)}天后重置`;
  if (hours > 0) return `${hours}小时${mins > 0 ? mins + "分" : ""}后重置`;
  return `${mins}分钟后重置`;
}

const PROVIDER_LABELS: Record<string, string> = {
  codex: "Codex",
  claude: "Claude",
  copilot: "Copilot",
  antigravity: "Antigravity",
  kiro: "Kiro",
  glm: "GLM",
  trae: "Trae",
};

const MAX_MENUBAR_ACCOUNTS = 6;
const SUPPORTED_PROVIDERS = ["codex", "claude", "copilot", "antigravity", "kiro", "glm", "trae"];

function providerLabel(id: string) {
  return PROVIDER_LABELS[id] ?? id;
}

// Bar color reuses the Quota page thresholds (quotaTone): >50% green,
// 10–50% amber, ≤10% red — so the menu-bar matches the main view.
const BAR_COLORS: Record<"good" | "warn" | "bad", string> = {
  good: "#34c759",
  warn: "#f59e0b",
  bad: "#ef4444",
};
function barTone(remaining: number) {
  return BAR_COLORS[quotaTone(remaining)];
}

/// The tray "menu bar" floating quota panel (see ui/menu_bar.png). Rendered in a
/// separate always-on-top frameless window; toggled from the tray icon.
export default function MenuBarPanel() {
  const app = useAppState();
  const theme = app.appState?.settings.theme ?? "system";

  useEffect(() => {
    const root = document.documentElement;
    const apply = () => {
      const effective =
        theme === "system"
          ? window.matchMedia("(prefers-color-scheme: dark)").matches
            ? "dark"
            : "light"
          : theme;
      root.setAttribute("data-theme", effective);
      root.style.colorScheme = effective;
    };
    apply();
    if (theme !== "system") return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);

  // The panel stays pinned above other apps (alwaysOnTop) and is toggled from
  // the tray icon, so it does NOT hide on blur. Refresh it lightly while open
  // (reads the shared backend state the main window keeps fresh).
  useEffect(() => {
    void app.refreshQuotas();
    const stateInterval = window.setInterval(() => {
      void app.refreshState();
    }, 20000);
    // Re-probe the real quotas every 10 minutes while the panel is open.
    const quotaInterval = window.setInterval(() => {
      void app.refreshQuotas();
    }, 10 * 60 * 1000);
    return () => {
      window.clearInterval(stateInterval);
      window.clearInterval(quotaInterval);
    };
  }, []);

  if (!app.appState) {
    return (
      <div className="menubar-root menubar-root--loading">
        <span className="pulse" />
      </div>
    );
  }

  return (
    <I18nProvider locale={resolveLocale(app.appState.settings.language)}>
      <MenuBarBody app={app} />
    </I18nProvider>
  );
}

function MenuBarBody({ app }: { app: ReturnType<typeof useAppState> }) {
  const t = useT();
  const appState = app.appState!;
  const proxy = appState.proxy;
  const quotas = appState.quotas;
  const authFiles = appState.auth_files ?? [];
  const providerIds = useMemo(
    () => [...new Set([...SUPPORTED_PROVIDERS, ...quotas.map((q) => q.provider_id)])],
    [quotas],
  );
  const providersWithData = useMemo(() => new Set(quotas.map((q) => q.provider_id)), [quotas]);
  const [activeTab, setActiveTab] = useState<string>("");
  // Default to the first provider that actually has data so the panel opens on
  // something useful, but keep every supported tab clickable.
  const defaultTab = providerIds.find((id) => providersWithData.has(id)) ?? providerIds[0] ?? "";
  const tab = activeTab && providerIds.includes(activeTab) ? activeTab : defaultTab;
  const accounts = quotas.filter((q) => q.provider_id === tab);
  const visibleAccounts = accounts.slice(0, MAX_MENUBAR_ACCOUNTS);
  const hiddenCount = accounts.length - visibleAccounts.length;
  const running = proxy.status === "running";

  // Guard the footer actions (open / quit / show-all) against rapid repeat
  // clicks, so a double-click doesn't fire the command twice. Re-enables after a
  // short delay (the window only hides, it isn't destroyed, so state persists).
  const [acting, setActing] = useState(false);
  function runOnce(command: string) {
    if (acting) return;
    setActing(true);
    void invoke(command);
    window.setTimeout(() => setActing(false), 1500);
  }

  // Resize the window to fit the (capped) content so the panel grows with the
  // number of accounts instead of leaving empty space.
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const tabsHeight = providerIds.length > 0 ? Math.ceil(providerIds.length / 3) * 30 + 12 : 0;
    const listHeight =
      accounts.length === 0
        ? 56
        : visibleAccounts.reduce((sum, acc) => sum + 22 + 18 + acc.models.length * 28, 0) +
          Math.max(0, visibleAccounts.length - 1) * 10;
    const moreHeight = hiddenCount > 0 ? 38 : 0;
    const desired = Math.max(200, 38 + tabsHeight + 16 + listHeight + moreHeight + 116);
    void import("@tauri-apps/api/window").then(async ({ getCurrentWindow, LogicalSize, currentMonitor }) => {
      const win = getCurrentWindow();
      // Cap to the screen height so the panel grows to fit all accounts instead
      // of scrolling at a fixed 720px — but never taller than the monitor.
      let cap = 900;
      try {
        const monitor = await currentMonitor();
        if (monitor) cap = Math.floor(monitor.size.height / monitor.scaleFactor) - 80;
      } catch {
        /* keep the 900px fallback */
      }
      void win.setSize(new LogicalSize(280, Math.min(desired, cap)));
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [appState.quotas, tab]);

  return (
    <div className="menubar-root">
      <header
        className="menubar-head"
        onMouseDown={(event) => {
          // Drag the frameless panel by its header (skip clicks on the button).
          if ((event.target as HTMLElement).closest("button")) return;
          if (!("__TAURI_INTERNALS__" in window)) return;
          void import("@tauri-apps/api/window").then(({ getCurrentWindow }) => {
            void getCurrentWindow().startDragging();
          });
        }}
      >
        <strong>Quotio</strong>
        <div className="menubar-proxy">
          <span className="menubar-endpoint">{proxy.endpoint}</span>
          <button
            className={running ? "menubar-proxy-toggle menubar-proxy-toggle--on" : "menubar-proxy-toggle"}
            type="button"
            onClick={() => void app.runProxyAction(running ? "stop_proxy" : "start_proxy")}
            disabled={app.isProxyBusy}
          >
            {running ? t("menubar.stop") : t("menubar.start")}
          </button>
        </div>
      </header>

      {providerIds.length > 0 ? (
        <nav className="menubar-tabs">
          {providerIds.map((id) => (
            <button key={id} className={id === tab ? "active" : ""} type="button" onClick={() => setActiveTab(id)}>
              {providerLabel(id)}
            </button>
          ))}
        </nav>
      ) : null}

      <div className="menubar-list">
        {accounts.length === 0 ? (
          <p className="menubar-empty">{t("menubar.empty")}</p>
        ) : (
          <>
            {visibleAccounts.map((account) => (
              <MenuBarAccount key={account.account_key} account={account} authFiles={authFiles} />
            ))}
            {hiddenCount > 0 ? (
              <button className="menubar-more" type="button" disabled={acting} onClick={() => runOnce("show_main_window")}>
                {t("menubar.more").replace("{n}", String(accounts.length))}
              </button>
            ) : null}
          </>
        )}
      </div>

      <footer className="menubar-foot">
        <button type="button" onClick={() => void app.refreshQuotas()} disabled={app.isQuotaBusy}>
          <span className={app.isQuotaBusy ? "menubar-foot-icon menubar-foot-icon--spin" : "menubar-foot-icon"}>↻</span>
          {app.isQuotaBusy ? t("menubar.refreshing") : t("menubar.refresh")}
        </button>
        <button type="button" disabled={acting} onClick={() => runOnce("show_main_window")}>
          <span className="menubar-foot-icon">⤢</span>
          {t("menubar.open")}
        </button>
        <button type="button" disabled={acting} onClick={() => runOnce("quit_app")}>
          <span className="menubar-foot-icon">⏻</span>
          {t("menubar.quit")}
        </button>
      </footer>
    </div>
  );
}

function MenuBarAccount({ account, authFiles }: { account: AccountQuota; authFiles: AuthFile[] }) {
  const statusMessage = account.status_message ?? "";
  const plan = parsePlan(statusMessage);
  const tier = plan ? planTier(plan) : null;
  const expiry = statusMessage.match(/until:\s*([^|]+)/i)?.[1]?.trim();
  const isCodex = account.provider_id === "codex";
  const resetCredits = isCodex ? parseResetCredits(statusMessage) : null;
  const authFailed = statusMessage === "auth_failed";
  const weeklyUsedUp =
    !account.is_forbidden && account.models.some((m) => /weekly/i.test(m.model) && m.remaining_percent <= 0);
  const file = matchAuthFile(account, authFiles);
  const recent = file?.recent_requests ?? [];
  const successCount = file?.success ?? 0;
  const failedCount = file?.failed ?? 0;
  const overall = accountOverallRemaining(account);
  const nearestReset = account.models
    .filter((m) => m.reset_at_unix)
    .sort((a, b) => (a.reset_at_unix ?? 0) - (b.reset_at_unix ?? 0))[0];
  const countdown = nearestReset?.reset_at_unix ? formatResetCountdown(nearestReset.reset_at_unix) : null;

  return (
    <div className="menubar-account">
      <div className="menubar-account-head">
        <span className="menubar-account-name">{account.account_label}</span>
        {overall != null ? (
          <span className={`menubar-overall menubar-overall--${quotaTone(overall)}`}>{overall}%</span>
        ) : null}
        {plan ? <span className={`menubar-plan-pill menubar-plan-pill--${tier}`}>{plan.toUpperCase()}</span> : null}
      </div>

      <div className="menubar-account-meta">
        {account.is_forbidden ? <span className="menubar-tag menubar-tag--bad">已禁用</span> : null}
        {authFailed ? <span className="menubar-tag menubar-tag--bad">需重新授权</span> : null}
        {weeklyUsedUp ? <span className="menubar-tag menubar-tag--warn">本周已用尽</span> : null}
        {account.warming_up ? <span className="menubar-tag menubar-tag--warn">预热中</span> : null}
        {account.in_use ? <span className="menubar-tag menubar-tag--blue">使用中</span> : null}
        {file?.quotio_scheduler_standby && file?.disabled ? (
          <span className="menubar-tag menubar-tag--blue">调度待命</span>
        ) : null}
        {resetCredits != null ? <span className="menubar-tag">重置×{resetCredits}</span> : null}
        {expiry ? <span className="menubar-meta-text">到期 {expiry}</span> : null}
        {countdown ? <span className="menubar-meta-text">{countdown}</span> : null}
        {(successCount > 0 || failedCount > 0) ? (
          <span className="menubar-meta-text">
            ✓{successCount}{failedCount > 0 ? <span className="menubar-fail"> ✗{failedCount}</span> : null}
          </span>
        ) : null}
      </div>

      {account.models.map((model) => {
        const remaining = Math.max(0, Math.min(100, model.remaining_percent));
        return (
          <div className="menubar-model" key={model.model}>
            <div className="menubar-model-top">
              <span className="menubar-model-name">{model.model}</span>
              {model.reset_at ? <span className="menubar-model-reset">{model.reset_at}</span> : null}
              <span className="menubar-model-pct">{Math.round(remaining)}%</span>
            </div>
            <div className="menubar-bar">
              <div className="menubar-bar-fill" style={{ width: `${remaining}%`, background: barTone(remaining) }} />
            </div>
          </div>
        );
      })}
      {recent.length > 0 ? <HealthDots recent={recent} compact /> : null}
    </div>
  );
}
