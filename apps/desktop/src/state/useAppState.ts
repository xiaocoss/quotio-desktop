import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../lib/tauri";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { listen } from "@tauri-apps/api/event";
import type {
  AgentBackupFile,
  AgentConfigurationRequest,
  AgentConfigurationResult,
  AppSettings,
  AppState,
  AvailableModel,
  CredentialStatus,
  FallbackConfigAction,
  OAuthStatusResponse,
  OAuthUrlResponse,
  ProxyCommand,
  SavedAgentConfiguration,
} from "../types";

export function useAppState() {
  const [appState, setAppState] = useState<AppState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [proxyAction, setProxyAction] = useState<string | null>(null);
  const [managementAction, setManagementAction] = useState<string | null>(null);
  const [localAction, setLocalAction] = useState<string | null>(null);
  const [agentAction, setAgentAction] = useState<string | null>(null);
  const [fallbackAction, setFallbackAction] = useState<string | null>(null);
  const [platformAction, setPlatformAction] = useState<string | null>(null);
  const [agentResult, setAgentResult] = useState<AgentConfigurationResult | null>(null);
  const [agentBackups, setAgentBackups] = useState<Record<string, AgentBackupFile[]>>({});
  const [agentConfigurations, setAgentConfigurations] = useState<Record<string, SavedAgentConfiguration>>({});
  const [availableModels, setAvailableModels] = useState<AvailableModel[]>([]);
  const [credentialStatus, setCredentialStatus] = useState<CredentialStatus | null>(null);
  const [proxyUrlDraft, setProxyUrlDraft] = useState("");
  const [isQuotaBusy, setIsQuotaBusy] = useState(false);
  const lowQuotaNotified = useRef<Set<string>>(new Set());
  const proxyDraftSeeded = useRef(false);
  const isMenuBarView =
    window.location.hash.replace(/^#/, "") === "menubar" ||
    new URLSearchParams(window.location.search).get("view") === "menubar";

  useEffect(() => {
    void refreshState();
  }, []);

  // Cross-window sync: the backend emits "proxy-changed" whenever the proxy is
  // started/stopped (from the main window OR the menu-bar panel); every window
  // re-fetches so the sidebar + menu-bar proxy status stay consistent.
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | undefined;
    void listen("proxy-changed", () => {
      void refreshState();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Background poll: drain the proxy's request-log queue (60s retention window)
  // while the local proxy runs, so the Logs "Requests" tab fills without manual
  // refresh. Only active in the real Tauri app (skipped in the browser mock).
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    if (isMenuBarView) return;
    // Drain every 15s regardless of the app-tracked proxy status. The command
    // no-ops when the proxy is unreachable, and this also covers a proxy that is
    // running but was not started by (or was orphaned from) this app session.
    const interval = window.setInterval(() => {
      invoke<AppState>("drain_request_logs")
        .then(setAppState)
        .catch(() => {});
    }, 15000);
    return () => window.clearInterval(interval);
  }, []);

  // Periodically refresh quotas in the background so low-quota notifications
  // fire even when the user isn't on the Quota page (Tauri only).
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    if (isMenuBarView) return;
    void refreshQuotas();
    const interval = window.setInterval(() => {
      void refreshQuotas();
    }, 5 * 60 * 1000);
    return () => window.clearInterval(interval);
  }, []);

  async function refreshState() {
    try {
      const state = await invoke<AppState>("get_app_state");
      setAppState(state);
      setCredentialStatus(state.credentials);
      setAvailableModels(state.fallback_runtime.available_models);
      // Seed the upstream-proxy input from the persisted setting on first load,
      // so a saved proxy URL shows after restart instead of an empty field.
      if (!proxyDraftSeeded.current) {
        proxyDraftSeeded.current = true;
        if (state.settings.proxy_url) setProxyUrlDraft(state.settings.proxy_url);
      }
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }

  async function refreshQuotas() {
    setIsQuotaBusy(true);
    try {
      const state = await invoke<AppState>("refresh_quotas");
      setAppState(state);
      void notifyLowQuotas(state);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setIsQuotaBusy(false);
    }
  }

  /// Fire a desktop notification when a model's remaining quota drops to/below
  /// 10%, once per account+model until it recovers. Best-effort (Tauri only).
  async function notifyLowQuotas(state: AppState) {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const low: string[] = [];
    for (const account of state.quotas) {
      for (const model of account.models) {
        const key = `${account.account_key}:${model.model}`;
        if (model.remaining_percent <= 10) {
          if (!lowQuotaNotified.current.has(key)) {
            lowQuotaNotified.current.add(key);
            low.push(`${account.account_label} · ${model.model}`);
          }
        } else {
          lowQuotaNotified.current.delete(key);
        }
      }
    }
    if (low.length === 0) return;
    try {
      if (await isPermissionGranted()) {
        sendNotification({ title: "Quotio", body: `额度不足 / Low quota: ${low.join("，")}` });
      }
    } catch {
      /* notifications are best-effort */
    }
  }

  async function saveSettings(settings: AppSettings) {
    setIsSaving(true);
    try {
      const nextState = await invoke<AppState>("save_settings", { settings });
      setAppState(nextState);
      setCredentialStatus(nextState.credentials);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setIsSaving(false);
    }
  }

  async function toggleNotifications() {
    if (!appState) return;

    await saveSettings({
      ...appState.settings,
      notifications_enabled: !appState.settings.notifications_enabled,
    });
  }

  async function runProxyAction(command: ProxyCommand) {
    setProxyAction(command);
    try {
      const nextState = await invoke<AppState>(command);
      setAppState(nextState);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setProxyAction(null);
    }
  }

  async function runManagementStateAction(command: string, args?: Record<string, unknown>) {
    setManagementAction(command);
    try {
      const nextState = await invoke<AppState>(command, args);
      setAppState(nextState);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setManagementAction(null);
    }
  }

  async function runFallbackConfigAction(action: FallbackConfigAction) {
    setLocalAction("update_fallback_configuration");
    try {
      const nextState = await invoke<AppState>("update_fallback_configuration", { action });
      setAppState(nextState);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setLocalAction(null);
    }
  }

  async function startOAuth(endpoint: string, projectId: string | null, isWebui = false) {
    const command = "start_management_oauth";
    setManagementAction(command);
    try {
      const response = await invoke<OAuthUrlResponse>(command, {
        endpoint,
        projectId: projectId && projectId.trim().length > 0 ? projectId.trim() : null,
        isWebui,
      });
      setError(response.error);
      return response;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
    } finally {
      setManagementAction(null);
    }
  }

  async function pollOAuth(token: string) {
    const command = "poll_management_oauth";
    setManagementAction(command);
    try {
      const response = await invoke<OAuthStatusResponse>(command, { token });
      setError(response.error);
      // CLIProxyAPI's /get-auth-status reports success as "ok" (matching the
      // macOS reference app); keep success/completed as tolerant fallbacks.
      if (response.status === "ok" || response.status === "success" || response.status === "completed") {
        const nextState = await invoke<AppState>("refresh_management_state");
        setAppState(nextState);
      }
      return response;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
    } finally {
      setManagementAction(null);
    }
  }

  async function refreshProxyUrlDraft() {
    setManagementAction("get_management_proxy_url");
    try {
      const value = await invoke<string>("get_management_proxy_url");
      setProxyUrlDraft(value);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setManagementAction(null);
    }
  }

  async function refreshAgentStatuses() {
    setAgentAction("detect_agents");
    try {
      const nextState = await invoke<AppState>("detect_agents");
      setAppState(nextState);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setAgentAction(null);
    }
  }

  async function readAgentConfiguration(agentId: string) {
    setAgentAction(`read_agent_configuration:${agentId}`);
    try {
      const configuration = await invoke<SavedAgentConfiguration>("read_agent_configuration", { agentId });
      setAgentConfigurations((current) => ({ ...current, [agentId]: configuration }));
      setAgentBackups((current) => ({ ...current, [agentId]: configuration.backups }));
      setError(null);
      return configuration;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
    } finally {
      setAgentAction(null);
    }
  }

  async function configureAgent(request: AgentConfigurationRequest) {
    setAgentAction(`configure_agent:${request.agent_id}`);
    try {
      const result = await invoke<AgentConfigurationResult>("configure_agent", { request });
      setAgentResult(result);
      setAgentBackups((current) => ({ ...current, [request.agent_id]: result.backups }));
      const nextState = await invoke<AppState>("detect_agents");
      setAppState(nextState);
      setError(result.error);
      return result;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
    } finally {
      setAgentAction(null);
    }
  }

  async function listAgentBackups(agentId: string) {
    setAgentAction(`list_agent_backups:${agentId}`);
    try {
      const backups = await invoke<AgentBackupFile[]>("list_agent_backups", { agentId });
      setAgentBackups((current) => ({ ...current, [agentId]: backups }));
      setError(null);
      return backups;
    } catch (cause) {
      setError(errorMessage(cause));
      return [];
    } finally {
      setAgentAction(null);
    }
  }

  async function restoreAgentBackup(agentId: string, backupPath: string) {
    setAgentAction(`restore_agent_backup:${agentId}`);
    try {
      const result = await invoke<AgentConfigurationResult>("restore_agent_backup", { agentId, backupPath });
      setAgentResult(result);
      setAgentBackups((current) => ({ ...current, [agentId]: result.backups }));
      const nextState = await invoke<AppState>("detect_agents");
      setAppState(nextState);
      setError(result.error);
      return result;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
    } finally {
      setAgentAction(null);
    }
  }

  async function resetAgentConfiguration(agentId: string) {
    setAgentAction(`reset_agent_configuration:${agentId}`);
    try {
      const result = await invoke<AgentConfigurationResult>("reset_agent_configuration", { agentId });
      setAgentResult(result);
      setAgentBackups((current) => ({ ...current, [agentId]: result.backups }));
      const nextState = await invoke<AppState>("detect_agents");
      setAppState(nextState);
      setError(result.error);
      return result;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
    } finally {
      setAgentAction(null);
    }
  }

  async function discoverAvailableModels() {
    setFallbackAction("discover_available_models");
    try {
      const models = await invoke<AvailableModel[]>("discover_available_models");
      setAvailableModels(models);
      const nextState = await invoke<AppState>("get_app_state");
      setAppState(nextState);
      setError(null);
      return models;
    } catch (cause) {
      setError(errorMessage(cause));
      return [];
    } finally {
      setFallbackAction(null);
    }
  }

  async function refreshFallbackRouteState() {
    setFallbackAction("refresh_fallback_route_state");
    try {
      const nextState = await invoke<AppState>("refresh_fallback_route_state");
      setAppState(nextState);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setFallbackAction(null);
    }
  }

  async function refreshCredentialStatus() {
    setPlatformAction("credential_status");
    try {
      const status = await invoke<CredentialStatus>("credential_status");
      setCredentialStatus(status);
      setAppState((current) => (current ? { ...current, credentials: status } : current));
      setError(null);
      return status;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
    } finally {
      setPlatformAction(null);
    }
  }

  async function clearRemoteManagementKey() {
    setPlatformAction("clear_remote_management_key");
    try {
      const nextState = await invoke<AppState>("clear_remote_management_key");
      setAppState(nextState);
      setCredentialStatus(nextState.credentials);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setPlatformAction(null);
    }
  }

  async function openConfigRoot() {
    setPlatformAction("open_config_root");
    try {
      await invoke<void>("open_config_root");
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setPlatformAction(null);
    }
  }

  async function setLaunchAtLogin(enabled: boolean) {
    setPlatformAction("set_launch_at_login");
    try {
      const nextState = await invoke<AppState>("set_launch_at_login", { enabled });
      setAppState(nextState);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setPlatformAction(null);
    }
  }

  async function requestNotificationPermission() {
    setPlatformAction("request_notification_permission");
    try {
      let allowed = await isPermissionGranted();
      if (!allowed) {
        const permission = await requestPermission();
        allowed = permission === "granted";
      }
      setError(allowed ? null : "系统通知未授权。");
      return allowed;
    } catch (cause) {
      setError(errorMessage(cause));
      return false;
    } finally {
      setPlatformAction(null);
    }
  }

  async function sendTestNotification() {
    setPlatformAction("send_test_notification");
    try {
      const allowed = await requestNotificationPermission();
      if (!allowed) return;
      sendNotification({ title: "Quotio", body: "桌面通知已可用。" });
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setPlatformAction(null);
    }
  }

  const isProxyBusy = useMemo(
    () => proxyAction !== null || appState?.proxy.status === "starting" || appState?.proxy.status === "stopping",
    [appState?.proxy.status, proxyAction],
  );
  const isManagementBusy = managementAction !== null;

  return {
    appState,
    error,
    isSaving,
    proxyAction,
    managementAction,
    localAction,
    agentAction,
    fallbackAction,
    platformAction,
    agentResult,
    agentBackups,
    agentConfigurations,
    availableModels,
    credentialStatus,
    proxyUrlDraft,
    isProxyBusy,
    isManagementBusy,
    isQuotaBusy,
    setProxyUrlDraft,
    refreshState,
    refreshQuotas,
    saveSettings,
    toggleNotifications,
    runProxyAction,
    runManagementStateAction,
    runFallbackConfigAction,
    startOAuth,
    pollOAuth,
    refreshProxyUrlDraft,
    refreshAgentStatuses,
    readAgentConfiguration,
    configureAgent,
    listAgentBackups,
    restoreAgentBackup,
    resetAgentConfiguration,
    discoverAvailableModels,
    refreshFallbackRouteState,
    refreshCredentialStatus,
    clearRemoteManagementKey,
    openConfigRoot,
    setLaunchAtLogin,
    requestNotificationPermission,
    sendTestNotification,
  };
}

function errorMessage(cause: unknown) {
  return cause instanceof Error ? cause.message : String(cause);
}