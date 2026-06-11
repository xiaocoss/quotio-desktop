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
import { FallbackScreen } from "./sections/FallbackScreen";
import { LogsScreen } from "./sections/LogsScreen";
import { ProvidersScreen } from "./sections/ProvidersScreen";
import { QuotaScreen } from "./sections/QuotaScreen";
import { SettingsScreen } from "./sections/SettingsScreen";

type AppShellProps = {
  appState: AppState;
  isSaving: boolean;
  isProxyBusy: boolean;
  isManagementBusy: boolean;
  isQuotaBusy: boolean;
  quotaToast: { loaded: number } | null;
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
  onSaveSettings: (settings: AppSettings) => void;
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
  { id: "fallback", label: "Fallback", symbol: "⑂", experimental: true },
  { id: "agents", label: "Agents", symbol: "▣" },
  { id: "api_keys", label: "API Keys", symbol: "⚿" },
  { id: "logs", label: "Logs", symbol: "☰" },
  { id: "settings", label: "Settings", symbol: "⚙" },
  { id: "about", label: "About", symbol: "ⓘ" },
];

async function windowAction(action: "close" | "minimize" | "maximize") {
  if (!("__TAURI_INTERNALS__" in window)) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const win = getCurrentWindow();
  if (action === "close") {
    // Fully quit the app. Just closing the main window would leave the process
    // alive in the tray (the hidden menu-bar window + tray icon keep it running).
    const { invoke } = await import("../lib/tauri");
    await invoke("quit_app");
  } else if (action === "minimize") {
    // Hide to the tray (remove from the taskbar) instead of a taskbar minimize,
    // then pop the always-on-top quota panel. The tray icon brings the app back.
    await win.hide();
    const { invoke } = await import("../lib/tauri");
    void invoke("show_menubar");
  } else await win.toggleMaximize();
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

export function AppShell(props: AppShellProps) {
  const [activeSection, setActiveSection] = useState<AppSection>("dashboard");
  const [closeDialog, setCloseDialog] = useState(false);
  const [rememberClose, setRememberClose] = useState(false);
  const [closing, setClosing] = useState(false);

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
        void windowAction("close");
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
    if (saved === "tray") return windowAction("minimize");
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
      void windowAction("minimize");
    } else {
      doQuit();
    }
  }

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="sidebar-titlebar">
          <div className="window-controls">
            <button type="button" className="win-dot win-dot--close" onClick={() => void requestClose()} aria-label="关闭" title="关闭">
              <span className="win-dot-glyph">✕</span>
            </button>
            <button type="button" className="win-dot win-dot--min" onClick={() => windowAction("minimize")} aria-label="最小化" title="最小化">
              <span className="win-dot-glyph">−</span>
            </button>
            <button type="button" className="win-dot win-dot--max" onClick={() => windowAction("maximize")} aria-label="最大化" title="最大化">
              <span className="win-dot-glyph">+</span>
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
              <span className="nav-symbol" aria-hidden="true">{item.symbol}</span>
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
        {renderSection(activeSection, props)}
      </section>

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

      {props.isRefreshing || props.isManagementBusy ? (
        <div className="closing-overlay">
          <div className="loading-card">
            <div className="boot-bar" aria-hidden="true">
              <span />
            </div>
            <p>正在刷新…</p>
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
          <div className="boot-bar" aria-hidden="true">
            <span />
          </div>
          <p>正在加载额度… {props.quotaToast.loaded}</p>
        </div>
      ) : null}
    </main>
  );
}

function renderSection(section: AppSection, props: AppShellProps) {
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
    case "fallback":
      return (
        <FallbackScreen
          appState={props.appState}
          isBusy={props.localAction === "update_fallback_configuration" || props.fallbackAction !== null}
          fallbackAction={props.fallbackAction}
          availableModels={props.availableModels.length > 0 ? props.availableModels : props.appState.fallback_runtime.available_models}
          onUpdateFallback={props.onRunFallbackConfigAction}
          onDiscoverModels={props.onDiscoverAvailableModels}
          onRefreshRouteState={props.onRefreshFallbackRouteState}
        />
      );
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
          onRunManagementStateAction={props.onRunManagementStateAction}
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
      return <AboutScreen appState={props.appState} />;
    case "dashboard":
    default:
      return <DashboardScreen appState={props.appState} />;
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

function AboutScreen({ appState }: { appState: AppState }) {
  const t = useT();
  return (
    <section className="dashboard-content">
      <header className="page-topbar" data-tauri-drag-region>
        <h1>{t("nav.about")}</h1>
      </header>
      <article className="panel about-panel">
        <div className="about-brand">
          <div className="about-mark">Q</div>
          <div>
            <strong>Quotio</strong>
            <span>v0.1.0</span>
          </div>
        </div>
        <dl className="detail-list compact-details">
          <div>
            <dt>平台</dt>
            <dd>
              {appState.platform.os} / {appState.platform.arch}
            </dd>
          </div>
          <div>
            <dt>运行模式</dt>
            <dd>{appState.settings.operating_mode}</dd>
          </div>
          <div>
            <dt>代理地址</dt>
            <dd>{appState.proxy.endpoint}</dd>
          </div>
          <div>
            <dt>配置目录</dt>
            <dd>{appState.config_root}</dd>
          </div>
        </dl>
        <p className="about-copy">Quotio · 多服务商 AI 代理与额度管理工具。</p>
      </article>
    </section>
  );
}