import { useEffect, useMemo, useState } from "react";
import "./App.css";
import "./menubar.css";
import "./components/rose-theme.css";
import { invoke } from "./lib/tauri";
import { applyTheme, resolveEffectiveTheme } from "./lib/theme";
import { I18nProvider, resolveLocale, useT } from "./i18n";
import { useAppState } from "./state/useAppState";
import { quotaTone, parsePlan, matchAuthFile, parseResetCredits } from "./lib/format";
import { HealthDots } from "./components/HealthDots";
import type { AccountQuota, AuthFile, QuotaModelUsage } from "./types";

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
  if (hours >= 24) return `${Math.floor(hours / 24)} 天后重置`;
  if (hours > 0) return `${hours} 小时${mins > 0 ? " " + mins + " 分" : ""}后重置`;
  return `${mins} 分钟后重置`;
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

const SUPPORTED_PROVIDERS = ["codex", "claude", "copilot", "antigravity", "kiro", "glm", "trae"];

function providerLabel(id: string) {
  return PROVIDER_LABELS[id] ?? id;
}

// Session / Weekly 两个主窗口(与「额度」页口径一致):Session 优先按名字匹配,匹配不到就取
// 「非 Weekly 的那个」当作 Session。悬浮窗只呈现这两行,与设计稿固定的两条进度条对应。
function splitSessionWeekly(models: QuotaModelUsage[]): { model: QuotaModelUsage; label: string }[] {
  const weekly = models.find((m) => /weekly/i.test(m.model)) ?? null;
  let session = models.find((m) => /session|5h|5\s*hour/i.test(m.model)) ?? null;
  if (!session) session = models.find((m) => m !== weekly) ?? null;
  const rows: { model: QuotaModelUsage; label: string }[] = [];
  if (session) rows.push({ model: session, label: "Session" });
  if (weekly) rows.push({ model: weekly, label: "Weekly" });
  return rows;
}

// 本地 SVG sprite 图标(素材见 public/floating/floating-window-icons.svg),以 currentColor 继承状态色。
function Icon({ id }: { id: string }) {
  return (
    <svg className="icon" aria-hidden="true">
      <use href={`/floating/floating-window-icons.svg#${id}`} />
    </svg>
  );
}

