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
  CodexLaunchProfile,
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

// Codex 模型的**回退**列表（仅当拉不到代理实时模型时用）；实际优先用 fetch_codex_models 从代理拉。
const CODEX_MODELS = [
  "gpt-5.5",
  "gpt-5.4",
  "gpt-5.4-mini",
  "gpt-5.3-codex-spark",
  "codex-auto-review",
];

// Kiro（claude-* via kiro-rs）模型。当配置的 API 密钥绑定到 Kiro 时,模型下拉改用这组。
// 只列 Kiro 免费账号能跑的（Sonnet 4.5 / Haiku 4.5）——列出账号用不了的(Opus、Sonnet 4.6)
// 会触发上游 INVALID_MODEL_ID 并冷却整个 Kiro 账号。与后端 DEFAULT_KIRO_MODELS 保持一致。
const KIRO_MODELS = [
  "claude-sonnet-4-5-20250929",
  "claude-sonnet-4-5-20250929-thinking",
  "claude-haiku-4-5-20251001",
  "claude-haiku-4-5-20251001-thinking",
];

// Codex 思考程度（model_reasoning_effort）。xhigh=极高，gpt-5.1-codex-max 等支持。
const CODEX_REASONING: { value: string; fallback: string }[] = [
  { value: "minimal", fallback: "最低" },
  { value: "low", fallback: "低" },
  { value: "medium", fallback: "中" },
  { value: "high", fallback: "高" },
  { value: "xhigh", fallback: "极高" },
];

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

// 新建/编辑方案的小表单草稿（id 为 null = 新建）。
type ProfileDraft = {
  id: string | null;
  name: string;
  launch_mode: string;
  bound_account: string;
  proxy_url: string;
  model: string;
  reasoning: string;
  api_key: string;
};

