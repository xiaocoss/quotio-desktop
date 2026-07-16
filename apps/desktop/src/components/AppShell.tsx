import { useEffect, useRef, useState } from "react";
import { useT } from "../i18n";
import type {
  AgentBackupFile,
  AgentConfigurationRequest,
  AgentConfigurationResult,
  AppSection,
  AppSettings,
  AppState,
  AvailableModel,
  CredentialStatus,
  FallbackConfigAction,
  OAuthStatusResponse,
  OAuthUrlResponse,
  ProxyCommand,
  ProxyState,
  ProxyStatusKind,
  SavedAgentConfiguration,
} from "../types";
import { DashboardScreen } from "./dashboard/DashboardScreen";
import { AgentsScreen } from "./sections/AgentsScreen";
import { ApiKeysScreen } from "./sections/ApiKeysScreen";
import { TwoFactorAuthScreen } from "./sections/TwoFactorAuthScreen";
import { LogsScreen } from "./sections/LogsScreen";
import { ProvidersScreen } from "./sections/ProvidersScreen";
import { QuotaScreen } from "./sections/QuotaScreen";
import { SettingsScreen } from "./sections/SettingsScreen";
import { ProxyInstabilityBanner } from "./ProxyInstabilityBanner";
import { UpdateDialog } from "./UpdateDialog";
import { useUpdater } from "../state/useUpdater";

type AppShellProps = {
  appState: AppState;
  isSaving: boolean;
  isProxyBusy: boolean;
  isManagementBusy: boolean;
  isQuotaBusy: boolean;
  quotaToast: { loaded: number; total: number; current?: string } | null;
  isRefreshing: boolean;
  proxyAction: string | null;
  managementAction: string | null;
  localAction: string | null;
  agentAction: string | null;
  fallbackAction: string | null;
  platformAction: string | null;
  agentResult: AgentConfigurationResult | null;
  agentBackups: Record<string, AgentBackupFile[]>;
  agentConfigurations: Record<string, SavedAgentConfiguration>;
  availableModels: AvailableModel[];
  credentialStatus: CredentialStatus | null;
  proxyUrlDraft: string;
  onProxyUrlDraftChange: (value: string) => void;
  onRefreshState: () => void;
  onRefreshQuotas: () => void;
  onToggleNotifications: () => void;
  onRunProxyAction: (command: ProxyCommand) => void;
  onSaveSettings: (
    settings: AppSettings,
    options?: { allowClearCodexProfiles?: boolean },
  ) => void;
  onRunManagementStateAction: (command: string, args?: Record<string, unknown>) => void;
  onRunFallbackConfigAction: (action: FallbackConfigAction) => void;
  onStartOAuth: (endpoint: string, projectId: string | null, isWebui?: boolean) => Promise<OAuthUrlResponse | null>;
  onPollOAuth: (token: string) => Promise<OAuthStatusResponse | null>;
  onRefreshProxyUrlDraft: () => void;
  onRefreshAgentStatuses: () => void;
  onReadAgentConfiguration: (agentId: string) => Promise<SavedAgentConfiguration | null>;
  onConfigureAgent: (request: AgentConfigurationRequest) => Promise<AgentConfigurationResult | null>;
  onListAgentBackups: (agentId: string) => Promise<AgentBackupFile[]>;
  onRestoreAgentBackup: (agentId: string, backupPath: string) => Promise<AgentConfigurationResult | null>;
  onResetAgentConfiguration: (agentId: string) => Promise<AgentConfigurationResult | null>;
  onDiscoverAvailableModels: () => Promise<AvailableModel[]>;
  onRefreshFallbackRouteState: () => void;
  onRefreshCredentialStatus: () => Promise<CredentialStatus | null>;
  onClearRemoteManagementKey: () => void;
  onOpenConfigRoot: () => void;
  onSetLaunchAtLogin: (enabled: boolean) => void;
  onRequestNotificationPermission: () => Promise<boolean>;
  onSendTestNotification: () => void;
};

