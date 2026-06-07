import { useEffect, useMemo, useState } from "react";
import "./App.css";
import { invoke } from "./lib/tauri";
import { I18nProvider, resolveLocale, useT } from "./i18n";
import { useAppState } from "./state/useAppState";
import type { AccountQuota } from "./types";

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

function barTone(remaining: number) {
  if (remaining <= 10) return "#ef4444";
  if (remaining <= 30) return "#f59e0b";
  return "#34c759";
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
    }, 10000);
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

  // Resize the window to fit the (capped) content so the panel grows with the
  // number of accounts instead of leaving empty space.
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const tabsHeight = providerIds.length > 0 ? Math.ceil(providerIds.length / 3) * 30 + 12 : 0;
    const listHeight =
      accounts.length === 0
        ? 56
        : visibleAccounts.reduce((sum, acc) => sum + 22 + acc.models.length * 28, 0) +
          Math.max(0, visibleAccounts.length - 1) * 10;
    const moreHeight = hiddenCount > 0 ? 38 : 0;
    const height = Math.min(720, Math.max(200, 38 + tabsHeight + 16 + listHeight + moreHeight + 116));
    void import("@tauri-apps/api/window").then(({ getCurrentWindow, LogicalSize }) => {
      void getCurrentWindow().setSize(new LogicalSize(280, height));
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [appState.quotas, tab]);

  return (
    <div className="menubar-root">
      <header className="menubar-head" data-tauri-drag-region>
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
              <MenuBarAccount key={account.account_key} account={account} />
            ))}
            {hiddenCount > 0 ? (
              <button className="menubar-more" type="button" onClick={() => void invoke("show_main_window")}>
                {t("menubar.more").replace("{n}", String(accounts.length))}
              </button>
            ) : null}
          </>
        )}
      </div>

      <footer className="menubar-foot">
        <button type="button" onClick={() => void app.refreshQuotas()} disabled={app.isQuotaBusy}>
          <span className="menubar-foot-icon">↻</span>
          {t("menubar.refresh")}
        </button>
        <button type="button" onClick={() => void invoke("show_main_window")}>
          <span className="menubar-foot-icon">⤢</span>
          {t("menubar.open")}
        </button>
        <button type="button" onClick={() => void invoke("quit_app")}>
          <span className="menubar-foot-icon">⏻</span>
          {t("menubar.quit")}
        </button>
      </footer>
    </div>
  );
}

function MenuBarAccount({ account }: { account: AccountQuota }) {
  return (
    <div className="menubar-account">
      <div className="menubar-account-head">
        <span className="menubar-account-name">{account.account_label}</span>
        {account.is_forbidden ? <span className="menubar-pill menubar-pill--warn">!</span> : null}
      </div>
      {account.models.map((model) => {
        const remaining = Math.max(0, Math.min(100, model.remaining_percent));
        return (
          <div className="menubar-model" key={model.model}>
            <div className="menubar-model-top">
              <span className="menubar-model-name">{model.model}</span>
              <span className="menubar-model-pct">{Math.round(remaining)}%</span>
            </div>
            <div className="menubar-bar">
              <div className="menubar-bar-fill" style={{ width: `${remaining}%`, background: barTone(remaining) }} />
            </div>
          </div>
        );
      })}
    </div>
  );
}