function newProfileId(): string {
  try {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
      return `codex-${crypto.randomUUID()}`;
    }
  } catch {
    /* 退回时间戳 */
  }
  return `codex-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
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
  const [apiKey, setApiKey] = useState(appState.settings.codex_api_key || "");
  const [slotDrafts, setSlotDrafts] = useState<Partial<Record<ModelSlot, string>>>({});
  const modelOptions = availableModels.length > 0 ? availableModels : appState.fallback_runtime.available_models;

  // ---- Codex 多套一键启动方案（注入临时、停止/关软件还原；同一刻只能跑一套）----
  const codexProfiles = useMemo<CodexLaunchProfile[]>(
    () => appState.settings.codex_profiles ?? [],
    [appState.settings.codex_profiles],
  );
  const [codexAccounts, setCodexAccounts] = useState<CodexAccountRef[]>([]);
  const [activeProfileId, setActiveProfileId] = useState<string | null>(null);
  const [proxyModels, setProxyModels] = useState<string[]>([]);
  const [launchBusy, setLaunchBusy] = useState(false); // 停止 / 全局忙
  const [startingId, setStartingId] = useState<string | null>(null); // 正在启动的方案行
  const [repairVisibilityBusy, setRepairVisibilityBusy] = useState(false);
  const [launchMsg, setLaunchMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [profileDraft, setProfileDraft] = useState<ProfileDraft | null>(null);
  const [pendingStart, setPendingStart] = useState<{ profile: CodexLaunchProfile; warnings: string[] } | null>(null);

  useEffect(() => {
    invoke<CodexAccountRef[]>("list_codex_launch_accounts").then(setCodexAccounts).catch((err) => console.warn("[AgentsScreen] list_codex_launch_accounts:", err));
    invoke<string | null>("codex_active_profile")
      .then((id) => setActiveProfileId(id ?? null))
      .catch((err) => console.warn("[AgentsScreen] codex_active_profile:", err));
    invoke<string[]>("fetch_codex_models").then(setProxyModels).catch((err) => console.warn("[AgentsScreen] fetch_codex_models:", err));
    // 后台监控发现用户自己退出了 Codex（没点「停止」）：状态回落，配置已自动还原。
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen("codex-launch-changed", () => {
          setActiveProfileId(null);
          setLaunchMsg({
            ok: true,
            text: t("agents.launch.autoRestored", "检测到 Codex 已退出，已自动还原配置"),
          });
        }),
      )
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      if (unlisten) unlisten();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const codexModelList = proxyModels.length > 0 ? proxyModels : CODEX_MODELS;
  const accountEmail = (key: string) =>
    codexAccounts.find((account) => account.key === key)?.email || key;
  const maskKey = (key: string) => {
    const trimmed = key.trim();
    if (!trimmed) return "";
    return trimmed.length <= 4 ? "••••" : `••••${trimmed.slice(-4)}`;
  };
  const BUILTIN_PROVIDERS = new Set(["codex", "claude", "copilot", "antigravity", "kiro", "glm", "trae"]);
  const codexKeyIssue = (profile: CodexLaunchProfile): string | null => {
    const key = profile.api_key.trim();
    if (!key) return null;
    const bound = (appState.api_key_bindings ?? []).find((binding) => binding.api_key === key)?.provider_id ?? "";
    if (bound === "codex") return null;
    if (bound && !BUILTIN_PROVIDERS.has(bound)) return null;
    const current = bound ? `其它服务商(${bound})` : "全部服务商";
    return `这个方案的 API 密钥没绑定到 Codex(当前:${current})——Codex 请求可能被路由到别的服务商而报错。建议先去「API 密钥」页把它绑到「Codex (OpenAI)」。`;
  };
  const reasoningLabel = (value: string) =>
    t(`agents.reasoning.${value}`, CODEX_REASONING.find((item) => item.value === value)?.fallback || value);

  function persistProfiles(next: CodexLaunchProfile[]) {
    onSaveSettings({ ...appState.settings, codex_profiles: next, remote_management_key: null });
  }

  function openNewProfile() {
    setLaunchMsg(null);
    setProfileDraft({
      id: null,
      name: "",
      launch_mode: "app",
      bound_account: "",
      proxy_url: appState.proxy.endpoint || "",
      model: "",
      reasoning: "high",
      api_key: "",
    });
  }

  // 编辑:把现有方案预填进同一个小表单(submitProfileDraft 按 id 命中就更新而不是新增)。
  function openEditProfile(profile: CodexLaunchProfile) {
    setLaunchMsg(null);
    setProfileDraft({
      id: profile.id,
      name: profile.name,
      launch_mode: profile.launch_mode || "app",
      bound_account: profile.bound_account,
      proxy_url: profile.proxy_url,
      model: profile.model,
      reasoning: profile.reasoning || "high",
      api_key: profile.api_key,
    });
  }

  function submitProfileDraft() {
    if (!profileDraft) return;
    const name = profileDraft.name.trim();
    if (!name) {
      setLaunchMsg({ ok: false, text: t("agents.launch.needName", "请填写方案名称") });
      return;
    }
    if (!profileDraft.bound_account.trim()) {
      setLaunchMsg({ ok: false, text: t("agents.launch.needAccount", "请选择要绑定的 Codex 账号") });
      return;
    }
    const saved: CodexLaunchProfile = {
      id: profileDraft.id ?? newProfileId(),
      name,
      launch_mode: profileDraft.launch_mode || "app",
      bound_account: profileDraft.bound_account,
      proxy_url: profileDraft.proxy_url.trim(),
      model: profileDraft.model,
      reasoning: profileDraft.reasoning || "high",
      api_key: profileDraft.api_key.trim(),
    };
    const exists = codexProfiles.some((profile) => profile.id === saved.id);
    const next = exists
      ? codexProfiles.map((profile) => (profile.id === saved.id ? saved : profile))
      : [...codexProfiles, saved];
    persistProfiles(next);
    setProfileDraft(null);
    setLaunchMsg(null);
  }

  function deleteProfile(profile: CodexLaunchProfile) {
    if (profile.id === activeProfileId) {
      setLaunchMsg({ ok: false, text: t("agents.launch.stopBeforeDelete", "该方案正在运行，请先停止再删除") });
      return;
    }
    if (profileDraft?.id === profile.id) setProfileDraft(null);
    persistProfiles(codexProfiles.filter((item) => item.id !== profile.id));
  }

  // 启动入口：收集"需要先确认"的事项（要切换正在跑的方案 / 密钥没绑到 Codex），
  // 有任意一条就先弹确认；都没有就直接起。
  function requestStartProfile(profile: CodexLaunchProfile) {
    setLaunchMsg(null);
    const warnings: string[] = [];
    if (activeProfileId && activeProfileId !== profile.id) {
      const current = codexProfiles.find((item) => item.id === activeProfileId);
      warnings.push(`会先停掉当前运行的「${current?.name ?? "方案"}」再启动这套。`);
    }
    const keyIssue = codexKeyIssue(profile);
    if (keyIssue) warnings.push(keyIssue);
    if (warnings.length > 0) {
      setPendingStart({ profile, warnings });
      return;
    }
    void doStartProfile(profile);
  }

  async function doStartProfile(profile: CodexLaunchProfile) {
    setPendingStart(null);
    setStartingId(profile.id);
    setLaunchMsg(null);
    try {
      const message = await invoke<string>("codex_start", { profileId: profile.id });
      setActiveProfileId(profile.id);
      setLaunchMsg({ ok: true, text: message });
    } catch (error) {
      setLaunchMsg({ ok: false, text: String(error) });
    } finally {
      setStartingId(null);
    }
  }

  async function stopActiveProfile() {
    setLaunchBusy(true);
    setLaunchMsg(null);
    try {
      const message = await invoke<string>("codex_stop");
      setActiveProfileId(null);
      setLaunchMsg({ ok: true, text: message });
    } catch (error) {
      setLaunchMsg({ ok: false, text: String(error) });
    } finally {
      setLaunchBusy(false);
    }
  }

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

  async function repairCodexVisibility() {
    setRepairVisibilityBusy(true);
    setLaunchMsg(null);
    try {
      const message = await invoke<string>("codex_repair_session_visibility");
      setLaunchMsg({ ok: true, text: message });
    } catch (error) {
      setLaunchMsg({ ok: false, text: String(error) });
    } finally {
      setRepairVisibilityBusy(false);
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

  // Codex 卡片下的「启动方案」区：一行一套,运行中的高亮,其余可启动。
  function codexLaunchPanel() {
    const running = (id: string) => id === activeProfileId;
    return (
      <div className="codex-launch">
        <div className="codex-launch-bar">
          <span className="codex-launch-label">{t("agents.launch.profiles", "启动方案")}</span>
          <span className="codex-launch-note">{t("agents.launch.onlyOne", "同一时刻只能启动一套")}</span>
          <button className="codex-profile-add" type="button" onClick={openNewProfile}>
            ＋ {t("agents.launch.newProfile", "新建方案")}
          </button>
        </div>

        <div className="codex-launch-apppath">
          <span>
            {t("agents.launch.appPath", "应用路径")}：
            <code>
              {appState.settings.codex_app_path ||
                t("agents.launch.appPathAuto", "未设置（启动时自动探测）")}
            </code>
          </span>
          <button className="ghost-action" type="button" onClick={() => void detectCodexApp()} disabled={launchBusy}>
            {t("agents.launch.detect", "探测")}
          </button>
        </div>

        {codexProfiles.length === 0 && !profileDraft ? (
          <p className="codex-launch-empty">
            {t("agents.launch.emptyProfiles", "还没有启动方案。点「新建方案」，选好账号 / 模型 / 思考程度，就能一键拉起 Codex。")}
          </p>
        ) : null}

        {codexProfiles.length > 0 ? (
          <ul className="codex-profile-list">
            {codexProfiles.map((profile) => {
              const isRunning = running(profile.id);
              const isStarting = startingId === profile.id;
              const keyIssue = codexKeyIssue(profile);
              const pending = pendingStart?.profile.id === profile.id ? pendingStart : null;
              return (
                <li key={profile.id} className={`codex-profile-row${isRunning ? " is-running" : ""}`}>
                  <span className={`codex-profile-dot${isRunning ? " on" : ""}`} aria-hidden="true" />
                  <div className="codex-profile-main">
                    <div className="codex-profile-name-line">
                      <span className="codex-profile-name">{profile.name}</span>
                      {isRunning ? (
                        <span className="codex-profile-running">{t("agents.launch.running", "运行中")}</span>
                      ) : null}
                      {!isRunning && keyIssue ? (
                        <span className="codex-profile-warn" title={keyIssue}>
                          {t("agents.launch.keyWarnBadge", "⚠ 密钥未绑 Codex")}
                        </span>
                      ) : null}
                    </div>
                    <span className="codex-profile-summary">
                      {accountEmail(profile.bound_account)} · {profile.model || t("agents.launch.defaultModel", "默认模型")} ·{" "}
                      {reasoningLabel(profile.reasoning)} · {profile.launch_mode === "cli" ? t("agents.launch.modeCli", "终端") : t("agents.launch.modeApp", "应用")}
                    </span>
                    {profile.proxy_url || profile.api_key ? (
                      <span className="codex-profile-route">
                        {profile.proxy_url || t("agents.launch.localProxy", "本机代理")}
                        {profile.api_key ? ` · 🔑 ${maskKey(profile.api_key)}` : ""}
                      </span>
                    ) : null}
                  </div>
                  <div className="codex-profile-actions">
                    {isRunning ? (
                      <button
                        className="agent-launch-btn agent-launch-btn--stop"
                        type="button"
                        onClick={() => void stopActiveProfile()}
                        disabled={launchBusy}
                      >
                        {launchBusy ? t("agents.launch.working", "处理中…") : t("agents.launch.stop", "停止")}
                      </button>
                    ) : (
                      <button
                        className="agent-launch-btn agent-launch-btn--start"
                        type="button"
                        onClick={() => requestStartProfile(profile)}
                        disabled={isStarting || launchBusy}
                      >
                        {isStarting ? t("agents.launch.launching", "启动中…") : t("agents.launch.start", "启动")}
                      </button>
                    )}
                    <button
                      className="codex-profile-edit"
                      type="button"
                      onClick={() => openEditProfile(profile)}
                      disabled={isRunning}
                    >
                      {t("common.edit", "编辑")}
                    </button>
                    <button
                      className="codex-profile-del"
                      type="button"
                      onClick={() => deleteProfile(profile)}
                      disabled={isRunning}
                    >
                      {t("common.delete", "删除")}
                    </button>
                  </div>
                  {pending ? (
                    <div className="codex-switch-confirm">
                      <div className="codex-warn-list">
                        {pending.warnings.map((warning, index) => (
                          <span key={index}>• {warning}</span>
                        ))}
                      </div>
                      <div className="codex-switch-actions">
                        <button className="primary-action" type="button" onClick={() => void doStartProfile(profile)}>
                          {t("agents.launch.startAnyway", "确认启动")}
                        </button>
                        <button className="secondary-action" type="button" onClick={() => setPendingStart(null)}>
                          {t("common.cancel", "取消")}
                        </button>
                      </div>
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        ) : null}

        {profileDraft ? (
          <div className="codex-profile-form">
            <div className="settings-form-grid">
              <label>
                {t("agents.launch.profileName", "方案名称")}
                <input
                  value={profileDraft.name}
                  placeholder={t("agents.launch.profileNamePlaceholder", "如：日常-5.5极高")}
                  onChange={(event) => setProfileDraft((draft) => (draft ? { ...draft, name: event.target.value } : draft))}
                />
              </label>
              <label>
                {t("agents.launch.mode", "启动方式")}
                <Select
                  value={profileDraft.launch_mode}
                  options={[
                    { value: "app", label: t("agents.launch.modeApp", "应用") },
                    { value: "cli", label: t("agents.launch.modeCli", "终端") },
                  ]}
                  onChange={(value) => setProfileDraft((draft) => (draft ? { ...draft, launch_mode: value } : draft))}
                />
              </label>
              <label>
                {t("agents.launch.account", "绑定账号")}
                <Select
                  value={profileDraft.bound_account}
                  options={[
                    { value: "", label: t("agents.launch.accountPick", "选择一个 Codex 账号") },
                    ...codexAccounts.map((account) => ({
                      value: account.key,
                      label: account.disabled ? `${account.email}（已禁用）` : account.email,
                    })),
                  ]}
                  onChange={(value) => setProfileDraft((draft) => (draft ? { ...draft, bound_account: value } : draft))}
                />
              </label>
              <label>
                {t("agents.apiKey")}
                <input
                  type="password"
                  value={profileDraft.api_key}
                  placeholder={t("agents.launch.apiKeyPlaceholder", "留空 = 自动取代理第一个 key")}
                  onChange={(event) => setProfileDraft((draft) => (draft ? { ...draft, api_key: event.target.value } : draft))}
                />
              </label>
              <label style={{ gridColumn: "1 / -1" }}>
                {t("agents.proxyUrl")}
                <input
                  value={profileDraft.proxy_url}
                  placeholder={appState.proxy.endpoint || "http://127.0.0.1:28317"}
                  onChange={(event) => setProfileDraft((draft) => (draft ? { ...draft, proxy_url: event.target.value } : draft))}
                />
              </label>
              <label>
                {t("agents.codexModel", "模型")}
                <Select
                  value={profileDraft.model}
                  options={[
                    { value: "", label: t("agents.unspecified") },
                    ...codexModelList.map((model) => ({ value: model, label: model })),
                  ]}
                  onChange={(value) => setProfileDraft((draft) => (draft ? { ...draft, model: value } : draft))}
                />
              </label>
              <label>
                {t("agents.codexReasoning", "思考程度")}
                <Select
                  value={profileDraft.reasoning}
                  options={CODEX_REASONING.map((item) => ({
                    value: item.value,
                    label: t(`agents.reasoning.${item.value}`, item.fallback),
                  }))}
                  onChange={(value) => setProfileDraft((draft) => (draft ? { ...draft, reasoning: value } : draft))}
                />
              </label>
            </div>
            <p className="codex-launch-hint">
              {t("agents.launch.accountHint", "绑定账号仅用于让应用登录启动；实际调用走代理，该账号本身不耗额度。")}
            </p>
            <div className="inline-actions">
              <button className="primary-action" type="button" onClick={submitProfileDraft}>
                {t("agents.launch.saveProfile", "保存方案")}
              </button>
              <button className="secondary-action" type="button" onClick={() => setProfileDraft(null)}>
                {t("common.cancel", "取消")}
              </button>
            </div>
          </div>
        ) : null}

        {launchMsg ? (
          <div className={`codex-launch-status ${launchMsg.ok ? "ok" : "err"}`}>{launchMsg.text}</div>
        ) : null}
      </div>
    );
  }

  function configForm(status: AgentStatus) {
    const configuration = agentConfigurations[status.agent.id];
    const backups = agentBackups[status.agent.id] ?? configuration?.backups ?? [];
    // 模型列表随配置的 API 密钥所绑定的池而变：绑 Kiro → 显示 Kiro 的 claude 模型,
    // 这样这个 CLI 走 Kiro;否则走 Codex（优先代理实时模型,回退内置列表 ∪ 发现的 gpt/codex）。
    const boundProvider = (appState.api_key_bindings ?? []).find(
      (binding) => binding.api_key === apiKey,
    )?.provider_id;
    const codexConfigModels =
      boundProvider === "kiro"
        ? KIRO_MODELS
        : proxyModels.length > 0
          ? proxyModels
          : [
              ...new Set([
                ...CODEX_MODELS,
                ...modelOptions
                  .filter((model) => /gpt-5|codex/i.test(model.id))
                  .map((model) => model.id),
              ]),
            ];

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

        {status.agent.id === "codex" ? (
          <div className="settings-form-grid">
            <label>
              {t("agents.codexModel", "模型")}
              <Select
                value={slotDrafts.sonnet ?? appState.settings.codex_model ?? ""}
                options={[
                  { value: "", label: t("agents.unspecified") },
                  ...codexConfigModels.map((model) => ({ value: model, label: model })),
                ]}
                disabled={isBusy}
                onChange={(value) => setSlotDrafts((current) => ({ ...current, sonnet: value }))}
              />
            </label>
          </div>
        ) : (
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
        )}

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
                onConfigure={
                  status.agent.id === "codex"
                    ? undefined // Codex 改用「启动方案」配置 + 启动,不再需要单独的「配置」面板
                    : () => setExpandedId((current) => (current === status.agent.id ? null : status.agent.id))
                }
                onRepairVisibility={status.agent.id === "codex" ? () => void repairCodexVisibility() : undefined}
                repairBusy={repairVisibilityBusy}
              />
              {status.agent.id === "codex" ? codexLaunchPanel() : null}
              {status.agent.id !== "codex" && expandedId === status.agent.id ? configForm(status) : null}
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
  onRepairVisibility,
  repairBusy = false,
}: {
  status: AgentStatus;
  accent: string;
  expanded?: boolean;
  muted?: boolean;
  isBusy: boolean;
  onConfigure?: () => void;
  onRepairVisibility?: () => void;
  repairBusy?: boolean;
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
        {status.binary_path ? <p className="agent-card-path">{status.binary_path}</p> : null}
      </div>
      <div className="agent-card-actions">
        {onRepairVisibility ? (
          <button className="ghost-action" type="button" onClick={onRepairVisibility} disabled={repairBusy}>
            {repairBusy ? t("agents.launch.repairingVisibility", "修复中…") : t("agents.launch.repairVisibility", "修复可见性")}
          </button>
        ) : null}
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
      </div>
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