type NavItem = {
  id: AppSection;
  label: string;
  symbol: string;
  experimental?: boolean;
};

const navItems: NavItem[] = [
  { id: "dashboard", label: "Dashboard", symbol: "⌂" },
  { id: "quota", label: "Quota", symbol: "▥" },
  { id: "providers", label: "Providers", symbol: "◎" },
  { id: "two_factor", label: "2FA", symbol: "⚿" },
  { id: "agents", label: "Agents", symbol: "▣" },
  { id: "api_keys", label: "API Keys", symbol: "⚿" },
  { id: "logs", label: "Logs", symbol: "☰" },
  { id: "settings", label: "Settings", symbol: "⚙" },
  { id: "about", label: "About", symbol: "ⓘ" },
];

/** 导航项 id → 侧栏线性图标 symbol(素材 public/nav-icons.svg)。 */
const NAV_ICON: Record<string, string> = {
  dashboard: "nav-dashboard",
  quota: "nav-quota",
  providers: "nav-providers",
  two_factor: "nav-2fa",
  agents: "nav-agents",
  api_keys: "nav-apikeys",
  logs: "nav-logs",
  settings: "nav-settings",
  about: "nav-about",
};

async function toggleMaximize() {
  if (!("__TAURI_INTERNALS__" in window)) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().toggleMaximize();
}

// Label for the global overlay shown while a proxy lifecycle action is in
// flight, so the user gets feedback instead of clicking the button repeatedly.
function proxyActionLabel(action: string | null): string | null {
  switch (action) {
    case "start_proxy":
      return "正在启动代理…";
    case "stop_proxy":
      return "正在停止代理…";
    case "restart_proxy":
      return "正在重启代理…";
    case "download_proxy_binary":
      return "正在下载代理…";
    default:
      return null;
  }
}

function managementActionLabel(action: string | null): string | null {
  switch (action) {
    case "refresh_management_state":
      return "正在刷新管理状态…";
    case "get_management_proxy_url":
      return "正在读取代理 URL…";
    case "set_management_proxy_url":
      return "正在写入代理 URL…";
    case "clear_management_proxy_url":
      return "正在清除代理 URL…";
    case "set_management_debug":
      return "正在切换调试模式…";
    case "set_management_request_log":
      return "正在切换请求日志…";
    case "set_management_logging_to_file":
      return "正在切换文件日志…";
    case "set_management_routing_strategy":
      return "正在设置路由策略…";
    case "set_management_request_retry":
      return "正在设置重试次数…";
    case "clear_management_logs":
      return "正在清除日志…";
    case "clear_request_logs":
      return "正在清空请求日志…";
    case "delete_management_auth_file":
      return "正在删除账号…";
    case "set_management_auth_file_disabled":
      return "正在切换账号状态…";
    case "add_api_key":
      return "正在添加 API Key…";
    case "remove_api_key":
      return "正在删除 API Key…";
    case "update_api_key":
      return "正在更新 API Key…";
    default:
      return action ? `正在执行 ${action}…` : null;
  }
}

