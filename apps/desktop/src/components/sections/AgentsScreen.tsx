import { Fragment, useEffect, useMemo, useState } from "react";
import type {
  AgentBackupFile,
  AgentConfigMode,
  AgentConfigStorageOption,
  AgentConfigurationRequest,
  AgentConfigurationResult,
  AgentSetupMode,
  AgentStatus,
  AppSettings,
  AppState,
  AvailableModel,
  CodexAccountRef,
  ModelSlot,
  SavedAgentConfiguration,
} from "../../types";
import { RefreshIcon } from "../icons";
import { Select } from "../Select";
import { useT } from "../../i18n";
import { invoke } from "../../lib/tauri";

type AgentsScreenProps = {
  appState: AppState;
  isBusy: boolean;
  action: string | null;
  agentResult: AgentConfigurationResult | null;
  agentBackups: Record<string, AgentBackupFile[]>;
  agentConfigurations: Record<string, SavedAgentConfiguration>;
  availableModels: AvailableModel[];
  onRefreshAgents: () => void;
  onReadConfiguration: (agentId: string) => Promise<SavedAgentConfiguration | null>;
  onConfigureAgent: (request: AgentConfigurationRequest) => Promise<AgentConfigurationResult | null>;
  onListBackups: (agentId: string) => Promise<AgentBackupFile[]>;
  onRestoreBackup: (agentId: string, backupPath: string) => Promise<AgentConfigurationResult | null>;
  onResetConfiguration: (agentId: string) => Promise<AgentConfigurationResult | null>;
  onSaveSettings: (settings: AppSettings) => void;
};

const modelSlots: ModelSlot[] = ["opus", "sonnet", "haiku"];

const AGENT_ACCENTS: Record<string, string> = {
  claude: "D97757",
  codex: "10A37F",
  factory: "2FB344",
  gemini: "4285F4",
  opencode: "8A5CF6",
  amp: "8A8A8E",
};

function agentAccent(id: string): string {
  return AGENT_ACCENTS[id] ?? "0A84FF";
}

