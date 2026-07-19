import { useEffect, useState } from "react";
import type { AppSettings, AppState, ConnectionMode, CredentialStatus, OperatingMode, ThemeMode } from "../../types";
import { Switch } from "../Switch";
import { Select } from "../Select";
import { useT } from "../../i18n";
import { invoke } from "../../lib/tauri";
import { isHideSensitiveEnabled, setHideSensitiveEnabled } from "../../lib/format";
import { TunnelCard, WarmupCard } from "../TunnelCard";
import "./settings.css";
import "./settings-rose.css";

type SettingsScreenProps = {
  appState: AppState;
  isSaving: boolean;
  isManagementBusy: boolean;
  managementAction: string | null;
  platformAction: string | null;
  credentialStatus: CredentialStatus;
  proxyUrlDraft: string;
  onProxyUrlDraftChange: (value: string) => void;
  onRefreshProxyUrlDraft: () => void;
  onSaveSettings: (settings: AppSettings) => void;
  onRunManagementStateAction: (command: string, args?: Record<string, unknown>) => void;
  onRefreshCredentialStatus: () => Promise<CredentialStatus | null>;
  onClearRemoteManagementKey: () => void;
  onOpenConfigRoot: () => void;
  onSetLaunchAtLogin: (enabled: boolean) => void;
  onRequestNotificationPermission: () => Promise<boolean>;
  onSendTestNotification: () => void;
};

type AppMode = "monitor" | "local" | "remote";

const APP_MODES: { id: AppMode; title: string; desc: string; badge?: string; accent: string; operating: OperatingMode; connection: ConnectionMode }[] = [
  { id: "monitor", title: "Monitor Only", desc: "Track quota usage without running a proxy server", badge: "Default", accent: "34C759", operating: "quota_only", connection: "local" },
  { id: "local", title: "Local Proxy", desc: "Run a local proxy server to manage AI requests", accent: "0A84FF", operating: "full", connection: "local" },
  { id: "remote", title: "Remote Proxy", desc: "Connect to a remote CLIProxyAPI server", badge: "Experimental", accent: "AF52DE", operating: "remote", connection: "remote" },
];

// 应用模式图标沿用素材精灵(public/settings/settings-icons.svg),与设计稿一致。
const MODE_ICONS: Record<AppMode, string> = { monitor: "chart", local: "server", remote: "globe" };

const LANGUAGES: { value: string; label: string }[] = [
  { value: "en", label: "English" },
  { value: "zh-CN", label: "简体中文" },
  { value: "zh-TW", label: "繁體中文" },
  { value: "ja", label: "日本語" },
];

// 内联的 SVG 符号图标(素材见 public/settings/settings-icons.svg)。
function Icon({ id }: { id: string }) {
  return (
    <svg className="sr-icon" aria-hidden="true">
      <use href={`/settings/settings-icons.svg#${id}`} />
    </svg>
  );
}