export function AppShell(props: AppShellProps) {
  const [activeSection, setActiveSection] = useState<AppSection>("dashboard");
  // One-shot「聚焦账号」:从额度卡片点图表/列表跳转时带上该账号邮箱,目标页挂载时读取并
  // 立即消费清空,避免下次手动进入该页时残留旧筛选。
  const [focusAccount, setFocusAccount] = useState<string | null>(null);
  const [closeDialog, setCloseDialog] = useState(false);
  const [rememberClose, setRememberClose] = useState(false);
  const [minimizeDialog, setMinimizeDialog] = useState(false);
  const [rememberMinimize, setRememberMinimize] = useState(false);
  const [closing, setClosing] = useState(false);
  const updater = useUpdater();

  // First time the user opens the Quota tab this session, kick off a fresh fetch
  // (with the loading card) so they get current data on demand — not just the
  // background snapshot from startup.
  const quotaVisited = useRef(false);
  useEffect(() => {
    if (activeSection === "quota" && !quotaVisited.current) {
      quotaVisited.current = true;
      props.onRefreshQuotas();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeSection]);
  const t = useT();

  // Quitting runs a brief proxy/resource cleanup that can freeze the window, so
  // paint a "closing" overlay first, then exit on the next frame so it shows.
  function doQuit() {
    setClosing(true);
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        void (async () => {
          if (!("__TAURI_INTERNALS__" in window)) return;
          const { invoke } = await import("../lib/tauri");
          await invoke("quit_app");
        })();
      }),
    );
  }

  // Closing prompts whether to quit or hide to the tray, unless a remembered
  // choice exists (saved after ticking "记住我的选择").
  async function requestClose() {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let saved: string | null = null;
    try {
      saved = localStorage.getItem("quotio.closeAction");
    } catch {
      /* storage unavailable */
    }
    if (saved === "quit") return doQuit();
    if (saved === "tray") return minimizeToTray();
    setCloseDialog(true);
  }

  function chooseClose(choice: "quit" | "tray") {
    if (rememberClose) {
      try {
        localStorage.setItem("quotio.closeAction", choice);
      } catch {
        /* storage unavailable */
      }
    }
    setCloseDialog(false);
    if (choice === "tray") {
      void minimizeToTray();
    } else {
      doQuit();
    }
  }

  async function minimizeToTray() {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().hide();
    const { invoke } = await import("../lib/tauri");
    void invoke("show_menubar");
  }

  async function minimizeToTaskbar() {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().minimize();
  }

  async function requestMinimize() {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let saved: string | null = null;
    try {
      saved = localStorage.getItem("quotio.minimizeAction");
    } catch {
      /* storage unavailable */
    }
    if (saved === "tray") return minimizeToTray();
    if (saved === "taskbar") return minimizeToTaskbar();
    setMinimizeDialog(true);
  }

  function chooseMinimize(choice: "tray" | "taskbar") {
    if (rememberMinimize) {
      try {
        localStorage.setItem("quotio.minimizeAction", choice);
      } catch {
        /* storage unavailable */
      }
    }
    setMinimizeDialog(false);
    if (choice === "tray") {
      void minimizeToTray();
    } else {
      void minimizeToTaskbar();
    }
  }

  return (
    <main className="app-shell app-shell--v2">
      <aside className="sidebar">
        <div className="sidebar-titlebar">
          <div className="window-controls">
            <button type="button" className="win-dot win-dot--close" onClick={() => void requestClose()} aria-label="关闭" title="关闭">
              <svg className="win-dot-icon" viewBox="0 0 12 12"><path d="M3.172 3.172a.5.5 0 0 1 .707 0L6 5.293l2.121-2.121a.5.5 0 1 1 .707.707L6.707 6l2.121 2.121a.5.5 0 0 1-.707.707L6 6.707 3.879 8.828a.5.5 0 1 1-.707-.707L5.293 6 3.172 3.879a.5.5 0 0 1 0-.707Z" fill="currentColor"/></svg>
            </button>
            <button type="button" className="win-dot win-dot--min" onClick={() => void requestMinimize()} aria-label="最小化" title="最小化">
              <svg className="win-dot-icon" viewBox="0 0 12 12"><rect x="2" y="5.25" width="8" height="1.5" rx=".75" fill="currentColor"/></svg>
            </button>
            <button type="button" className="win-dot win-dot--max" onClick={() => void toggleMaximize()} aria-label="最大化" title="最大化">
              <svg className="win-dot-icon" viewBox="0 0 12 12"><path d="M2 4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V4Zm2-.75a.75.75 0 0 0-.75.75v4c0 .414.336.75.75.75h4a.75.75 0 0 0 .75-.75V4a.75.75 0 0 0-.75-.75H4Z" fill="currentColor"/></svg>
            </button>
          </div>
          <div className="titlebar-drag" data-tauri-drag-region />
        </div>

        <nav className="nav-list" aria-label="Primary navigation">
          {navItems.map((item) => (
            <button
              className={activeSection === item.id ? "active" : ""}
              key={item.id}
              type="button"
              onClick={() => setActiveSection(item.id)}
            >
              <span className="nav-symbol" aria-hidden="true">
                <svg width="20" height="20">
                  <use href={`/nav-icons.svg#${NAV_ICON[item.id] ?? "nav-dashboard"}`} />
                </svg>
              </span>
              <span className="nav-label">{t(`nav.${item.id}`, item.label)}</span>
              {item.experimental ? <span className="nav-badge">{t("common.experimental")}</span> : null}
            </button>
          ))}
        </nav>

        <ProxyStatusCard
          proxy={props.appState.proxy}
          isProxyBusy={props.isProxyBusy}
          proxyAction={props.proxyAction}
          onRunProxyAction={props.onRunProxyAction}
        />
      </aside>

      <section className={activeSection === "dashboard" ? "content content--dashboard" : "content"}>
        {renderSection(activeSection, props, updater, setActiveSection, {
          account: focusAccount,
          goToAccount: (section, account) => {
            setFocusAccount(account);
            setActiveSection(section);
          },
          consume: () => setFocusAccount(null),
        })}
      </section>

      <ProxyInstabilityBanner appState={props.appState} />

      <UpdateDialog
        status={updater.status}
        version={updater.version}
        notes={updater.notes}
        percent={updater.percent}
        error={updater.error}
        onInstall={updater.install}
        onRetry={updater.retry}
        onDismiss={updater.dismiss}
      />

      {closeDialog ? (
        <div className="modal-overlay" onClick={() => setCloseDialog(false)}>
          <div className="close-dialog" onClick={(event) => event.stopPropagation()}>
            <strong className="close-dialog-title">{t("close.title", "关闭 Quotio")}</strong>
            <p className="close-dialog-desc">{t("close.desc", "退出程序,还是最小化到托盘继续后台运行?")}</p>
            <label className="close-dialog-remember">
              <input type="checkbox" checked={rememberClose} onChange={(event) => setRememberClose(event.target.checked)} />
              <span>{t("close.remember", "记住我的选择")}</span>
            </label>
            <div className="close-dialog-actions">
              <button type="button" className="ghost-action" onClick={() => setCloseDialog(false)}>
                {t("close.cancel", "取消")}
              </button>
              <button type="button" className="secondary-action" onClick={() => chooseClose("tray")}>
                {t("close.tray", "最小化到托盘")}
              </button>
              <button type="button" className="danger-action" onClick={() => chooseClose("quit")}>
                {t("close.quit", "退出程序")}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {minimizeDialog ? (
        <div className="modal-overlay" onClick={() => setMinimizeDialog(false)}>
          <div className="close-dialog" onClick={(event) => event.stopPropagation()}>
            <strong className="close-dialog-title">最小化 Quotio</strong>
            <p className="close-dialog-desc">隐藏到托盘并弹出悬浮窗，还是最小化到任务栏？</p>
            <label className="close-dialog-remember">
              <input type="checkbox" checked={rememberMinimize} onChange={(event) => setRememberMinimize(event.target.checked)} />
              <span>记住我的选择</span>
            </label>
            <div className="close-dialog-actions">
              <button type="button" className="ghost-action" onClick={() => setMinimizeDialog(false)}>
                取消
              </button>
              <button type="button" className="secondary-action" onClick={() => chooseMinimize("taskbar")}>
                最小化到任务栏
              </button>
              <button type="button" className="primary-action" onClick={() => chooseMinimize("tray")}>
                隐藏到托盘
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {props.isRefreshing || props.isManagementBusy ? (
        <div className="closing-overlay">
          <div className="loading-card">
            <div className="boot-bar" aria-hidden="true">
              <span />
            </div>
            <p>{managementActionLabel(props.managementAction) ?? "正在刷新…"}</p>
          </div>
        </div>
      ) : null}

      {proxyActionLabel(props.proxyAction) ? (
        <div className="closing-overlay">
          <div className="loading-card">
            <div className="boot-bar" aria-hidden="true">
              <span />
            </div>
            <p>{proxyActionLabel(props.proxyAction)}</p>
          </div>
        </div>
      ) : null}

      {closing ? (
        <div className="closing-overlay">
          <div className="loading-card">
            <div className="boot-bar" aria-hidden="true">
            <span />
          </div>
            <p>{t("close.closing", "正在关闭…")}</p>
          </div>
        </div>
      ) : null}

      {props.quotaToast ? (
        <div className="quota-toast">
          {props.quotaToast.total > 0 ? (
            <div className="update-progress" aria-hidden="true">
              <span
                style={{
                  width: `${Math.min(100, Math.round((props.quotaToast.loaded / props.quotaToast.total) * 100))}%`,
                }}
              />
            </div>
          ) : (
            <div className="boot-bar" aria-hidden="true">
              <span />
            </div>
          )}
          <p>
            正在加载额度… {Math.min(props.quotaToast.loaded, props.quotaToast.total || props.quotaToast.loaded)}
            {props.quotaToast.total > 0 ? ` / ${props.quotaToast.total}` : ""}
            {props.quotaToast.current ? ` — ${props.quotaToast.current}` : ""}
          </p>
        </div>
      ) : null}
    </main>
  );
}

type NavFocus = {
  account: string | null;
  goToAccount: (section: AppSection, account: string) => void;
  consume: () => void;
};

function renderSection(section: AppSection, props: AppShellProps, updater: ReturnType<typeof useUpdater>, navigate: (section: AppSection) => void, focus: NavFocus) {
  switch (section) {
    case "providers":
      return (
        <ProvidersScreen
          appState={props.appState}
          isManagementBusy={props.isManagementBusy}
          managementAction={props.managementAction}
          onRefreshManagement={() => props.onRunManagementStateAction("refresh_management_state")}
          onRefreshQuotas={props.onRefreshQuotas}
          onRunManagementStateAction={props.onRunManagementStateAction}
          onStartOAuth={props.onStartOAuth}
          onPollOAuth={props.onPollOAuth}
        />
      );
    case "quota":
      return (
        <QuotaScreen
          appState={props.appState}
          isManagementBusy={props.isManagementBusy}
          isQuotaBusy={props.isQuotaBusy}
          managementAction={props.managementAction}
          onRefreshManagement={() => props.onRunManagementStateAction("refresh_management_state")}
          onRefreshQuotas={props.onRefreshQuotas}
          onRunManagementStateAction={props.onRunManagementStateAction}
          onSaveSettings={props.onSaveSettings}
          onAddAccount={() => navigate("providers")}
          onViewAccountChart={(account) => focus.goToAccount("dashboard", account)}
          onViewAccountLogs={(account) => focus.goToAccount("logs", account)}
        />
      );
    case "agents":
      return (
        <AgentsScreen
          appState={props.appState}
          isBusy={props.agentAction !== null}
          action={props.agentAction}
          agentResult={props.agentResult}
          agentBackups={props.agentBackups}
          agentConfigurations={props.agentConfigurations}
          availableModels={props.availableModels.length > 0 ? props.availableModels : props.appState.fallback_runtime.available_models}
          onRefreshAgents={props.onRefreshAgentStatuses}
          onReadConfiguration={props.onReadAgentConfiguration}
          onConfigureAgent={props.onConfigureAgent}
          onListBackups={props.onListAgentBackups}
          onRestoreBackup={props.onRestoreAgentBackup}
          onResetConfiguration={props.onResetAgentConfiguration}
          onSaveSettings={props.onSaveSettings}
        />
      );
    case "two_factor":
      return <TwoFactorAuthScreen />;
    case "api_keys":
      return (
        <ApiKeysScreen
          appState={props.appState}
          isManagementBusy={props.isManagementBusy}
          managementAction={props.managementAction}
          onRefreshManagement={() => props.onRunManagementStateAction("refresh_management_state")}
          onRunManagementStateAction={props.onRunManagementStateAction}
        />
      );
    case "logs":
      return (
        <LogsScreen
          appState={props.appState}
          isManagementBusy={props.isManagementBusy}
          managementAction={props.managementAction}
          onRefreshManagement={() => props.onRunManagementStateAction("refresh_management_state")}
          onClearLogs={() => props.onRunManagementStateAction("clear_management_logs")}
          onClearRequests={() => props.onRunManagementStateAction("clear_request_logs")}
          onRunManagementStateAction={props.onRunManagementStateAction}
          initialAccount={focus.account}
          onFocusConsumed={focus.consume}
        />
      );
    case "settings":
      return (
        <SettingsScreen
          appState={props.appState}
          isSaving={props.isSaving}
          isManagementBusy={props.isManagementBusy}
          managementAction={props.managementAction}
          platformAction={props.platformAction}
          credentialStatus={props.credentialStatus ?? props.appState.credentials}
          proxyUrlDraft={props.proxyUrlDraft}
          onProxyUrlDraftChange={props.onProxyUrlDraftChange}
          onRefreshProxyUrlDraft={props.onRefreshProxyUrlDraft}
          onSaveSettings={props.onSaveSettings}
          onRunManagementStateAction={props.onRunManagementStateAction}
          onRefreshCredentialStatus={props.onRefreshCredentialStatus}
          onClearRemoteManagementKey={props.onClearRemoteManagementKey}
          onOpenConfigRoot={props.onOpenConfigRoot}
          onSetLaunchAtLogin={props.onSetLaunchAtLogin}
          onRequestNotificationPermission={props.onRequestNotificationPermission}
          onSendTestNotification={props.onSendTestNotification}
        />
      );
    case "about":
      return (
        <AboutScreen
          appState={props.appState}
          onCheckUpdate={() => void updater.check(true)}
          checking={updater.status === "checking"}
        />
      );
    case "dashboard":
    default:
      return <DashboardScreen initialAccount={focus.account} onFocusConsumed={focus.consume} />;
  }
}

const PROXY_STATUS_LABELS: Record<ProxyStatusKind, string> = {
  stopped: "Stopped",
  starting: "Starting",
  running: "Running",
  stopping: "Stopping",
  missing_binary: "No binary",
  crashed: "Crashed",
  error: "Error",
};

function proxyTone(status: ProxyStatusKind): "good" | "warn" | "bad" {
  if (status === "running") return "good";
  if (status === "starting" || status === "stopping" || status === "stopped") return "warn";
  return "bad";
}

type ProxyStatusCardProps = {
  proxy: ProxyState;
  isProxyBusy: boolean;
  proxyAction: string | null;
  onRunProxyAction: (command: ProxyCommand) => void;
};

function ProxyStatusCard({ proxy, isProxyBusy, proxyAction, onRunProxyAction }: ProxyStatusCardProps) {
  const running = proxy.status === "running";
  const isMissing = proxy.status === "missing_binary";
  const downloading = proxyAction === "download_proxy_binary";
  const host = proxy.endpoint.replace(/^https?:\/\//, "");
  const port = host.includes(":") ? `:${host.split(":").pop()}` : host;
  const action: ProxyCommand = isMissing ? "download_proxy_binary" : running ? "stop_proxy" : "start_proxy";
  const busy = isProxyBusy || proxyAction === "start_proxy" || proxyAction === "stop_proxy" || downloading;
  const tone = proxyTone(proxy.status);
  const t = useT();

  const [progress, setProgress] = useState<number | null>(null);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<number>("proxy-download-progress", (event) => {
          setProgress(event.payload >= 100 ? null : event.payload);
        }),
      )
      .then((fn) => {
        unlisten = fn;
      });
    return () => unlisten?.();
  }, []);

  return (
    <div className="sidebar-footer">
      <button
        className="proxy-card"
        type="button"
        onClick={() => onRunProxyAction(action)}
        disabled={busy}
        title={isMissing ? t("proxy.download") : running ? "停止本地代理" : "启动本地代理"}
      >
        <span className={`proxy-card-icon proxy-card-icon--${tone}`} aria-hidden="true">
          ▣
        </span>
        <span className="proxy-card-text">
          <strong>{t("proxy.title")}</strong>
          <span>
            {isMissing
              ? downloading
                ? `${t("proxy.downloading")}${progress != null ? ` ${progress}%` : ""}`
                : t("proxy.download")
              : `${port} · ${t(`proxy.${proxy.status}`, PROXY_STATUS_LABELS[proxy.status])}`}
          </span>
        </span>
        <span className="proxy-card-chevron" aria-hidden="true">
          ›
        </span>
      </button>
      <div className="proxy-status-bar">
        <span className={`status-dot status-dot--${tone}`} aria-hidden="true" />
        <span>{t(`proxy.${proxy.status}`, PROXY_STATUS_LABELS[proxy.status])}</span>
        <span className="proxy-status-port">{port}</span>
      </div>
    </div>
  );
}

function AboutIcon({ id }: { id: string }) {
  return (
    <svg width="20" height="20" aria-hidden="true">
      <use href={`/about/about-icons.svg#${id}`} />
    </svg>
  );
}

const ABOUT_MODE_LABEL: Record<string, string> = { full: "本地代理", quota_only: "仅监控", remote: "远程代理" };
const ABOUT_STRATEGY_LABEL: Record<string, string> = { full: "本地优先", quota_only: "仅监控", remote: "远程优先" };

function AboutScreen({
  appState,
  onCheckUpdate,
  checking,
}: {
  appState: AppState;
  onCheckUpdate: () => void;
  checking: boolean;
}) {
  const t = useT();
  const [appVersion, setAppVersion] = useState("");
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    void import("@tauri-apps/api/app")
      .then(({ getVersion }) => getVersion())
      .then(setAppVersion)
      .catch(() => {});
  }, []);

  const version = appVersion ? `v${appVersion}` : "—";
  const mode = appState.settings.operating_mode;
  const modeLabel = ABOUT_MODE_LABEL[mode] ?? mode;
  const strategyLabel = ABOUT_STRATEGY_LABEL[mode] ?? mode;
  const proxyRunning = appState.proxy.status === "running";
  const proxyHealthy = proxyRunning && Boolean(appState.proxy.health.ok);

  async function copyConfigRoot() {
    try {
      await navigator.clipboard.writeText(appState.config_root);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch {
      setCopied(false);
    }
  }

  return (
    <section className="dashboard-content dashboard-content--fixed about-redesign">
      <header className="page-topbar" data-tauri-drag-region>
        <h1 data-tauri-drag-region="false">{t("nav.about")}</h1>
        <p className="about-subtitle" data-tauri-drag-region="false">{t("about.pageSubtitle", "Quotio 产品信息与运行环境")}</p>
      </header>

      <div className="about-scroll">
        <article className="panel about-hero">
          <div className="about-mark">Q</div>
          <div className="about-hero-main">
            <strong className="about-name">Quotio</strong>
            <div className="about-version">{version}</div>
            <p className="about-tagline">{t("about.tagline", "多服务商 AI 代理与额度管理工具")}</p>
            <div className="about-pills">
              <span className="about-pill about-pill--blue">{t("about.cap.proxy", "多服务商代理")}</span>
              <span className="about-pill about-pill--green">{t("about.cap.quota", "额度监控")}</span>
              <span className="about-pill about-pill--lav">{t("about.cap.local", "本地管理")}</span>
            </div>
          </div>
          <div className="about-orbit" aria-hidden="true" />
          <aside className="about-status">
            <div className="about-status-title">{t("about.versionStatus", "版本状态")}</div>
            <div className="about-status-ok">
              {checking ? null : <AboutIcon id="check" />}
              {checking ? t("update.checking", "检查中…") : t("about.upToDate", "当前已是最新版本")}
            </div>
            <button type="button" className="about-check-btn" onClick={onCheckUpdate} disabled={checking}>
              {checking ? t("update.checking", "检查中…") : t("update.check", "检查更新")}
            </button>
            <div className="about-status-note">
              {t("about.currentVersion", "当前版本")} {version}
            </div>
          </aside>
        </article>

        <section className="about-cards">
          <article className="panel about-card">
            <div className="about-card-head">
              <span className="about-card-icon blue">
                <AboutIcon id="monitor" />
              </span>
              <h2>{t("about.runtime", "运行环境")}</h2>
            </div>
            <dl className="about-rows">
              <div>
                <dt>{t("about.platform", "平台")}</dt>
                <dd>{appState.platform.os}</dd>
              </div>
              <div>
                <dt>{t("about.arch", "架构")}</dt>
                <dd>{appState.platform.arch}</dd>
              </div>
              <div>
                <dt>{t("about.mode", "运行模式")}</dt>
                <dd>
                  {modeLabel}
                  <span className="about-tag">{mode}</span>
                </dd>
              </div>
            </dl>
          </article>

          <article className="panel about-card">
            <div className="about-card-head">
              <span className="about-card-icon green">
                <AboutIcon id="server" />
              </span>
              <h2>{t("about.localService", "本地服务")}</h2>
              {proxyRunning ? <span className="about-badge">{t("about.running", "运行正常")}</span> : null}
            </div>
            <dl className="about-rows">
              <div>
                <dt>{t("about.endpoint", "端点")}</dt>
                <dd className="link">{appState.proxy.endpoint}</dd>
              </div>
              <div>
                <dt>{t("about.proxyService", "代理服务")}</dt>
                <dd className={proxyHealthy ? "ok" : undefined}>
                  {proxyHealthy ? "healthy" : appState.proxy.status}
                </dd>
              </div>
              <div>
                <dt>{t("about.strategy", "策略")}</dt>
                <dd className="link">{strategyLabel}</dd>
              </div>
            </dl>
          </article>

          <article className="panel about-card">
            <div className="about-card-head">
              <span className="about-card-icon lav">
                <AboutIcon id="folder" />
              </span>
              <h2>{t("about.configData", "配置与数据")}</h2>
            </div>
            <div className="about-config-label">{t("about.configDir", "配置目录")}</div>
            <div className="about-config-row">
              <code>{appState.config_root}</code>
              <button
                type="button"
                className="about-copy"
                onClick={() => void copyConfigRoot()}
                title={t("common.copy", "复制")}
                aria-label={t("common.copy", "复制")}
              >
                <AboutIcon id={copied ? "check" : "copy"} />
              </button>
            </div>
            <p className="about-config-note">{t("about.configLocal", "配置保存在本机")}</p>
          </article>
        </section>

        <section className="panel about-features">
          <article className="about-feature">
            <span className="about-feature-icon blue">
              <AboutIcon id="users" />
            </span>
            <div>
              <strong>{t("about.feat.providers.title", "统一管理服务商")}</strong>
              <p>{t("about.feat.providers.desc", "集中管理多个 AI 服务商的代理配置。")}</p>
            </div>
          </article>
          <article className="about-feature">
            <span className="about-feature-icon green">
              <AboutIcon id="chart" />
            </span>
            <div>
              <strong>{t("about.feat.quota.title", "查看额度与使用")}</strong>
              <p>{t("about.feat.quota.desc", "实时查看各服务商额度与使用情况。")}</p>
            </div>
          </article>
          <article className="about-feature">
            <span className="about-feature-icon lav">
              <AboutIcon id="send" />
            </span>
            <div>
              <strong>{t("about.feat.forward.title", "本地代理转发")}</strong>
              <p>{t("about.feat.forward.desc", "通过本地代理安全转发 API 请求。")}</p>
            </div>
          </article>
        </section>

        <p className="about-foot">Quotio · {t("about.tagline", "多服务商 AI 代理与额度管理工具")}。</p>
      </div>
    </section>
  );
}
