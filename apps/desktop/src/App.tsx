import { useEffect, useState } from "react";
import "./App.css";
import { AppShell } from "./components/AppShell";
import { I18nProvider, resolveLocale } from "./i18n";
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

  // Keep the boot screen up for a brief minimum so the app-open animation is
  // actually seen, even when state loads instantly. The window is revealed only
  // once this screen has painted (see main.tsx), so there's still no white flash.
  const [bootDone, setBootDone] = useState(false);
  useEffect(() => {
    const timer = window.setTimeout(() => setBootDone(true), 5000);
    return () => window.clearTimeout(timer);
  }, []);

  const theme = app.appState?.settings.theme ?? "system";
  useEffect(() => {
    const root = document.documentElement;
    const applyTheme = () => {
      const effective =
        theme === "system"
          ? window.matchMedia("(prefers-color-scheme: dark)").matches
            ? "dark"
            : "light"
          : theme;
      root.setAttribute("data-theme", effective);
      root.style.colorScheme = effective;
    };
    applyTheme();
    if (theme !== "system") return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", applyTheme);
    return () => media.removeEventListener("change", applyTheme);
  }, [theme]);

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
            {app.error ?? "Loading..."}
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
        onSaveSettings={(settings) => void app.saveSettings(settings)}
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