export function SettingsScreen({
  appState,
  isSaving,
  isManagementBusy,
  managementAction,
  platformAction,
  credentialStatus,
  proxyUrlDraft,
  onProxyUrlDraftChange,
  onRefreshProxyUrlDraft,
  onSaveSettings,
  onRunManagementStateAction,
  onRefreshCredentialStatus,
  onClearRemoteManagementKey,
  onOpenConfigRoot,
  onSetLaunchAtLogin,
  onRequestNotificationPermission,
  onSendTestNotification,
}: SettingsScreenProps) {
  const t = useT();
  const appModeKey: Record<string, string> = { monitor: "monitorOnly", local: "localProxy", remote: "remoteProxy" };
  const settings = appState.settings;
  const config = appState.management.config;
  const platform = appState.platform_features;

  const [hideSensitive, setHideSensitive] = useState(isHideSensitiveEnabled());
  const [showRemoteKey, setShowRemoteKey] = useState(false);
  // 窗口「关闭 / 最小化」按钮行为的记忆值(存 localStorage,与 AppShell 的对话框共用同一 key)。
  // 勾了「记住我的选择」后对话框不再弹,这里给一个随时能改 / 重置的入口。"ask" = 未记忆(仍会弹框)。
  const readWinAction = (key: string) => {
    try {
      return localStorage.getItem(key) || "ask";
    } catch {
      return "ask";
    }
  };
  const [closeAction, setCloseAction] = useState(() => readWinAction("quotio.closeAction"));
  const [minimizeAction, setMinimizeAction] = useState(() => readWinAction("quotio.minimizeAction"));
  const setWinAction = (storageKey: string, value: string, setter: (v: string) => void) => {
    setter(value);
    try {
      if (value === "ask") localStorage.removeItem(storageKey);
      else localStorage.setItem(storageKey, value);
    } catch {
      /* storage unavailable */
    }
  };
  const [connDraft, setConnDraft] = useState({
    proxy_host: settings.proxy_host,
    proxy_port: settings.proxy_port,
    remote_endpoint_url: settings.remote_endpoint_url,
    remote_management_key: "" as string,
    allow_remote: settings.allow_remote,
  });

  useEffect(() => {
    setConnDraft({
      proxy_host: settings.proxy_host,
      proxy_port: settings.proxy_port,
      remote_endpoint_url: settings.remote_endpoint_url,
      remote_management_key: "",
      allow_remote: settings.allow_remote,
    });
  }, [settings.proxy_host, settings.proxy_port, settings.remote_endpoint_url, settings.allow_remote]);

  // Whether the connection draft differs from the saved settings. Drives the
  // "unsaved changes" hint, since editing host/port only updates a local draft —
  // the change (and the bottom-left port) take effect only after an explicit Save.
  const connDirty =
    connDraft.proxy_host !== settings.proxy_host ||
    connDraft.proxy_port !== settings.proxy_port ||
    (connDraft.remote_endpoint_url ?? null) !== (settings.remote_endpoint_url ?? null) ||
    connDraft.allow_remote !== settings.allow_remote ||
    connDraft.remote_management_key.trim().length > 0;

  // Advanced text/number fields commit on blur, not per keystroke. Previously
  // each keystroke called onSaveSettings (a disk write + full re-render), which
  // made typing stutter. The draft keeps typing local + snappy.
  const [advDraft, setAdvDraft] = useState({
    force_model: settings.force_model,
    session_affinity_ttl: settings.session_affinity_ttl,
    max_retry_credentials: settings.max_retry_credentials,
    logs_max_total_size_mb: settings.logs_max_total_size_mb,
  });
  useEffect(() => {
    setAdvDraft({
      force_model: settings.force_model,
      session_affinity_ttl: settings.session_affinity_ttl,
      max_retry_credentials: settings.max_retry_credentials,
      logs_max_total_size_mb: settings.logs_max_total_size_mb,
    });
  }, [settings.force_model, settings.session_affinity_ttl, settings.max_retry_credentials, settings.logs_max_total_size_mb]);
  // Draft for the "Request retry" field so it commits on blur, not on every
  // keystroke (the old onChange saved live each keystroke, which set the field
  // busy → disabled → you couldn't actually finish typing a value).
  const [retryDraft, setRetryDraft] = useState(settings.request_retry);
  useEffect(() => {
    setRetryDraft(settings.request_retry);
  }, [settings.request_retry]);

  const activeMode: AppMode =
    settings.connection_mode === "remote" || settings.operating_mode === "remote"
      ? "remote"
      : settings.operating_mode === "quota_only"
        ? "monitor"
        : "local";
  const launchEnabled = platform.launch_at_login_enabled || settings.launch_at_login;

  function applySettings(patch: Partial<AppSettings>) {
    onSaveSettings({ ...settings, ...patch, remote_management_key: null });
  }

  function selectMode(mode: (typeof APP_MODES)[number]) {
    applySettings({ operating_mode: mode.operating, connection_mode: mode.connection });
  }

  async function toggleNotifications() {
    if (!settings.notifications_enabled) {
      const allowed = await onRequestNotificationPermission();
      if (!allowed) return;
      applySettings({ notifications_enabled: true });
      return;
    }
    applySettings({ notifications_enabled: false });
  }

  function toggleHideSensitive() {
    const next = !hideSensitive;
    setHideSensitive(next);
    setHideSensitiveEnabled(next);
  }

  function saveConnection() {
    onSaveSettings({
      ...settings,
      proxy_host: connDraft.proxy_host,
      proxy_port: connDraft.proxy_port,
      remote_endpoint_url: connDraft.remote_endpoint_url,
      allow_remote: connDraft.allow_remote,
      remote_management_key: connDraft.remote_management_key.trim() ? connDraft.remote_management_key : null,
    });
  }

  // 顶栏健康徽标 / 连接卡运行状态 —— 沿用原有 proxy 状态判定。
  const proxyHealthy = appState.proxy.status === "running" && Boolean(appState.proxy.health.ok);
  const proxyAddr = `${settings.proxy_host}:${settings.proxy_port}`;
  const proxyRuntime = proxyHealthy ? "running · healthy" : appState.proxy.status;

  return (
    <section className="section-page settings-redesign">
      <header className="page-topbar" data-tauri-drag-region>
        <div className="sr-header-lead" data-tauri-drag-region="false">
          <h1 className="sr-title">{t("nav.settings")}</h1>
          <p className="sr-subtitle">{t("settings.subtitle", "配置运行模式、代理连接与请求行为")}</p>
        </div>
        <div className="sr-header-actions">
          <button className="sr-btn" type="button" onClick={onOpenConfigRoot} disabled={platformAction === "open_config_root"}>
            <Icon id="folder" />
            {t("settings.openConfigDir")}
          </button>
          <button className="sr-btn" type="button" onClick={() => void invoke("open_logs_dir")}>
            <Icon id="file" />
            {t("settings.openLogsDir")}
          </button>
          <span className={proxyHealthy ? "sr-health" : "sr-health sr-health--down"}>
            <Icon id="check" />
            {proxyHealthy ? `${t("settings.proxyHealthy", "本地代理运行正常")} · ${proxyAddr}` : `${proxyAddr} · ${proxyRuntime}`}
          </span>
        </div>
      </header>

      {/* 应用模式 */}
      <section className="sr-panel sr-mode-panel">
        <h2 className="sr-section-label">{t("settings.appMode")}</h2>
        <div className="sr-mode-grid">
          {APP_MODES.map((mode) => {
            const selected = activeMode === mode.id;
            const modeClass = ["sr-mode-card", mode.id === "local" ? "sr-mode-card--local" : mode.id === "remote" ? "sr-mode-card--remote" : "", selected ? "is-selected" : ""].filter(Boolean).join(" ");
            return (
              <button key={mode.id} type="button" className={modeClass} onClick={() => selectMode(mode)} disabled={isSaving}>
                <span className="sr-mode-icon" aria-hidden="true">
                  <Icon id={MODE_ICONS[mode.id]} />
                </span>
                <span className="sr-mode-text">
                  <span className="sr-mode-title">
                    {t(`settings.${appModeKey[mode.id] ?? "localProxy"}`, mode.title)}
                    {mode.badge ? (
                      <span className={mode.badge === "Experimental" ? "sr-badge sr-badge--warn" : "sr-badge"}>
                        {mode.badge === "Experimental" ? t("common.experimental") : t("common.default")}
                      </span>
                    ) : null}
                  </span>
                  <span className="sr-mode-desc">{t(`settings.${appModeKey[mode.id] ?? "localProxy"}.desc`, mode.desc)}</span>
                </span>
                <span className="sr-radio" aria-hidden="true" />
              </button>
            );
          })}
        </div>
      </section>

      {/* 基础设置 | 代理连接 */}
      <section className="sr-grid sr-grid--primary">
        <article className="sr-panel sr-card sr-base-card">
          <div className="sr-card-head">
            <h2 className="sr-card-title">
              <Icon id="settings" />
              {t("settings.baseSettings", "基础设置")}
            </h2>
          </div>
          <div className="sr-rows">
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.launchAtLogin")}</strong>
                <small>{t("settings.launchAtLoginDesc", "登录系统时自动启动服务")}</small>
              </div>
              <Switch
                on={launchEnabled}
                disabled={isSaving || platformAction === "set_launch_at_login" || !platform.launch_at_login_available}
                onChange={() => onSetLaunchAtLogin(!launchEnabled)}
                label="Launch at login"
              />
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.keepProxyOnExit", "退出时保留代理运行")}</strong>
                <small>{t("settings.keepProxyOnExitDesc", "关闭 Quotio 后代理继续在后台运行,依赖它的 Codex 等客户端不会因断连崩溃;下次启动 Quotio 会自动接管")}</small>
              </div>
              <Switch
                on={settings.keep_proxy_on_exit}
                disabled={isSaving}
                onChange={() => applySettings({ keep_proxy_on_exit: !settings.keep_proxy_on_exit })}
                label="Keep proxy running on exit"
              />
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.closeAction", "点关闭按钮时")}</strong>
                <small>{t("settings.closeActionDesc", "选「每次询问」会重新弹出选择框(即清除已记住的选择)")}</small>
              </div>
              <Select
                value={closeAction}
                options={[
                  { value: "ask", label: t("settings.winActionAsk", "每次询问") },
                  { value: "tray", label: t("settings.closeToTray", "最小化到托盘") },
                  { value: "quit", label: t("settings.closeToQuit", "直接退出") },
                ]}
                onChange={(value) => setWinAction("quotio.closeAction", value, setCloseAction)}
              />
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.minimizeAction", "点最小化按钮时")}</strong>
                <small>{t("settings.minimizeActionDesc", "选「每次询问」会重新弹出选择框(即清除已记住的选择)")}</small>
              </div>
              <Select
                value={minimizeAction}
                options={[
                  { value: "ask", label: t("settings.winActionAsk", "每次询问") },
                  { value: "tray", label: t("settings.minimizeToTray", "隐藏到托盘") },
                  { value: "taskbar", label: t("settings.minimizeToTaskbar", "最小化到任务栏") },
                ]}
                onChange={(value) => setWinAction("quotio.minimizeAction", value, setMinimizeAction)}
              />
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.notifications")}</strong>
                <small>{platform.notifications_available ? t("settings.notifications.desc") : t("settings.notifications.unavailable")}</small>
              </div>
              <div className="sr-row-controls">
                <button className="sr-ghost" type="button" onClick={onSendTestNotification} disabled={platformAction !== null || !settings.notifications_enabled}>
                  {t("common.test")}
                </button>
                <Switch on={settings.notifications_enabled} disabled={isSaving || platformAction !== null} onChange={() => void toggleNotifications()} label="Notifications" />
              </div>
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.language.label")}</strong>
              </div>
              <Select value={settings.language} options={LANGUAGES} onChange={(value) => applySettings({ language: value })} />
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.theme")}</strong>
                <small>{t("settings.theme.desc")}</small>
              </div>
              <Select
                value={settings.theme}
                options={[
                  { value: "system", label: t("theme.system") },
                  { value: "light", label: t("theme.light") },
                  { value: "rose", label: t("theme.rose") },
                  { value: "dark", label: t("theme.dark") },
                ]}
                onChange={(value) => applySettings({ theme: value as ThemeMode })}
              />
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.hideSensitive")}</strong>
                <small>{t("settings.hideSensitive.desc")}</small>
              </div>
              <Switch on={hideSensitive} onChange={toggleHideSensitive} label="Hide Sensitive Information" />
            </div>
          </div>
        </article>

        <article className="sr-panel sr-card sr-connection-card sr-proxy-card">
          <img className="sr-connection-flow" src="/settings/connection-flow.svg" alt="" />
          <div className="sr-card-head">
            <h2 className="sr-card-title">
              <Icon id="link" />
              {t("settings.proxyConnection")}
            </h2>
            {connDirty ? (
              <span className="sr-unsaved">● {t("settings.unsavedChanges")}</span>
            ) : (
              <span className="sr-saved">{t("settings.connectionSaved", "连接已保存")}</span>
            )}
          </div>
          <div className="sr-rows">
            <div className="sr-field-line">
              <span className="sr-field-label">{t("settings.host")}</span>
              <input className="sr-input" value={connDraft.proxy_host} onChange={(event) => setConnDraft({ ...connDraft, proxy_host: event.target.value })} />
              <span className="sr-field-label">{t("settings.port")}</span>
              <input
                className="sr-input sr-input--sm"
                type="number"
                min={1}
                max={65535}
                value={connDraft.proxy_port}
                onChange={(event) => setConnDraft({ ...connDraft, proxy_port: Number(event.target.value) })}
              />
            </div>
            <div className="sr-field-line sr-field-line--single">
              <span className="sr-field-label">{t("settings.remoteEndpoint")}</span>
              <input
                className="sr-input"
                value={connDraft.remote_endpoint_url ?? ""}
                onChange={(event) => setConnDraft({ ...connDraft, remote_endpoint_url: event.target.value.trim() ? event.target.value : null })}
                placeholder="https://example.com/v0/management"
              />
              <span />
            </div>
            <div className="sr-field-line sr-field-line--single sr-field-line--secret">
              <span className="sr-field-label">{t("settings.remoteKey")}</span>
              <span className="sr-secret-input">
                <input
                  className="sr-input"
                  type={showRemoteKey ? "text" : "password"}
                  value={connDraft.remote_management_key}
                  onChange={(event) => setConnDraft({ ...connDraft, remote_management_key: event.target.value })}
                  placeholder={credentialStatus.remote_management_key_masked ?? "保存后迁入安全存储"}
                />
                <button
                  className="sr-secret-toggle"
                  type="button"
                  onClick={() => setShowRemoteKey((visible) => !visible)}
                  aria-label={showRemoteKey ? t("common.hide", "隐藏") : t("common.show", "显示")}
                >
                  <Icon id="eye" />
                </button>
              </span>
              <span className="sr-secret-hint">{t("settings.secureStorageHint", "保存后注入安全存储")}</span>
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.allowRemote")}</strong>
                <small>{t("settings.allowRemoteDesc")}</small>
              </div>
              <Switch on={connDraft.allow_remote} disabled={isSaving} onChange={() => setConnDraft({ ...connDraft, allow_remote: !connDraft.allow_remote })} label="Allow remote" />
            </div>
          </div>
          <div className="sr-connection-actions">
            <span className="sr-runtime">
              <i className="sr-dot" />
              {t("settings.runtimeStatus", "运行状态")}
              <strong>{proxyRuntime}</strong>
            </span>
            <div className="sr-row-controls">
              <button className="sr-mini-btn sr-mini-btn--danger" type="button" onClick={onClearRemoteManagementKey} disabled={platformAction !== null}>
                {t("settings.clearKey")}
              </button>
              <button className="sr-mini-btn" type="button" onClick={() => void onRefreshCredentialStatus()} disabled={platformAction !== null}>
                {t("settings.refreshCreds")}
              </button>
              <button className={connDirty ? "sr-mini-btn sr-mini-btn--primary" : "sr-mini-btn"} type="button" onClick={saveConnection} disabled={isSaving}>
                {isSaving ? t("common.saving") : t("settings.saveConnection")}
              </button>
            </div>
          </div>
        </article>
      </section>

      {/* 管理 API | 高级设置 */}
      <section className="sr-grid sr-grid--secondary">
        <article className="sr-panel sr-card sr-management-card">
          <div className="sr-card-head">
            <h2 className="sr-card-title">
              <Icon id="debug" />
              {t("settings.managementApi")}
            </h2>
          </div>
          <div className="sr-rows">
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.debug")}</strong>
                <small>{t("settings.debugDesc")}</small>
              </div>
              <Switch
                on={config?.debug ?? settings.debug}
                disabled={isManagementBusy}
                onChange={() => {
                  const next = !(config?.debug ?? settings.debug);
                  onSaveSettings({ ...settings, debug: next });
                  if (config) {
                    onRunManagementStateAction("set_management_debug", { enabled: next });
                  }
                }}
                label="Debug"
              />
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.requestLog")}</strong>
                <small>{t("settings.requestLogDesc")}</small>
              </div>
              <Switch
                on={config?.request_log ?? false}
                disabled={isManagementBusy || !config}
                onChange={() => onRunManagementStateAction("set_management_request_log", { enabled: !(config?.request_log ?? false) })}
                label="Request log"
              />
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.loggingToFile")}</strong>
                <small>{t("settings.loggingToFileDesc")}</small>
              </div>
              <Switch
                on={config?.logging_to_file ?? settings.logging_to_file}
                disabled={isManagementBusy}
                onChange={() => {
                  const next = !(config?.logging_to_file ?? settings.logging_to_file);
                  onSaveSettings({ ...settings, logging_to_file: next });
                  if (config) {
                    onRunManagementStateAction("set_management_logging_to_file", { enabled: next });
                  }
                }}
                label="Logging to file"
              />
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.routingStrategy")}</strong>
              </div>
              <Select
                value={config?.routing_strategy ?? settings.routing_strategy}
                options={[
                  { value: "round-robin", label: t("settings.routingRoundRobin") },
                  { value: "fill-first", label: t("settings.routingFillFirst") },
                ]}
                disabled={isManagementBusy}
                onChange={(value) => {
                  const strategy = value as AppSettings["routing_strategy"];
                  // Always persist to settings.json so render_proxy_config writes
                  // it into config.yaml on restart; also apply live if running.
                  onSaveSettings({ ...settings, routing_strategy: strategy });
                  if (config) {
                    onRunManagementStateAction("set_management_routing_strategy", { strategy: value });
                  }
                }}
              />
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.requestRetry")}</strong>
                <small>{t("settings.requestRetryDesc", "失败时重试次数")}</small>
              </div>
              <input
                className="sr-input sr-input--sm"
                type="number"
                min={0}
                max={10}
                value={retryDraft}
                onChange={(event) => setRetryDraft(Number(event.target.value))}
                onBlur={() => {
                  if (retryDraft === settings.request_retry) return;
                  // Persist to settings.json (regenerated into config.yaml) AND apply
                  // live on the running proxy. Committing on blur means typing no
                  // longer fires a save per keystroke.
                  onSaveSettings({ ...settings, request_retry: retryDraft });
                  if (config) onRunManagementStateAction("set_management_request_retry", { count: retryDraft });
                }}
                disabled={isManagementBusy}
              />
            </div>
            <div className="sr-row">
              <div className="sr-row-text">
                <strong>{t("settings.upstreamProxy")}</strong>
                <small>{config?.proxy_url ? t("settings.upstreamConfigured") : t("settings.upstreamEmpty")}</small>
              </div>
              <div className="sr-api-url">
                <input
                  className="sr-input"
                  value={proxyUrlDraft}
                  onChange={(event) => onProxyUrlDraftChange(event.target.value)}
                  placeholder={config?.proxy_url || "http://127.0.0.1:7890"}
                />
                <button className="sr-mini-btn" type="button" onClick={onRefreshProxyUrlDraft} disabled={isManagementBusy}>
                  {t("common.read")}
                </button>
                <button
                  className="sr-mini-btn"
                  type="button"
                  onClick={() => {
                    onSaveSettings({ ...settings, proxy_url: proxyUrlDraft });
                    onRunManagementStateAction("set_management_proxy_url", { url: proxyUrlDraft });
                  }}
                  disabled={isManagementBusy || proxyUrlDraft.trim().length === 0}
                >
                  {t("common.write", "写入")}
                </button>
                <button
                  className="sr-mini-btn sr-mini-btn--danger"
                  type="button"
                  onClick={() => {
                    onSaveSettings({ ...settings, proxy_url: "" });
                    onRunManagementStateAction("clear_management_proxy_url");
                  }}
                  disabled={isManagementBusy}
                >
                  {t("common.clear", "清空")}
                </button>
              </div>
            </div>
          </div>
          {managementAction ? <p className="sr-note sr-note--busy">{t("settings.writing", "写入中…")} {managementAction}</p> : null}
        </article>

        <article className="sr-panel sr-card sr-advanced-card">
          <div className="sr-card-head">
            <h2 className="sr-card-title">
              <Icon id="brain" />
              {t("settings.advancedTitle", "高级设置")}
            </h2>
          </div>
          <div className="sr-advanced-columns">
            <div className="sr-compact-rows">
              <div className="sr-compact-row">
                <div className="sr-compact-text">
                  <strong>{t("settings.reasoningEffort")}</strong>
                  <small title={t("settings.reasoningEffortDesc")}>{t("settings.reasoningEffortDesc")}</small>
                </div>
                <Select
                  value={settings.reasoning_effort || ""}
                  options={[
                    { value: "", label: t("settings.reasoningDefault") },
                    { value: "low", label: t("logs.rsLow") },
                    { value: "medium", label: t("logs.rsMedium") },
                    { value: "high", label: t("logs.rsHigh") },
                    { value: "xhigh", label: t("logs.rsXHigh") },
                  ]}
                  disabled={isSaving}
                  onChange={(value) => onSaveSettings({ ...settings, reasoning_effort: value })}
                />
              </div>
              <div className="sr-compact-row">
                <div className="sr-compact-text">
                  <strong>{t("settings.forceModel")}</strong>
                  <small title={t("settings.forceModelDesc")}>{t("settings.forceModelDesc")}</small>
                </div>
                <input
                  className="sr-input sr-input--sm"
                  value={advDraft.force_model}
                  onChange={(event) => setAdvDraft({ ...advDraft, force_model: event.target.value })}
                  onBlur={() => {
                    if (advDraft.force_model !== settings.force_model) applySettings({ force_model: advDraft.force_model });
                  }}
                  placeholder="gpt-5.5"
                />
              </div>
              <div className="sr-compact-row">
                <div className="sr-compact-text">
                  <strong>{t("settings.sessionAffinity")}</strong>
                  <small title={t("settings.sessionAffinityDesc")}>{t("settings.sessionAffinityDesc")}</small>
                </div>
                <Switch
                  on={settings.session_affinity}
                  disabled={isSaving}
                  onChange={() => onSaveSettings({ ...settings, session_affinity: !settings.session_affinity })}
                  label="Session affinity"
                />
              </div>
              <div className="sr-compact-row">
                <div className="sr-compact-text">
                  <strong>{t("settings.absorbBoundAccount", "启动账号加入代理轮换")}</strong>
                  <small title={t("settings.absorbBoundAccountDesc", "开启后,一键启动绑定的账号不再被隔离,而是也留在代理池被 Codex(经代理)轮换使用,把它闲置的额度也用上。代价:该账号 token 同时用于登录和轮换,provider 敏感时极端情况可能需重新授权。默认关。")}>{t("settings.absorbBoundAccountDesc", "开启后,一键启动绑定的账号不再被隔离,而是也留在代理池被 Codex(经代理)轮换使用,把它闲置的额度也用上。代价:该账号 token 同时用于登录和轮换,provider 敏感时极端情况可能需重新授权。默认关。")}</small>
                </div>
                <Switch
                  on={settings.absorb_bound_account}
                  disabled={isSaving}
                  onChange={() => onSaveSettings({ ...settings, absorb_bound_account: !settings.absorb_bound_account })}
                  label="Absorb bound launch account into pool"
                />
              </div>
              <div className="sr-compact-row">
                <div className="sr-compact-text">
                  <strong>{t("settings.sessionAffinityTtl")}</strong>
                </div>
                <input
                  className="sr-input sr-input--sm"
                  value={advDraft.session_affinity_ttl}
                  onChange={(event) => setAdvDraft({ ...advDraft, session_affinity_ttl: event.target.value })}
                  onBlur={() => {
                    if (advDraft.session_affinity_ttl !== settings.session_affinity_ttl) applySettings({ session_affinity_ttl: advDraft.session_affinity_ttl });
                  }}
                  placeholder="1h"
                />
              </div>
              <div className="sr-compact-row">
                <div className="sr-compact-text">
                  <strong>{t("settings.maxRetryCredentials")}</strong>
                  <small title={t("settings.maxRetryCredentialsDesc")}>{t("settings.maxRetryCredentialsDesc")}</small>
                </div>
                <input
                  className="sr-input sr-input--sm"
                  type="number"
                  min={0}
                  value={advDraft.max_retry_credentials}
                  onChange={(event) => setAdvDraft({ ...advDraft, max_retry_credentials: Number(event.target.value) })}
                  onBlur={() => {
                    if (advDraft.max_retry_credentials !== settings.max_retry_credentials) applySettings({ max_retry_credentials: advDraft.max_retry_credentials });
                  }}
                />
              </div>
              <div className="sr-compact-row">
                <div className="sr-compact-text">
                  <strong>{t("settings.logsMaxSize")}</strong>
                  <small title={t("settings.logsMaxSizeDesc")}>{t("settings.logsMaxSizeDesc")}</small>
                </div>
                <input
                  className="sr-input sr-input--sm"
                  type="number"
                  min={0}
                  value={advDraft.logs_max_total_size_mb}
                  onChange={(event) => setAdvDraft({ ...advDraft, logs_max_total_size_mb: Number(event.target.value) })}
                  onBlur={() => {
                    if (advDraft.logs_max_total_size_mb !== settings.logs_max_total_size_mb) applySettings({ logs_max_total_size_mb: advDraft.logs_max_total_size_mb });
                  }}
                />
              </div>
            </div>
            <div className="sr-compact-rows">
              <div className="sr-compact-row">
                <div className="sr-compact-text">
                  <strong>{t("settings.disableCooling")}</strong>
                  <small title={t("settings.disableCoolingDesc")}>{t("settings.disableCoolingDesc")}</small>
                </div>
                <Switch
                  on={settings.disable_cooling}
                  disabled={isSaving}
                  onChange={() => onSaveSettings({ ...settings, disable_cooling: !settings.disable_cooling })}
                  label="Disable cooling"
                />
              </div>
              <div className="sr-compact-row">
                <div className="sr-compact-text">
                  <strong>{t("settings.disableImageGen")}</strong>
                  <small title={t("settings.disableImageGenDesc")}>{t("settings.disableImageGenDesc")}</small>
                </div>
                <Switch
                  on={settings.disable_image_generation}
                  disabled={isSaving}
                  onChange={() => onSaveSettings({ ...settings, disable_image_generation: !settings.disable_image_generation })}
                  label="Disable image generation"
                />
              </div>
              <div className="sr-compact-row">
                <div className="sr-compact-text">
                  <strong>{t("settings.forceModelPrefix")}</strong>
                  <small title={t("settings.forceModelPrefixDesc")}>{t("settings.forceModelPrefixDesc")}</small>
                </div>
                <Switch
                  on={settings.force_model_prefix}
                  disabled={isSaving}
                  onChange={() => onSaveSettings({ ...settings, force_model_prefix: !settings.force_model_prefix })}
                  label="Force model prefix"
                />
              </div>
              <div className="sr-compact-row">
                <div className="sr-compact-text">
                  <strong>{t("settings.passthroughHeaders")}</strong>
                  <small title={t("settings.passthroughHeadersDesc")}>{t("settings.passthroughHeadersDesc")}</small>
                </div>
                <Switch
                  on={settings.passthrough_headers}
                  disabled={isSaving}
                  onChange={() => onSaveSettings({ ...settings, passthrough_headers: !settings.passthrough_headers })}
                  label="Passthrough headers"
                />
              </div>
            </div>
          </div>
          <p className="sr-note">{t("settings.advancedNote")}</p>
        </article>
      </section>

      {/* 工具与自动化 */}
      <section className="sr-panel sr-tools-panel">
        <h2 className="sr-section-label">{t("settings.tools", "工具与自动化")}</h2>
        <div className="sr-tools-grid">
          <TunnelCard />
          <WarmupCard />
        </div>
      </section>
    </section>
  );
}