/// The tray "menu bar" floating quota panel (see 设计图/原版/悬浮窗). Rendered in a
/// separate always-on-top frameless transparent window; toggled from the tray icon.
export default function MenuBarPanel() {
  const app = useAppState();
  const theme = app.appState?.settings.theme ?? "system";

  useEffect(() => {
    const root = document.documentElement;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const syncTheme = () => {
      applyTheme(root, resolveEffectiveTheme(theme, media.matches));
    };
    syncTheme();
    if (theme !== "system") return;
    media.addEventListener("change", syncTheme);
    return () => media.removeEventListener("change", syncTheme);
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
      <div className="floating-window floating-window--loading">
        <span className="fw-pulse" />
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
  const running = proxy.status === "running";

  // Guard the footer actions (open / quit) against rapid repeat clicks, so a
  // double-click doesn't fire the command twice. Re-enables after a short delay
  // (the window only hides, it isn't destroyed, so state persists).
  const [acting, setActing] = useState(false);
  function runOnce(command: string) {
    if (acting) return;
    setActing(true);
    void invoke(command);
    window.setTimeout(() => setActing(false), 1500);
  }

  // Resize the window to fit the content so the panel grows with the number of
  // accounts, but caps to the screen height (or 960px logical) and lets the
  // middle account list scroll. Header 48 + tabs + list + footer 144.
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const tabsHeight = providerIds.length > 0 ? Math.ceil(providerIds.length / 4) * 28 + 17 : 0;
    const listHeight =
      accounts.length === 0 ? 56 : accounts.length * 156 + Math.max(0, accounts.length - 1) * 10 + 22;
    const desired = Math.max(220, 48 + tabsHeight + listHeight + 144);
    void import("@tauri-apps/api/window").then(async ({ getCurrentWindow, LogicalSize, currentMonitor }) => {
      const win = getCurrentWindow();
      let cap = 960;
      try {
        const monitor = await currentMonitor();
        if (monitor) cap = Math.min(960, Math.floor(monitor.size.height / monitor.scaleFactor) - 80);
      } catch {
        /* keep the 960px fallback */
      }
      void win.setSize(new LogicalSize(360, Math.min(desired, cap)));
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [appState.quotas, tab]);

  return (
    <main className="floating-window">
      <header
        className="window-header"
        onMouseDown={(event) => {
          // Drag the frameless panel by its header (skip clicks on the button).
          if ((event.target as HTMLElement).closest("button")) return;
          if (!("__TAURI_INTERNALS__" in window)) return;
          void import("@tauri-apps/api/window").then(({ getCurrentWindow }) => {
            void getCurrentWindow().startDragging();
          });
        }}
      >
        <img className="rose-menubar-avatar" src="/rose/character-avatar.png" alt="" aria-hidden="true" />
        <div className="brand">Quotio</div>
        <div
          className={running ? "endpoint" : "endpoint is-stopped"}
          title={running ? `${t("menubar.running", "本地服务运行中")}：${proxy.endpoint}` : proxy.endpoint}
        >
          <svg className="status-icon" aria-hidden="true">
            <use href="/floating/floating-window-icons.svg#icon-status" />
          </svg>
          <code>{proxy.endpoint}</code>
        </div>
        <button
          className={running ? "stop-button" : "stop-button is-start"}
          type="button"
          onClick={() => void app.runProxyAction(running ? "stop_proxy" : "start_proxy")}
          disabled={app.isProxyBusy}
        >
          {running ? <Icon id="icon-stop" /> : null}
          {running ? t("menubar.stop") : t("menubar.start")}
        </button>
      </header>

      {providerIds.length > 0 ? (
        <nav className="provider-tabs" aria-label={t("menubar.tabsLabel", "智能体筛选")}>
          {providerIds.map((id) => (
            <button
              key={id}
              className={id === tab ? "provider-tab active" : "provider-tab"}
              type="button"
              aria-pressed={id === tab}
              onClick={() => setActiveTab(id)}
            >
              {providerLabel(id)}
            </button>
          ))}
        </nav>
      ) : null}

      <section className="account-scroll" aria-label={t("menubar.listLabel", "账号额度列表")}>
        {accounts.length === 0 ? (
          <p className="account-empty">{t("menubar.empty")}</p>
        ) : (
          accounts.map((account) => (
            <MenuBarAccount key={account.account_key} account={account} authFiles={authFiles} />
          ))
        )}
      </section>

      <footer className="window-actions">
        <button
          className="window-action action-refresh"
          type="button"
          onClick={() => void app.refreshQuotas()}
          disabled={app.isQuotaBusy}
        >
          <span className={app.isQuotaBusy ? "fw-action-icon--spin" : ""}>
            <Icon id="icon-refresh" />
          </span>
          <span>{app.isQuotaBusy ? t("menubar.refreshing") : t("menubar.refresh")}</span>
        </button>
        <button className="window-action action-open" type="button" disabled={acting} onClick={() => runOnce("show_main_window")}>
          <Icon id="icon-open" />
          <span>{t("menubar.open")}</span>
        </button>
        <button className="window-action action-exit" type="button" disabled={acting} onClick={() => runOnce("quit_app")}>
          <Icon id="icon-exit" />
          <span>{t("menubar.quit")}</span>
        </button>
      </footer>
    </main>
  );
}

function MenuBarAccount({ account, authFiles }: { account: AccountQuota; authFiles: AuthFile[] }) {
  const statusMessage = account.status_message ?? "";
  const plan = parsePlan(statusMessage);
  const expiry = statusMessage.match(/until:\s*([^|]+)/i)?.[1]?.trim();
  const isCodex = account.provider_id === "codex";
  const resetCredits = isCodex ? parseResetCredits(statusMessage) : null;
  const file = matchAuthFile(account, authFiles);
  const recent = file?.recent_requests ?? [];
  const overall = accountOverallRemaining(account);
  const overallTone = overall != null ? quotaTone(overall) : "good";
  const nearestReset = account.models
    .filter((m) => m.reset_at_unix)
    .sort((a, b) => (a.reset_at_unix ?? 0) - (b.reset_at_unix ?? 0))[0];
  const countdown = nearestReset?.reset_at_unix ? formatResetCountdown(nearestReset.reset_at_unix) : null;
  const rows = splitSessionWeekly(account.models);

  return (
    <article className={`account-card account-card--${overallTone}`}>
      <div className="account-title">
        <span className="account-email" title={account.account_label}>
          {account.account_label}
        </span>
        {overall != null ? (
          <span className="health-percent" title={`健康 ${overall}%`}>
            {overall}%
          </span>
        ) : null}
        {plan ? <span className="plan-badge">{plan.toUpperCase()}</span> : null}
      </div>

      <div className="account-meta">
        {resetCredits != null ? (
          <span className="meta-item">
            <Icon id="icon-calendar" />
            重置 ×{resetCredits}
          </span>
        ) : null}
        {expiry ? (
          <span className="meta-item">
            <Icon id="icon-calendar" />
            到期 {expiry}
          </span>
        ) : null}
        {countdown ? (
          <span className="meta-item">
            <Icon id="icon-clock" />
            {countdown}
          </span>
        ) : null}
      </div>

      {recent.length > 0 ? <HealthDots recent={recent} /> : <div className="health-dots" aria-hidden="true" />}

      {rows.map(({ model, label }) => {
        const remaining = Math.max(0, Math.min(100, model.remaining_percent));
        const tone = quotaTone(model.remaining_percent);
        return (
          <div className={`quota-row quota-row--${tone}`} key={model.model}>
            <span className="quota-name">{label}</span>
            <span className="quota-time">{model.reset_at ?? ""}</span>
            <strong className="quota-percent">{Math.round(remaining)}%</strong>
            <div className="progress-track">
              <div className="progress-fill" style={{ width: `${remaining}%` }} />
            </div>
          </div>
        );
      })}
    </article>
  );
}