export function AgentsScreen({
  appState,
  isBusy,
  action,
  agentResult,
  agentBackups,
  agentConfigurations,
  availableModels,
  onRefreshAgents,
  onReadConfiguration,
  onConfigureAgent,
  onListBackups,
  onRestoreBackup,
  onResetConfiguration,
  onSaveSettings,
}: AgentsScreenProps) {
  const t = useT();
  const sortedAgents = useMemo(
    () =>
      [...appState.agents].sort((left, right) => {
        if (left.installed !== right.installed) return left.installed ? -1 : 1;
        return left.agent.display_name.localeCompare(right.agent.display_name);
      }),
    [appState.agents],
  );
  const installedAgents = sortedAgents.filter((status) => status.installed);
  const configuredAgents = sortedAgents.filter((status) => status.configured);
  const notInstalledAgents = sortedAgents.filter((status) => !status.installed);

  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [mode, setMode] = useState<AgentConfigMode>("manual");
  const [setupMode, setSetupMode] = useState<AgentSetupMode>("proxy");
  const [storageOption, setStorageOption] = useState<AgentConfigStorageOption>("both");
  const [proxyUrl, setProxyUrl] = useState(appState.proxy.endpoint || "http://127.0.0.1:28317");
  const [apiKey, setApiKey] = useState("");
  const [slotDrafts, setSlotDrafts] = useState<Partial<Record<ModelSlot, string>>>({});
  const modelOptions = availableModels.length > 0 ? availableModels : appState.fallback_runtime.available_models;

  // ---- Codex 一键启动 ----
  const [launchMode, setLaunchMode] = useState<string>(appState.settings.codex_launch_mode || "app");
  const [boundAccount, setBoundAccount] = useState<string>(appState.settings.codex_bound_account || "");
  const [codexAccounts, setCodexAccounts] = useState<CodexAccountRef[]>([]);
  const [launchBusy, setLaunchBusy] = useState(false);
  const [launchMsg, setLaunchMsg] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    invoke<CodexAccountRef[]>("list_codex_launch_accounts").then(setCodexAccounts).catch(() => {});
  }, []);

  async function detectCodexApp() {
    try {
      const path = await invoke<string | null>("detect_codex_app");
      if (path) {
        onSaveSettings({ ...appState.settings, codex_app_path: path, remote_management_key: null });
        setLaunchMsg({ ok: true, text: `${t("agents.launch.detected", "已探测到 Codex 应用")}：${path}` });
      } else {
        setLaunchMsg({ ok: false, text: t("agents.launch.notDetected", "未探测到 Codex 应用，请确认已安装桌面版") });
      }
    } catch (error) {
      setLaunchMsg({ ok: false, text: String(error) });
    }
  }

  async function launchCodex(status: AgentStatus) {
    const model_slots: Partial<Record<ModelSlot, string>> = {};
    for (const slot of modelSlots) {
      const value = slotDrafts[slot]?.trim();
      if (value) model_slots[slot] = value;
    }
    const request: AgentConfigurationRequest = {
      agent_id: status.agent.id,
      mode: "automatic",
      setup_mode: "proxy",
      storage_option: storageOption,
      proxy_url: proxyUrl.trim(),
      api_key: apiKey.trim(),
      model_slots,
      use_oauth: false,
      available_models: modelOptions,
    };
    onSaveSettings({
      ...appState.settings,
      codex_launch_mode: launchMode,
      codex_bound_account: boundAccount,
      remote_management_key: null,
    });
    setLaunchBusy(true);
    setLaunchMsg(null);
    try {
      const message = await invoke<string>("configure_and_launch_codex", {
        request,
        accountKey: boundAccount,
        launchMode,
        appPath: appState.settings.codex_app_path || null,
      });
      setLaunchMsg({ ok: true, text: message });
    } catch (error) {
      setLaunchMsg({ ok: false, text: String(error) });
    } finally {
      setLaunchBusy(false);
    }
  }

  async function submitConfiguration(status: AgentStatus) {
    const model_slots: Partial<Record<ModelSlot, string>> = {};
    for (const slot of modelSlots) {
      const value = slotDrafts[slot]?.trim();
      if (value) model_slots[slot] = value;
    }
    await onConfigureAgent({
      agent_id: status.agent.id,
      mode,
      setup_mode: setupMode,
      storage_option: storageOption,
      proxy_url: proxyUrl.trim(),
      api_key: apiKey.trim(),
      model_slots,
      use_oauth: false,
      available_models: modelOptions,
    });
  }

  function configForm(status: AgentStatus) {
    const configuration = agentConfigurations[status.agent.id];
    const backups = agentBackups[status.agent.id] ?? configuration?.backups ?? [];

    return (
      <div className="agent-config-panel">
        <div className="settings-form-grid">
          <label>
            {t("agents.mode")}
            <Select
              value={mode}
              options={[
                { value: "manual", label: t("agents.modeManual") },
                { value: "automatic", label: t("agents.modeAuto") },
              ]}
              disabled={isBusy}
              onChange={(value) => setMode(value as AgentConfigMode)}
            />
          </label>
          <label>
            {t("agents.setup")}
            <Select
              value={setupMode}
              options={[
                { value: "proxy", label: t("agents.setupProxy") },
                { value: "default", label: t("agents.setupDefault") },
              ]}
              disabled={isBusy}
              onChange={(value) => setSetupMode(value as AgentSetupMode)}
            />
          </label>
          <label>
            {t("agents.storage")}
            <Select
              value={storageOption}
              options={[
                { value: "json", label: t("agents.storageJson") },
                { value: "shell", label: t("agents.storageShell") },
                { value: "both", label: t("agents.storageBoth") },
              ]}
              disabled={isBusy}
              onChange={(value) => setStorageOption(value as AgentConfigStorageOption)}
            />
          </label>
          <label>
            {t("agents.proxyUrl")}
            <input value={proxyUrl} onChange={(event) => setProxyUrl(event.target.value)} placeholder={appState.proxy.endpoint} />
          </label>
          <label>
            {t("agents.apiKey")}
            <input type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={t("agents.apiKeyPlaceholder")} />
          </label>
        </div>

        <div className="settings-form-grid">
          {modelSlots.map((slot) => (
            <label key={slot}>
              {slotLabel(slot)} {t("agents.model")}
              {modelOptions.length > 0 ? (
                <Select
                  value={slotDrafts[slot] ?? ""}
                  options={[{ value: "", label: t("agents.unspecified") }, ...modelOptions.map((model) => ({ value: model.id, label: model.name || model.id }))]}
                  disabled={isBusy}
                  onChange={(value) => setSlotDrafts((current) => ({ ...current, [slot]: value }))}
                />
              ) : (
                <input
                  value={slotDrafts[slot] ?? ""}
                  onChange={(event) => setSlotDrafts((current) => ({ ...current, [slot]: event.target.value }))}
                  placeholder={`${slot}-model-id`}
                />
              )}
            </label>
          ))}
        </div>

        <div className="inline-actions">
          <button className="secondary-action" type="button" onClick={() => void onReadConfiguration(status.agent.id)} disabled={isBusy}>
            {t("agents.readConfig")}
          </button>
          <button
            className="primary-action"
            type="button"
            onClick={() => void submitConfiguration(status)}
            disabled={isBusy || (setupMode === "proxy" && proxyUrl.trim().length === 0)}
          >
            {mode === "automatic" ? t("agents.writeConfig") : t("agents.generateConfig")}
          </button>
          <button className="danger-action" type="button" onClick={() => void onResetConfiguration(status.agent.id)} disabled={isBusy}>
            {t("agents.resetDefault")}
          </button>
        </div>

        {status.agent.id === "codex" ? (
          <div className="codex-launch-block">
            <div className="codex-launch-head">
              <strong>{t("agents.launch.title", "启动 Codex")}</strong>
              <span className="codex-launch-tag">{t("agents.launch.tag", "一键")}</span>
            </div>
            <p className="codex-launch-desc">
              {t("agents.launch.desc", "配好代理 → 绑定一个账号登录 → 启动 Codex，一步到位。")}
            </p>
            <div className="settings-form-grid">
              <label>
                {t("agents.launch.mode", "启动方式")}
                <Select
                  value={launchMode}
                  options={[
                    { value: "app", label: t("agents.launch.modeApp", "应用") },
                    { value: "cli", label: t("agents.launch.modeCli", "终端") },
                  ]}
                  disabled={launchBusy}
                  onChange={setLaunchMode}
                />
              </label>
              <label>
                {t("agents.launch.account", "绑定账号")}
                <Select
                  value={boundAccount}
                  options={[
                    { value: "", label: t("agents.launch.accountPick", "选择一个 Codex 账号") },
                    ...codexAccounts.map((account) => ({
                      value: account.key,
                      label: account.disabled ? `${account.email}（已禁用）` : account.email,
                    })),
                  ]}
                  disabled={launchBusy}
                  onChange={setBoundAccount}
                />
              </label>
            </div>
            <p className="codex-launch-hint">
              {t("agents.launch.accountHint", "仅用于让应用登录启动；实际调用走代理，该账号本身不耗额度。")}
            </p>
            {launchMode === "app" ? (
              <div className="codex-launch-path">
                <span>
                  {t("agents.launch.appPath", "应用路径")}：
                  <code>
                    {appState.settings.codex_app_path ||
                      t("agents.launch.appPathAuto", "未设置（启动时自动探测）")}
                  </code>
                </span>
                <button
                  className="secondary-action"
                  type="button"
                  onClick={() => void detectCodexApp()}
                  disabled={launchBusy}
                >
                  {t("agents.launch.detect", "探测")}
                </button>
              </div>
            ) : null}
            <div className="inline-actions">
              <button
                className="primary-action"
                type="button"
                onClick={() => void launchCodex(status)}
                disabled={launchBusy || boundAccount.trim().length === 0}
              >
                {launchBusy ? t("agents.launch.launching", "启动中…") : t("agents.launch.go", "配置并启动")}
              </button>
            </div>
            {launchMsg ? (
              <div className={`codex-launch-status ${launchMsg.ok ? "ok" : "err"}`}>{launchMsg.text}</div>
            ) : null}
          </div>
        ) : null}

        {configuration ? <SavedConfigurationCard configuration={configuration} /> : null}
        {agentResult ? <AgentResultCard result={agentResult} /> : null}
        <BackupList
          backups={backups}
          isBusy={isBusy}
          onRefresh={() => void onListBackups(status.agent.id)}
          onRestore={(backupPath) => void onRestoreBackup(status.agent.id, backupPath)}
        />
      </div>
    );
  }

  return (
    <section className="section-page agents-page">
      <header className="page-topbar" data-tauri-drag-region>
        <h1>{t("title.agents")}</h1>
        <button className="icon-button" type="button" onClick={onRefreshAgents} disabled={isBusy} title="重新检测" aria-label="重新检测">
          <RefreshIcon />
        </button>
      </header>

      <p className="page-subtitle">{t("agents.subtitle")}</p>

      <div className="agent-summary">
        <span className="agent-summary-item">
          <span className="agent-summary-dot agent-summary-dot--good" />
          <strong>{installedAgents.length}</strong> {t("agents.installed")}
        </span>
        <span className="agent-summary-item">
          <span className="agent-summary-dot agent-summary-dot--blue" />
          <strong>{configuredAgents.length}</strong> {t("agents.configured")}
        </span>
      </div>

      <section className="agent-section">
        <h2 className="agent-section-title">{t("agents.installed")}</h2>
        <div className="agent-list">
          {installedAgents.map((status) => (
            <Fragment key={status.agent.id}>
              <AgentCard
                status={status}
                accent={agentAccent(status.agent.id)}
                expanded={expandedId === status.agent.id}
                isBusy={isBusy}
                onConfigure={() => setExpandedId((current) => (current === status.agent.id ? null : status.agent.id))}
              />
              {expandedId === status.agent.id ? configForm(status) : null}
            </Fragment>
          ))}
        </div>
      </section>

      {notInstalledAgents.length > 0 ? (
        <section className="agent-section">
          <h2 className="agent-section-title">{t("agents.notInstalled")}</h2>
          <div className="agent-list">
            {notInstalledAgents.map((status) => (
              <AgentCard key={status.agent.id} status={status} accent={agentAccent(status.agent.id)} muted isBusy={isBusy} />
            ))}
          </div>
        </section>
      ) : null}

      {action === "detect_agents" ? <p className="page-subtitle">{t("agents.detecting")}</p> : null}
    </section>
  );
}

