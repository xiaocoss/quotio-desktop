import { useEffect } from "react";
import "./App.css";
import { AppShell } from "./components/AppShell";
import { I18nProvider, resolveLocale } from "./i18n";
import { useAppState } from "./state/useAppState";

function App() {
  const app = useAppState();

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

  if (!app.appState) {
    return (
      <main className="app-shell app-shell--centered">
        <div className="loading-card">
          <span className="pulse" />
          <p>正在加载 Quotio Dashboard...</p>
          {app.error ? <strong>{app.error}</strong> : null}
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
        onRefreshState={() => void app.refreshState()}
        onRefreshQuotas={() => void app.refreshQuotas()}
        onToggleNotifications={() => void app.toggleNotifications()}
        onRunProxyAction={(command) => void app.runProxyAction(command)}
        onSaveSettings={(settings) => void app.saveSettings(settings)}
        onRunManagementStateAction={(command, args) => void app.runManagementStateAction(command, args)}
        onRunFallbackConfigAction={(action) => void app.runFallbackConfigAction(action)}
        onStartOAuth={app.startOAuth}
        onPollOAuth={app.pollOAuth}
        onRefreshProxyUrlDraft={() => void app.refreshProxyUrlDraft()}
        onRefreshAgentStatuses={() => void app.refreshAgentStatuses()}
        onReadAgentConfiguration={app.readAgentConfiguration}
        onConfigureAgent={app.configureAgent}
        onListAgentBackups={app.listAgentBackups}
        onRestoreAgentBackup={app.restoreAgentBackup}
        onResetAgentConfiguration={app.resetAgentConfiguration}
        onDiscoverAvailableModels={app.discoverAvailableModels}
        onRefreshFallbackRouteState={() => void app.refreshFallbackRouteState()}
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