import { useEffect, useState } from "react";
import type { AppSettings, AppState, ConnectionMode, CredentialStatus, OperatingMode, ThemeMode } from "../../types";
import { Switch } from "../Switch";
import { Select } from "../Select";
import { useT } from "../../i18n";
import { invoke } from "../../lib/tauri";
import { isHideSensitiveEnabled, setHideSensitiveEnabled } from "../../lib/format";
import { TunnelCard, WarmupCard } from "../TunnelCard";

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

const LANGUAGES: { value: string; label: string }[] = [
  { value: "en", label: "English" },
  { value: "zh-CN", label: "简体中文" },
  { value: "zh-TW", label: "繁體中文" },
  { value: "ja", label: "日本語" },
];

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

  return (
    <section className="section-page settings-page">
      <header className="page-topbar" data-tauri-drag-region>
        <h1>{t("nav.settings")}</h1>
        <div className="topbar-actions">
          <button className="ghost-action" type="button" onClick={onOpenConfigRoot} disabled={platformAction === "open_config_root"}>
            {t("settings.openConfigDir")}
          </button>
          <button className="ghost-action" type="button" onClick={() => void invoke("open_logs_dir")}>
            {t("settings.openLogsDir")}
          </button>
        </div>
      </header>

      <div className="settings-group">
        <h2 className="settings-group-title">{t("settings.appMode")}</h2>
        <div className="app-mode-cards">
          {APP_MODES.map((mode) => (
            <button
              key={mode.id}
              type="button"
              className={activeMode === mode.id ? "app-mode-card app-mode-card--selected" : "app-mode-card"}
              style={activeMode === mode.id ? { borderColor: `#${mode.accent}`, background: `#${mode.accent}0f` } : undefined}
              onClick={() => selectMode(mode)}
              disabled={isSaving}
            >
              <span className="app-mode-icon" style={{ color: `#${mode.accent}`, background: `#${mode.accent}1f` }} aria-hidden="true">
                <AppModeIcon mode={mode.id} />
              </span>
              <span className="app-mode-text">
                <span className="app-mode-title">
                  {t(`settings.${appModeKey[mode.id] ?? "localProxy"}`, mode.title)}
                  {mode.badge ? <span className={mode.badge === "Experimental" ? "nav-badge" : "app-mode-badge"}>{mode.badge === "Experimental" ? t("common.experimental") : t("common.default")}</span> : null}
                </span>
                <span className="app-mode-desc">{t(`settings.${appModeKey[mode.id] ?? "localProxy"}.desc`, mode.desc)}</span>
              </span>
              <span className={activeMode === mode.id ? "app-mode-radio app-mode-radio--on" : "app-mode-radio"} style={activeMode === mode.id ? { borderColor: `#${mode.accent}`, color: `#${mode.accent}` } : undefined} aria-hidden="true" />
            </button>
          ))}
        </div>
      </div>

      <div className="settings-group">
        <h2 className="settings-group-title">{t("settings.general")}</h2>
        <div className="settings-card">
          <div className="settings-row">
            <div className="settings-row-text">
              <strong>{t("settings.launchAtLogin")}</strong>
            </div>
            <Switch
              on={launchEnabled}
              disabled={isSaving || platformAction === "set_launch_at_login" || !platform.launch_at_login_available}
              onChange={() => onSetLaunchAtLogin(!launchEnabled)}
              label="Launch at login"
            />
          </div>
          <div className="settings-row">
            <div className="settings-row-text">
              <strong>{t("settings.notifications")}</strong>
              <small>{platform.notifications_available ? t("settings.notifications.desc") : t("settings.notifications.unavailable")}</small>
            </div>
            <div className="settings-row-controls">
              <button className="ghost-action" type="button" onClick={onSendTestNotification} disabled={platformAction !== null || !settings.notifications_enabled}>
                {t("common.test")}
              </button>
              <Switch
                on={settings.notifications_enabled}
                disabled={isSaving || platformAction !== null}
                onChange={() => void toggleNotifications()}
                label="Notifications"
              />
            </div>
          </div>
        </div>
      </div>

      <div className="settings-group">
        <h2 className="settings-group-title">{t("settings.language")}</h2>
        <div className="settings-card">
          <div className="settings-row">
            <div className="settings-row-text">
              <strong>{t("settings.language.label")}</strong>
            </div>
            <Select value={settings.language} options={LANGUAGES} onChange={(value) => applySettings({ language: value })} />
          </div>
        </div>
      </div>

      <div className="settings-group">
        <h2 className="settings-group-title">{t("settings.appearance")}</h2>
        <div className="settings-card">
          <div className="settings-row">
            <div className="settings-row-text">
              <strong>{t("settings.theme")}</strong>
              <small>{t("settings.theme.desc")}</small>
            </div>
            <Select
              value={settings.theme}
              options={[
                { value: "system", label: t("theme.system") },
                { value: "light", label: t("theme.light") },
                { value: "dark", label: t("theme.dark") },
              ]}
              onChange={(value) => applySettings({ theme: value as ThemeMode })}
            />
          </div>
        </div>
      </div>

      <div className="settings-group">
        <h2 className="settings-group-title">{t("settings.privacy")}</h2>
        <div className="settings-card">
          <div className="settings-row">
            <div className="settings-row-text">
              <strong>{t("settings.hideSensitive")}</strong>
              <small>{t("settings.hideSensitive.desc")}</small>
            </div>
            <Switch on={hideSensitive} onChange={toggleHideSensitive} label="Hide Sensitive Information" />
          </div>
        </div>
      </div>

      <div className="settings-group">
        <h2 className="settings-group-title">{t("settings.proxyConnection")}</h2>
        <div className="settings-card">
          <div className="settings-row settings-row--input">
            <div className="settings-row-text">
              <strong>{t("settings.host")}</strong>
            </div>
            <input className="settings-input" value={connDraft.proxy_host} onChange={(event) => setConnDraft({ ...connDraft, proxy_host: event.target.value })} />
          </div>
          <div className="settings-row settings-row--input">
            <div className="settings-row-text">
              <strong>{t("settings.port")}</strong>
            </div>
            <input
              className="settings-input"
              type="number"
              min={1}
              max={65535}
              value={connDraft.proxy_port}
              onChange={(event) => setConnDraft({ ...connDraft, proxy_port: Number(event.target.value) })}
            />
          </div>
          <div className="settings-row settings-row--input">
            <div className="settings-row-text">
              <strong>{t("settings.remoteEndpoint")}</strong>
            </div>
            <input
              className="settings-input"
              value={connDraft.remote_endpoint_url ?? ""}
              onChange={(event) => setConnDraft({ ...connDraft, remote_endpoint_url: event.target.value.trim() ? event.target.value : null })}
              placeholder="https://example.com/v0/management"
            />
          </div>
          <div className="settings-row settings-row--input">
            <div className="settings-row-text">
              <strong>{t("settings.remoteKey")}</strong>
            </div>
            <input
              className="settings-input"
              type="password"
              value={connDraft.remote_management_key}
              onChange={(event) => setConnDraft({ ...connDraft, remote_management_key: event.target.value })}
              placeholder={credentialStatus.remote_management_key_masked ?? "保存后迁入安全存储"}
            />
          </div>
          <div className="settings-row">
            <div className="settings-row-text">
              <strong>{t("settings.allowRemote")}</strong>
              <small>{t("settings.allowRemoteDesc")}</small>
            </div>
            <Switch on={connDraft.allow_remote} disabled={isSaving} onChange={() => setConnDraft({ ...connDraft, allow_remote: !connDraft.allow_remote })} label="Allow remote" />
          </div>
          <div className="settings-row settings-row--actions">
            <span className="settings-status">
              {t("settings.localRuntime")}：{appState.proxy.status === "running" && appState.proxy.health.ok ? "running · healthy" : appState.proxy.status}
              {connDirty ? (
                <strong className="settings-unsaved" style={{ color: "#d97706", marginInlineStart: 8 }}>
                  ● {t("settings.unsavedChanges")}
                </strong>
              ) : null}
            </span>
            <div className="settings-row-controls">
              <button className="danger-action" type="button" onClick={onClearRemoteManagementKey} disabled={platformAction !== null}>
                {t("settings.clearKey")}
              </button>
              <button className="ghost-action" type="button" onClick={() => void onRefreshCredentialStatus()} disabled={platformAction !== null}>
                {t("settings.refreshCreds")}
              </button>
              <button className={connDirty ? "primary-action" : "secondary-action"} type="button" onClick={saveConnection} disabled={isSaving}>
                {isSaving ? t("common.saving") : t("settings.saveConnection")}
              </button>
            </div>
          </div>
        </div>
      </div>

      <div className="settings-group">
        <h2 className="settings-group-title">{t("settings.managementApi")}</h2>
        <div className="settings-card">
          <div className="settings-row">
            <div className="settings-row-text">
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
          <div className="settings-row">
            <div className="settings-row-text">
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
          <div className="settings-row">
            <div className="settings-row-text">
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
          <div className="settings-row settings-row--input">
            <div className="settings-row-text">
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
          <div className="settings-row settings-row--input">
            <div className="settings-row-text">
              <strong>{t("settings.requestRetry")}</strong>
            </div>
            <input
              className="settings-input settings-input--sm"
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
          <div className="settings-row settings-row--input">
            <div className="settings-row-text">
              <strong>{t("settings.upstreamProxy")}</strong>
              <small>{config?.proxy_url ? t("settings.upstreamConfigured") : t("settings.upstreamEmpty")}</small>
            </div>
            <div className="settings-row-controls">
              <input
                className="settings-input"
                value={proxyUrlDraft}
                onChange={(event) => onProxyUrlDraftChange(event.target.value)}
                placeholder={config?.proxy_url || "http://127.0.0.1:7890"}
              />
              <button className="ghost-action" type="button" onClick={onRefreshProxyUrlDraft} disabled={isManagementBusy}>
                {t("common.read")}
              </button>
              <button
                className="secondary-action"
                type="button"
                onClick={() => {
                  onSaveSettings({ ...settings, proxy_url: proxyUrlDraft });
                  onRunManagementStateAction("set_management_proxy_url", { url: proxyUrlDraft });
                }}
                disabled={isManagementBusy || proxyUrlDraft.trim().length === 0}
              >
                写入
              </button>
              <button
                className="danger-action"
                type="button"
                onClick={() => {
                  onSaveSettings({ ...settings, proxy_url: "" });
                  onRunManagementStateAction("clear_management_proxy_url");
                }}
                disabled={isManagementBusy}
              >
                清空
              </button>
            </div>
          </div>
          {managementAction ? <p className="settings-status settings-status--busy">写入中… {managementAction}</p> : null}
        </div>
      </div>

      <div className="settings-group">
        <h2 className="settings-group-title">{t("settings.advanced")}</h2>
        <div className="settings-card">
          <div className="settings-row settings-row--input">
            <div className="settings-row-text">
              <strong>{t("settings.reasoningEffort")}</strong>
              <small>{t("settings.reasoningEffortDesc")}</small>
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
          <div className="settings-row settings-row--input">
            <div className="settings-row-text">
              <strong>{t("settings.forceModel")}</strong>
              <small>{t("settings.forceModelDesc")}</small>
            </div>
            <input
              className="settings-input"
              value={advDraft.force_model}
              onChange={(event) => setAdvDraft({ ...advDraft, force_model: event.target.value })}
              onBlur={() => {
                if (advDraft.force_model !== settings.force_model) applySettings({ force_model: advDraft.force_model });
              }}
              placeholder="gpt-5.5"
            />
          </div>
          <div className="settings-row">
            <div className="settings-row-text">
              <strong>{t("settings.sessionAffinity")}</strong>
              <small>{t("settings.sessionAffinityDesc")}</small>
            </div>
            <Switch
              on={settings.session_affinity}
              disabled={isSaving}
              onChange={() => onSaveSettings({ ...settings, session_affinity: !settings.session_affinity })}
              label="Session affinity"
            />
          </div>
          <div className="settings-row settings-row--input">
            <div className="settings-row-text">
              <strong>{t("settings.sessionAffinityTtl")}</strong>
            </div>
            <input
              className="settings-input settings-input--sm"
              value={advDraft.session_affinity_ttl}
              onChange={(event) => setAdvDraft({ ...advDraft, session_affinity_ttl: event.target.value })}
              onBlur={() => {
                if (advDraft.session_affinity_ttl !== settings.session_affinity_ttl)
                  applySettings({ session_affinity_ttl: advDraft.session_affinity_ttl });
              }}
              placeholder="1h"
            />
          </div>
          <div className="settings-row settings-row--input">
            <div className="settings-row-text">
              <strong>{t("settings.maxRetryCredentials")}</strong>
              <small>{t("settings.maxRetryCredentialsDesc")}</small>
            </div>
            <input
              className="settings-input settings-input--sm"
              type="number"
              min={0}
              value={advDraft.max_retry_credentials}
              onChange={(event) => setAdvDraft({ ...advDraft, max_retry_credentials: Number(event.target.value) })}
              onBlur={() => {
                if (advDraft.max_retry_credentials !== settings.max_retry_credentials)
                  applySettings({ max_retry_credentials: advDraft.max_retry_credentials });
              }}
            />
          </div>
          <div className="settings-row settings-row--input">
            <div className="settings-row-text">
              <strong>{t("settings.logsMaxSize")}</strong>
              <small>{t("settings.logsMaxSizeDesc")}</small>
            </div>
            <input
              className="settings-input settings-input--sm"
              type="number"
              min={0}
              value={advDraft.logs_max_total_size_mb}
              onChange={(event) => setAdvDraft({ ...advDraft, logs_max_total_size_mb: Number(event.target.value) })}
              onBlur={() => {
                if (advDraft.logs_max_total_size_mb !== settings.logs_max_total_size_mb)
                  applySettings({ logs_max_total_size_mb: advDraft.logs_max_total_size_mb });
              }}
            />
          </div>
          <div className="settings-row">
            <div className="settings-row-text">
              <strong>{t("settings.disableCooling")}</strong>
              <small>{t("settings.disableCoolingDesc")}</small>
            </div>
            <Switch
              on={settings.disable_cooling}
              disabled={isSaving}
              onChange={() => onSaveSettings({ ...settings, disable_cooling: !settings.disable_cooling })}
              label="Disable cooling"
            />
          </div>
          <div className="settings-row">
            <div className="settings-row-text">
              <strong>{t("settings.disableImageGen")}</strong>
              <small>{t("settings.disableImageGenDesc")}</small>
            </div>
            <Switch
              on={settings.disable_image_generation}
              disabled={isSaving}
              onChange={() => onSaveSettings({ ...settings, disable_image_generation: !settings.disable_image_generation })}
              label="Disable image generation"
            />
          </div>
          <div className="settings-row">
            <div className="settings-row-text">
              <strong>{t("settings.forceModelPrefix")}</strong>
              <small>{t("settings.forceModelPrefixDesc")}</small>
            </div>
            <Switch
              on={settings.force_model_prefix}
              disabled={isSaving}
              onChange={() => onSaveSettings({ ...settings, force_model_prefix: !settings.force_model_prefix })}
              label="Force model prefix"
            />
          </div>
          <div className="settings-row">
            <div className="settings-row-text">
              <strong>{t("settings.passthroughHeaders")}</strong>
              <small>{t("settings.passthroughHeadersDesc")}</small>
            </div>
            <Switch
              on={settings.passthrough_headers}
              disabled={isSaving}
              onChange={() => onSaveSettings({ ...settings, passthrough_headers: !settings.passthrough_headers })}
              label="Passthrough headers"
            />
          </div>
          <p className="settings-status">{t("settings.advancedNote")}</p>
        </div>
      </div>

      <TunnelCard />
      <WarmupCard />
    </section>
  );
}

function AppModeIcon({ mode }: { mode: AppMode }) {
  if (mode === "monitor") {
    return (
      <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
        <path d="M3 13V8M6.5 13V4M10 13v-6M13.5 13V6" />
      </svg>
    );
  }
  if (mode === "remote") {
    return (
      <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="8" cy="8" r="6" />
        <path d="M2 8h12M8 2c1.8 1.6 2.8 3.8 2.8 6S9.8 12.4 8 14C6.2 12.4 5.2 10.2 5.2 8S6.2 3.6 8 2z" />
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <rect x="2.5" y="3" width="11" height="4" rx="1.2" />
      <rect x="2.5" y="9" width="11" height="4" rx="1.2" />
      <path d="M5 5h.01M5 11h.01" />
    </svg>
  );
}