function AgentCard({
  status,
  accent,
  expanded = false,
  muted = false,
  isBusy,
  onConfigure,
}: {
  status: AgentStatus;
  accent: string;
  expanded?: boolean;
  muted?: boolean;
  isBusy: boolean;
  onConfigure?: () => void;
}) {
  const t = useT();
  const initial = status.agent.display_name.trim().charAt(0).toUpperCase() || "?";
  const statusLabel = !status.installed ? t("agents.statusNotInstalled") : status.configured ? t("agents.configured") : t("agents.installed");
  const tone = !status.installed ? "neutral" : status.configured ? "good" : "warn";

  return (
    <article className={muted ? "agent-card agent-card--muted" : "agent-card"}>
      <span className="agent-icon" style={{ color: `#${accent}`, background: `#${accent}1f` }} aria-hidden="true">
        {initial}
      </span>
      <div className="agent-card-info">
        <div className="agent-card-title">
          <strong>{status.agent.display_name}</strong>
          {!muted ? <span className={`agent-status-pill agent-status-pill--${tone}`}>{statusLabel}</span> : null}
        </div>
        {!muted ? <p className="agent-card-desc">{status.agent.description}</p> : null}
        {status.binary_path ? <p className="agent-card-path">{status.binary_path}</p> : null}
      </div>
      {!muted && onConfigure ? (
        <button
          className="agent-configure-btn"
          type="button"
          onClick={onConfigure}
          disabled={isBusy}
          style={{ color: `#${accent}`, background: `#${accent}1f` }}
        >
          {expanded ? t("common.collapse") : status.configured ? t("agents.reconfigure") : t("agents.configure")}
        </button>
      ) : null}
      {muted ? <span className="agent-muted-tag">{t("agents.autoDetected")}</span> : null}
    </article>
  );
}

