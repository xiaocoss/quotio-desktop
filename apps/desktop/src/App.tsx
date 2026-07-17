import { useEffect, useState } from "react";
import "./App.css";
import "./components/shell.css";
import "./components/about.css";
import "./components/rose-theme.css";
import { AppShell } from "./components/AppShell";
import { I18nProvider, resolveLocale } from "./i18n";
import { applyTheme, resolveEffectiveTheme } from "./lib/theme";
import { useAppState } from "./state/useAppState";

function App() {
  const app = useAppState();

  // Manual-refresh loading overlay: set only while a user-clicked refresh runs,
  // so the background auto-poll (which calls the refresh directly) never fires
  // it. Drives the same card popup as the proxy start/stop loading.
  const [isRefreshing, setIsRefreshing] = useState(false);
  const manualRefresh = (fn: () => Promise<unknown>) => async () => {
    setIsRefreshing(true);
    try {
      await fn();
    } finally {
      setIsRefreshing(false);
    }
  };

  const [bootDone, setBootDone] = useState(false);
  useEffect(() => {
    const timer = window.setTimeout(() => setBootDone(true), 1500);
    return () => window.clearTimeout(timer);
  }, []);

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

  // 长按窗口任意处拖动:无边框窗口默认只有标题栏能拖。这里监听鼠标——左键按住约 260ms
  // 不松开,即进入「拖窗」,之后不松开继续移动整窗跟着走;快速点击仍是正常点击。
  // 按钮 / 输入框 / 下拉 / 滑块 / 可编辑 / 链接、以及标了 .no-window-drag 的区域排除
  //(这些长按另有用途)。仅 Tauri 环境生效。
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const HOLD_MS = 260;
    const NO_DRAG =
      "button, [role='button'], input, textarea, select, [contenteditable], [contenteditable=''], [role='slider'], a[href], .no-window-drag";
    let timer: number | null = null;
    const clear = () => {
      if (timer !== null) {
        window.clearTimeout(timer);
        timer = null;
      }
    };
    const onDown = (event: MouseEvent) => {
      if (event.button !== 0) return;
      const target = event.target as HTMLElement | null;
      if (target?.closest(NO_DRAG)) return;
      clear();
      timer = window.setTimeout(() => {
        timer = null;
        void import("@tauri-apps/api/window").then(({ getCurrentWindow }) =>
          getCurrentWindow().startDragging().catch(() => {}),
        );
      }, HOLD_MS);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("mouseup", clear);
    window.addEventListener("blur", clear);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("mouseup", clear);
      window.removeEventListener("blur", clear);
      clear();
    };
  }, []);

  if (!app.appState || !bootDone) {
    return (
      <main className="app-shell app-shell--centered">
        <div className="boot-screen">
          <div className="boot-logo" aria-hidden="true">
            Q
          </div>
          <p className="boot-title">Quotio</p>
          <div className="boot-bar" aria-hidden="true">
            <span />
          </div>
          <p className={app.error ? "boot-hint boot-hint--error" : "boot-hint"}>
            {app.error ?? "正在加载…"}
          </p>
        </div>
      </main>
    );
  }

  return (
    <I18nProvider locale={resolveLocale(app.appState.settings.language)}>
      {app.error ? <div className="floating-error">{app.error}</div> : null}
      <AppShell
        appState={app.appState}
        isSaving={app.isSaving}
        isProxyBusy={app.isProxyBusy}
        isManagementBusy={app.isManagementBusy}
        isQuotaBusy={app.isQuotaBusy}
        quotaToast={app.quotaToast}
        isRefreshing={isRefreshing}
        proxyAction={app.proxyAction}
        managementAction={app.managementAction}
        localAction={app.localAction}
        agentAction={app.agentAction}
        fallbackAction={app.fallbackAction}
        platformAction={app.platformAction}
        agentResult={app.agentResult}
        agentBackups={app.agentBackups}
        agentConfigurations={app.agentConfigurations}
        availableModels={app.availableModels}
        credentialStatus={app.credentialStatus}
        proxyUrlDraft={app.proxyUrlDraft}
        onProxyUrlDraftChange={app.setProxyUrlDraft}
        onRefreshState={manualRefresh(() => app.refreshState())}
        onRefreshQuotas={() => void app.refreshQuotas(true)}
        onToggleNotifications={() => void app.toggleNotifications()}
        onRunProxyAction={(command) => void app.runProxyAction(command)}
        onSaveSettings={(settings, options) => void app.saveSettings(settings, options)}
        onRunManagementStateAction={(command, args) => void app.runManagementStateAction(command, args)}
        onRunFallbackConfigAction={(action) => void app.runFallbackConfigAction(action)}
        onStartOAuth={app.startOAuth}
        onPollOAuth={app.pollOAuth}
        onRefreshProxyUrlDraft={manualRefresh(() => app.refreshProxyUrlDraft())}
        onRefreshAgentStatuses={manualRefresh(() => app.refreshAgentStatuses())}
        onReadAgentConfiguration={app.readAgentConfiguration}
        onConfigureAgent={app.configureAgent}
        onListAgentBackups={app.listAgentBackups}
        onRestoreAgentBackup={app.restoreAgentBackup}
        onResetAgentConfiguration={app.resetAgentConfiguration}
        onDiscoverAvailableModels={app.discoverAvailableModels}
        onRefreshFallbackRouteState={manualRefresh(() => app.refreshFallbackRouteState())}
        onRefreshCredentialStatus={app.refreshCredentialStatus}
        onClearRemoteManagementKey={() => void app.clearRemoteManagementKey()}
        onOpenConfigRoot={() => void app.openConfigRoot()}
        onSetLaunchAtLogin={(enabled) => void app.setLaunchAtLogin(enabled)}
        onRequestNotificationPermission={app.requestNotificationPermission}
        onSendTestNotification={() => void app.sendTestNotification()}
      />
    </I18nProvider>
  );
}

export default App;
