import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../lib/tauri";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { listen } from "@tauri-apps/api/event";
import type {
  AccountQuota,
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

// Replace the matching account (by provider + key) or append it, so streamed
// "quota-account" events update the quota list in place as they arrive.
function upsertQuota(quotas: AccountQuota[], account: AccountQuota): AccountQuota[] {
  const index = quotas.findIndex(
    (item) => item.provider_id === account.provider_id && item.account_key === account.account_key,
  );
  if (index >= 0) {
    // Mirror the backend's "keep last-known-good" rule while streaming: a probe
    // that came back transiently blank (no models, not exhausted, not auth-failed)
    // must not wipe the numbers we're already showing for this account.
    const old = quotas[index];
    const blank =
      account.models.length === 0 && !account.is_forbidden && account.status_message !== "auth_failed";
    const next = quotas.slice();
    next[index] = blank && old.models.length > 0 ? old : account;
    return next;
  }
  return [...quotas, account];
}

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
  // Non-blocking floating toast during a user-triggered quota refresh: counts
  // accounts as they stream in. Null = hidden (incl. the silent background poll).
  const [quotaToast, setQuotaToast] = useState<{ loaded: number; total: number; current?: string } | null>(null);
  const lowQuotaNotified = useRef<Set<string>>(new Set());
  const proxyDraftSeeded = useRef(false);
  // 防重入:手动刷新 + 5 分钟后台轮询可能并发,导致重复注册 quota-account 监听器、
  // 每个事件被多次处理、toast 计数翻倍。同一时刻只允许一次配额刷新在跑。
  const quotaRefreshInFlight = useRef(false);
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

  // 智能调度切换了账号池状态(选号/待命/还原) → 重拉状态刷新界面。
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | undefined;
    void listen("scheduler-changed", () => {
      void refreshState();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Background poll: refresh the management snapshot (each account's health ✓/✗
  // dots + the Logs "Requests" tab) every 5 minutes. This is UI-only — draining
  // the proxy's usage queue is owned by the backend usage collector (a separate
  // ~1.5s loop that persists to SQLite), and the dashboard updates live off its
  // "usage-updated" events. So a slow cadence here loses no data; it just makes
  // the health dots / request log slightly staler, which avoids the constant
  // whole-app re-render. Only in the real Tauri app; throws (caught) when the
  // proxy is unreachable, which also covers an orphaned/externally-started proxy.
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    if (isMenuBarView) return;
    const interval = window.setInterval(() => {
      invoke<AppState>("refresh_management_state")
        .then(setAppState)
        .catch((err) => console.warn("[useAppState] refresh_management_state failed:", err));
    }, 5 * 60 * 1000);
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

  async function refreshQuotas(manual = false) {
    // 已有刷新在跑就跳过本次,避免并发注册重复监听器/重复处理事件。
    if (quotaRefreshInFlight.current) return;
    quotaRefreshInFlight.current = true;
    setIsQuotaBusy(true);
    // Floating toast only for user-triggered refreshes; the background poll
    // passes no `manual`, so it stays silent.
    if (manual) {
      // 确定性进度条的分母=代理已知的账号(auth 文件)数。对文件型账号(codex/claude/
      // copilot/antigravity/kiro 等)精确;glm/trae/cursor 等少数源可能略有出入,故下方
      // flush 用 max(total, loaded) 钳位,保证 X 永不超过 N、进度条视觉始终正常。
      const total = appState?.management?.auth_files?.length ?? appState?.auth_files?.length ?? 0;
      setQuotaToast({ loaded: 0, total, current: undefined });
    }
    // Stream accounts in as the backend fetches them ("quota-account" per
    // account), so they appear one-by-one and one unreachable account never
    // blocks the rest. Register before the invoke so no early account is missed;
    // the invoke still returns the full list at the end as a sync.
    let unlisten: (() => void) | undefined;
    if ("__TAURI_INTERNALS__" in window) {
      // Coalesce the per-account stream into at most one state update per frame.
      // 80 accounts would otherwise fire 80 setAppState → 80 full-tree re-renders.
      let pending: AccountQuota[] = [];
      let rafId: number | null = null;
      const flush = () => {
        rafId = null;
        if (pending.length === 0) return;
        const batch = pending;
        pending = [];
        setAppState((prev) => {
          if (!prev) return prev;
          let quotas = prev.quotas;
          for (const account of batch) quotas = upsertQuota(quotas, account);
          return { ...prev, quotas };
        });
        setQuotaToast((toast) => {
          if (!toast) return toast;
          const loaded = toast.loaded + batch.length;
          // total 取 max:万一实际流入账号超过 auth_files 估计,分母同步增长,X 永不超过 N。
          return { loaded, total: Math.max(toast.total, loaded), current: batch[batch.length - 1].account_label };
        });
      };
      const tauriUnlisten = await listen<AccountQuota>("quota-account", (event) => {
        pending.push(event.payload);
        if (rafId === null) rafId = window.requestAnimationFrame(flush);
      });
      unlisten = () => {
        if (rafId !== null) window.cancelAnimationFrame(rafId);
        tauriUnlisten();
      };
    }
    try {
      const state = await invoke<AppState>("refresh_quotas");
      setAppState(state);
      void notifyLowQuotas(state);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      unlisten?.();
      if (manual) setQuotaToast(null);
      setIsQuotaBusy(false);
      quotaRefreshInFlight.current = false;
    }
  }

  /// Fire a desktop notification when a model's remaining quota drops to/below
  /// 10%, once per account+model until it recovers. Best-effort (Tauri only).
  async function notifyLowQuotas(state: AppState) {
    if (!("__TAURI_INTERNALS__" in window)) return;
    if (!state.settings.notifications_enabled) return; // respect the Settings toggle
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
    try {
      const response = await invoke<OAuthUrlResponse>("start_management_oauth", {
        endpoint,
        projectId: projectId && projectId.trim().length > 0 ? projectId.trim() : null,
        isWebui,
      });
      setError(response.error);
      return response;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
    }
  }

  async function pollOAuth(token: string) {
    try {
      const response = await invoke<OAuthStatusResponse>("poll_management_oauth", { token });
      setError(response.error);
      if (response.status === "ok" || response.status === "success" || response.status === "completed") {
        const nextState = await invoke<AppState>("refresh_management_state");
        setAppState(nextState);
      }
      return response;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
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
    quotaToast,
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