function SavedConfigurationCard({ configuration }: { configuration: SavedAgentConfiguration }) {
  const t = useT();
  const slots = Object.entries(configuration.model_slots).filter(([, value]) => Boolean(value));

  return (
    <div className="record-card fallback-entry-card">
      <div className="record-meta">
        <span>{configuration.is_proxy_configured ? t("agents.proxyConfigured") : t("agents.proxyNotDetected")}</span>
        <span>{configuration.api_key_masked ?? t("agents.noKey")}</span>
      </div>
      <dl className="detail-list compact-details">
        <div>
          <dt>{t("agents.baseUrl")}</dt>
          <dd>{configuration.base_url ?? "未配置"}</dd>
        </div>
        <div>
          <dt>{t("agents.backups")}</dt>
          <dd>{configuration.backups.length}</dd>
        </div>
        {slots.map(([slot, value]) => (
          <div key={slot}>
            <dt>{slot}</dt>
            <dd>{value}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

function AgentResultCard({ result }: { result: AgentConfigurationResult }) {
  const t = useT();
  return (
    <div className={`record-card quota-account-card quota-account-card--${result.success ? "good" : "warn"}`}>
      <div className="record-meta">
        <span>{result.mode}</span>
        <span>{configTypeLabel(result.config_type)}</span>
        <span>{result.models_configured} {t("agents.modelsUnit")}</span>
      </div>
      <p>{result.instructions}</p>
      <dl className="detail-list compact-details">
        <div>
          <dt>{t("agents.configPath")}</dt>
          <dd>{result.config_path ?? "manual output"}</dd>
        </div>
        <div>
          <dt>{t("agents.authPath")}</dt>
          <dd>{result.auth_path ?? "无"}</dd>
        </div>
        <div>
          <dt>{t("agents.backup")}</dt>
          <dd>{result.backup_path ?? "未创建"}</dd>
        </div>
      </dl>
      {result.error ? <p className="empty-copy">{result.error}</p> : null}
      {result.raw_configs.map((config) => (
        <div className="stacked-form muted-form" key={`${config.filename ?? config.format}-${config.target_path ?? "manual"}`}>
          <strong>{config.filename ?? config.format}</strong>
          <small>{config.target_path ?? config.instructions}</small>
          <textarea readOnly value={config.content} rows={Math.min(12, Math.max(4, config.content.split("\n").length))} />
        </div>
      ))}
    </div>
  );
}

function BackupList({
  backups,
  isBusy,
  onRefresh,
  onRestore,
}: {
  backups: AgentBackupFile[];
  isBusy: boolean;
  onRefresh: () => void;
  onRestore: (backupPath: string) => void;
}) {
  const t = useT();
  return (
    <div className="stacked-form muted-form">
      <div className="panel-header">
        <div>
          <p className="eyebrow">{t("agents.backups")}</p>
          <h3>配置备份</h3>
        </div>
        <button className="ghost-action" type="button" onClick={onRefresh} disabled={isBusy}>
          刷新
        </button>
      </div>
      {backups.length === 0 ? (
        <p className="empty-copy">暂无备份。automatic 写入或恢复前会自动创建备份。</p>
      ) : (
        <div className="record-list compact-records">
          {backups.map((backup) => (
            <div className="record-card fallback-entry-card" key={backup.path}>
              <div>
                <strong>{backup.display_name}</strong>
                <small>{new Date(backup.timestamp_unix_seconds * 1000).toLocaleString()}</small>
              </div>
              <p>{backup.path}</p>
              <button className="secondary-action" type="button" onClick={() => onRestore(backup.path)} disabled={isBusy}>
                恢复
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function slotLabel(slot: ModelSlot) {
  switch (slot) {
    case "opus":
      return "Opus";
    case "sonnet":
      return "Sonnet";
    case "haiku":
      return "Haiku";
  }
}

function configTypeLabel(type: AgentStatus["agent"]["config_type"] | AgentConfigurationResult["config_type"]) {
  switch (type) {
    case "environment":
      return "Environment";
    case "file":
      return "File";
    case "both":
      return "File + Env";
  }
}
