import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
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
  DreamSkinThemeSummary,
  ModelSlot,
  SavedAgentConfiguration,
} from "../../types";
import { Select } from "../Select";
import { Switch } from "../Switch";
import { useT } from "../../i18n";
import { normalizeCodexReasoningLevels } from "../../lib/codexReasoning";
import { invoke } from "../../lib/tauri";
import "./agents.css";
import "./agents-rose.css";

// 内联 SVG symbol 图标(素材见 public/agents/agent-icons.svg;id 无 icon- 前缀)。
function Icon({ name }: { name: string }) {
  return (
    <svg className="icon" aria-hidden="true">
      <use href={`/agents/agent-icons.svg#${name}`} />
    </svg>
  );
}

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
  onSaveSettings: (
    settings: AppSettings,
    options?: { allowClearCodexProfiles?: boolean },
  ) => void;
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

// Codex 思考程度（model_reasoning_effort）的档位**不是固定的**：它按 model slug 从 Codex 的
// 模型目录里查（实测 gpt-5.6-sol/terra 六档 low…ultra，luna 五档，gpt-5.5 及更早只有四档）。
// 所以下拉项由后端 fetch_codex_reasoning_levels 按当前模型给出，这里只保留兜底。
// 兜底用 Codex 对自定义 provider 的通用默认四档；不含 minimal —— 目录里没有任何模型支持它，
// 选了就是往 config.toml 写一个无效的 model_reasoning_effort。
const CODEX_REASONING_FALLBACK = ["low", "medium", "high", "xhigh"];

// effort → 共享的 i18n key。和日志页/设置页复用同一批标签，三处永远一致。
const REASONING_I18N: Record<string, string> = {
  minimal: "logs.rsMinimal",
  low: "logs.rsLow",
  medium: "logs.rsMedium",
  high: "logs.rsHigh",
  xhigh: "logs.rsXHigh",
  max: "logs.rsMax",
  ultra: "logs.rsUltra",
};

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
  dream_skin_enabled: boolean;
  dream_skin_theme_id: string;
  bound_account: string;
  proxy_url: string;
  model: string;
  reasoning: string;
  api_key: string;
};

type CodexReasoningCatalogState = {
  model: string;
  levels: string[];
  loading: boolean;
  error: boolean;
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
  const isWindows = appState.platform.os === "windows";
  const bundledDreamSkinThemes = useMemo(
    () => [
      { id: "dream", name: t("agents.launch.dreamSkinThemeDream"), built_in: true },
      { id: "aurora", name: t("agents.launch.dreamSkinThemeAurora"), built_in: true },
      { id: "midnight", name: t("agents.launch.dreamSkinThemeMidnight"), built_in: true },
    ],
    [t],
  );
  const [dreamSkinThemeCatalog, setDreamSkinThemeCatalog] = useState<DreamSkinThemeSummary[]>([]);
  const [dreamSkinImportBusy, setDreamSkinImportBusy] = useState(false);
  const dreamSkinThemes = useMemo(
    () =>
      (dreamSkinThemeCatalog.length > 0 ? dreamSkinThemeCatalog : bundledDreamSkinThemes).map((theme) => ({
        value: theme.id,
        label: theme.name,
      })),
    [bundledDreamSkinThemes, dreamSkinThemeCatalog],
  );
  const dreamSkinThemeLabel = (themeId?: string) =>
    dreamSkinThemes.find((theme) => theme.value === themeId)?.label ?? themeId ?? dreamSkinThemes[0].label;
  const [codexAccounts, setCodexAccounts] = useState<CodexAccountRef[]>([]);
  const [activeProfileId, setActiveProfileId] = useState<string | null>(null);
  const [proxyModels, setProxyModels] = useState<string[]>([]);
  const [launchBusy, setLaunchBusy] = useState(false); // 停止 / 全局忙
  const [startingId, setStartingId] = useState<string | null>(null); // 正在启动的方案行
  const [repairVisibilityBusy, setRepairVisibilityBusy] = useState(false);
  const [launchMsg, setLaunchMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [profileDraft, setProfileDraft] = useState<ProfileDraft | null>(null);
  const [pendingStart, setPendingStart] = useState<{ profile: CodexLaunchProfile; warnings: string[] } | null>(null);
  const profileFormRef = useRef<HTMLDivElement>(null);
  const profileDraftOpen = profileDraft !== null;
  const profileDraftId = profileDraft?.id ?? null;

  useEffect(() => {
    if (!profileDraftOpen) return;
    const frame = window.requestAnimationFrame(() => {
      profileFormRef.current?.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "nearest" });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [profileDraftId, profileDraftOpen]);

  // 从运行中的代理拉它实际服务的模型。**不能只在挂载时拉一次**:用户去「服务商」页改了
  // 自定义接口(加/改模型名、加模型前缀)后,代理的 /v1/models 就变了,而本页的 proxyModels
  // 还是旧的 —— 下拉里看不到刚加的带前缀模型(如 weiloo/gpt-5.5),得手动点刷新才出来。
  // 所以打开「新建 / 编辑方案」表单时也重拉一次(下拉只有那时才用得上,拉得最准)。
  const refreshProxyModels = useCallback(() => {
    invoke<string[]>("fetch_codex_models")
      .then(setProxyModels)
      .catch((err) => console.warn("[AgentsScreen] fetch_codex_models:", err));
  }, []);

  const refreshDreamSkinThemes = useCallback(async () => {
    if (!isWindows) return;
    try {
      const themes = await invoke<DreamSkinThemeSummary[]>("list_dream_skin_themes");
      if (themes.length > 0) setDreamSkinThemeCatalog(themes);
    } catch (err) {
      console.warn("[AgentsScreen] list_dream_skin_themes:", err);
    }
  }, [isWindows]);

  useEffect(() => {
    invoke<CodexAccountRef[]>("list_codex_launch_accounts").then(setCodexAccounts).catch((err) => console.warn("[AgentsScreen] list_codex_launch_accounts:", err));
    invoke<string | null>("codex_active_profile")
      .then((id) => setActiveProfileId(id ?? null))
      .catch((err) => console.warn("[AgentsScreen] codex_active_profile:", err));
    refreshProxyModels();
    void refreshDreamSkinThemes();
    // 后台监控发现用户自己退出了 Codex（没点「停止」）：状态回落，配置已自动还原。
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen("codex-launch-changed", () => {
          setActiveProfileId(null);
          setLaunchMsg({
            ok: true,
            text: t("agents.launch.autoRestored"),
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
    const current = bound ? `${t("logs.allProviders")} (${bound})` : t("logs.allProviders");
    return t("agents.boundKeyWarning").replace("{current}", current);
  };
  // 未知档位（Codex 以后新增的）直接显示原始 effort，总好过显示空白。
  const reasoningLabel = (effort: string) =>
    REASONING_I18N[effort] ? t(REASONING_I18N[effort]) : effort;

  // 当前方案所选模型支持的档位。目录里没有该模型时后端会成功返回空数组，静默回退到保守四档；
  // 只有 IPC / 目录读取失败或响应格式异常时才显示失败提示。
  const [modelCatalog, setModelCatalog] = useState<CodexReasoningCatalogState>({
    model: "",
    levels: [],
    loading: false,
    error: false,
  });
  const draftModel = profileDraft?.model.trim() ?? "";
  const currentModelCatalog = modelCatalog.model === draftModel ? modelCatalog : null;
  const modelLevels = currentModelCatalog?.levels ?? [];
  const modelLevelsLoading = currentModelCatalog?.loading ?? false;
  const modelLevelsError = currentModelCatalog?.error ?? false;
  useEffect(() => {
    if (!draftModel) {
      setModelCatalog({ model: "", levels: [], loading: false, error: false });
      return;
    }
    let stale = false;
    const model = draftModel;
    setModelCatalog({ model, levels: [], loading: true, error: false });
    invoke<unknown>("fetch_codex_reasoning_levels", { model })
      .then((payload) => {
        if (stale) return;
        const levels = normalizeCodexReasoningLevels(payload);
        if (levels === null) {
          console.warn(
            "[AgentsScreen] fetch_codex_reasoning_levels returned malformed data:",
            Array.isArray(payload)
              ? "array contains a blank or non-string entry"
              : `expected an array, received ${typeof payload}`,
          );
          setModelCatalog({ model, levels: [], loading: true, error: true });
          return;
        }
        setModelCatalog({ model, levels, loading: true, error: false });
      })
      .catch((err) => {
        if (stale) return;
        console.warn("[AgentsScreen] fetch_codex_reasoning_levels:", err);
        setModelCatalog({ model, levels: [], loading: true, error: true });
      })
      .finally(() => {
        if (stale) return;
        setModelCatalog((current) => (
          current.model === model ? { ...current, loading: false } : current
        ));
      });
    return () => { stale = true; };
  }, [draftModel]);

  const reasoningOptions = useMemo(() => {
    const levels = modelLevels.length > 0 ? modelLevels : CODEX_REASONING_FALLBACK;
    const current = profileDraft?.reasoning?.trim();
    // 保底：方案里已选的档位若不在当前模型的支持列表里（换了模型、或目录取不到），也保留为
    // 选项——否则下拉显示空白，一保存就把它悄悄改掉了。
    const all = current && !levels.includes(current) ? [...levels, current] : levels;
    return all.map((effort) => ({ value: effort, label: reasoningLabel(effort) }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modelLevels, profileDraft?.reasoning, t]);

  function persistProfiles(
    next: CodexLaunchProfile[],
    options?: { allowClearCodexProfiles?: boolean },
  ) {
    onSaveSettings(
      { ...appState.settings, codex_profiles: next, remote_management_key: null },
      options,
    );
  }

  function openNewProfile() {
    setLaunchMsg(null);
    // 重拉模型:自定义接口刚改过(如新加了模型前缀)时,下拉才能看到 weiloo/gpt-5.5 这类新名字。
    refreshProxyModels();
    void refreshDreamSkinThemes();
    setProfileDraft({
      id: null,
      name: "",
      launch_mode: "app",
      dream_skin_enabled: false,
      dream_skin_theme_id: "dream",
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
    refreshProxyModels();
    void refreshDreamSkinThemes();
    setProfileDraft({
      id: profile.id,
      name: profile.name,
      launch_mode: profile.launch_mode || "app",
      dream_skin_enabled: profile.launch_mode === "cli" ? false : (profile.dream_skin_enabled ?? false),
      dream_skin_theme_id: profile.dream_skin_theme_id || "dream",
      bound_account: profile.bound_account,
      proxy_url: profile.proxy_url,
      model: profile.model,
      reasoning: profile.reasoning || "high",
      api_key: profile.api_key,
    });
  }

  async function importDreamSkinImage() {
    if (!("__TAURI_INTERNALS__" in window)) {
      setLaunchMsg({
        ok: false,
        text: t("agents.launch.dreamSkinDesktopOnly"),
      });
      return;
    }
    setLaunchMsg(null);
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: t("agents.launch.dreamSkinImageFilter"),
            extensions: ["png", "jpg", "jpeg", "webp"],
          },
        ],
      });
      if (!selected || Array.isArray(selected)) return;
      setDreamSkinImportBusy(true);
      const imported = await invoke<DreamSkinThemeSummary>("import_dream_skin_theme", {
        imagePath: selected,
        name: null,
      });
      setDreamSkinThemeCatalog((themes) => {
        const withoutImported = themes.filter((theme) => theme.id !== imported.id);
        return [...withoutImported, imported];
      });
      setProfileDraft((draft) =>
        draft ? { ...draft, dream_skin_theme_id: imported.id } : draft,
      );
      await refreshDreamSkinThemes();
      setLaunchMsg({
        ok: true,
        text: t("agents.themeImported").replace("{name}", imported.name),
      });
    } catch (err) {
      setLaunchMsg({
        ok: false,
        text: `${t("agents.launch.dreamSkinImportFailed")}：${String(err)}`,
      });
    } finally {
      setDreamSkinImportBusy(false);
    }
  }

  function submitProfileDraft() {
    if (!profileDraft) return;
    const name = profileDraft.name.trim();
    if (!name) {
      setLaunchMsg({ ok: false, text: t("agents.launch.needName") });
      return;
    }
    if (!profileDraft.bound_account.trim()) {
      setLaunchMsg({ ok: false, text: t("agents.launch.needAccount") });
      return;
    }
    const launchMode = profileDraft.launch_mode || "app";
    const saved: CodexLaunchProfile = {
      id: profileDraft.id ?? newProfileId(),
      name,
      launch_mode: launchMode,
      dream_skin_enabled: launchMode === "cli" ? false : profileDraft.dream_skin_enabled,
      dream_skin_theme_id: profileDraft.dream_skin_theme_id || "dream",
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
      setLaunchMsg({ ok: false, text: t("agents.launch.stopBeforeDelete") });
      return;
    }
    if (profileDraft?.id === profile.id) setProfileDraft(null);
    const next = codexProfiles.filter((item) => item.id !== profile.id);
    persistProfiles(next, {
      allowClearCodexProfiles: codexProfiles.length > 0 && next.length === 0,
    });
  }

  // 启动入口：收集"需要先确认"的事项（要切换正在跑的方案 / 密钥没绑到 Codex），
  // 有任意一条就先弹确认；都没有就直接起。
  function requestStartProfile(profile: CodexLaunchProfile) {
    setLaunchMsg(null);
    const warnings: string[] = [];
    if (activeProfileId && activeProfileId !== profile.id) {
      const current = codexProfiles.find((item) => item.id === activeProfileId);
      warnings.push(`会先停掉当前运行的「${current?.name ?? t("hardcoded.w068")}」再启动这套。`);
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
    setLaunchBusy(true);
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
      setLaunchBusy(false);
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
        setLaunchMsg({ ok: true, text: `${t("agents.launch.detected")}：${path}` });
      } else {
        setLaunchMsg({ ok: false, text: t("agents.launch.notDetected") });
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

  // 「当前启动方案」区:一行一套 scheme-card,运行中的高亮,其余可启动。保留全部逻辑
  // (启动/停止/编辑/删除/切换确认/密钥未绑警告/新建·编辑表单/应用路径探测/状态提示)。
  function codexLaunchPanel() {
    const running = (id: string) => id === activeProfileId;
    return (
      <section className="panel scheme-panel">
        <div className="scheme-head">
          <div>
            <h2 className="panel-title">{t("agents.launch.currentScheme")}</h2>
            <div className="app-path">
              {t("agents.launch.appPath")}：
              <code>
                {appState.settings.codex_app_path ||
                  t("agents.launch.appPathAuto")}
              </code>
              <button className="detect-link" type="button" onClick={() => void detectCodexApp()} disabled={launchBusy}>
                {t("agents.launch.detect")}
              </button>
            </div>
            <span className="scheme-note">{t("agents.launch.onlyOne")}</span>
          </div>
          <div className="rose-scheme-toolbar" aria-hidden="true">
            <span>{t("agents.launch.backupProfile")}</span>
            <span>{t("agents.launch.restoreProfile")}</span>
            <span>{t("agents.launch.resetProfile")}</span>
          </div>
          <button className="btn primary" type="button" onClick={openNewProfile}>
            <Icon name="plus" />
            {t("agents.launch.newProfile")}
          </button>
        </div>

        {codexProfiles.length === 0 && !profileDraft ? (
          <p className="scheme-empty">
            {t("agents.launch.emptyProfiles")}
          </p>
        ) : null}

        {codexProfiles.length > 0 ? (
          <div className="scheme-list">
            {codexProfiles.map((profile) => {
              const isRunning = running(profile.id);
              const isStarting = startingId === profile.id;
              const keyIssue = codexKeyIssue(profile);
              const pending = pendingStart?.profile.id === profile.id ? pendingStart : null;
              const flowEndpoint = (profile.proxy_url || appState.proxy.endpoint || "127.0.0.1:28317").replace(
                /^https?:\/\//,
                "",
              );
              const flowModel = profile.model || t("agents.launch.defaultModel");
              const flowKey = profile.api_key ? maskKey(profile.api_key) : t("agents.launch.autoKey");
              return (
                <div key={profile.id} className={`scheme-card${isRunning ? "" : " scheme-card--idle"}`}>
                  <div className="scheme-identity">
                    <span className={`play-orb${isRunning ? "" : " play-orb--idle"}`} aria-hidden="true">
                      <Icon name="play" />
                    </span>
                    <div>
                      <div className="agent-name-line">
                        <span className="scheme-name">{profile.name}</span>
                        {isRunning ? (
                          <span className="badge running">
                            <i className="tiny-dot" />
                            {t("agents.launch.running")}
                          </span>
                        ) : null}
                        {!isRunning && keyIssue ? (
                          <span className="badge warning" title={keyIssue}>
                            {t("agents.launch.keyWarnBadge")}
                          </span>
                        ) : null}
                        {isWindows && profile.launch_mode !== "cli" && (profile.dream_skin_enabled ?? false) ? (
                          <span className="badge dream-skin">
                            {t("agents.launch.dreamSkinBadge", "Dream Skin")} · {dreamSkinThemeLabel(profile.dream_skin_theme_id)}
                          </span>
                        ) : null}
                      </div>
                      <div className="scheme-sub">
                        {reasoningLabel(profile.reasoning)} ·{" "}
                        {profile.launch_mode === "cli" ? t("agents.launch.modeCli") : t("agents.launch.modeApp")}
                      </div>
                    </div>
                  </div>
                  <div className="scheme-field">
                    <div className="field-label"><Icon name="user" />{t("agents.launch.account")}</div>
                    <div className="field-value">{accountEmail(profile.bound_account)}</div>
                  </div>
                  <div className="scheme-field">
                    <div className="field-label"><Icon name="cube" />{t("agents.codexModel")}</div>
                    <div className="field-value">{profile.model || t("agents.launch.defaultModel")}</div>
                  </div>
                  <div className="scheme-field">
                    <div className="field-label"><Icon name="globe" />{t("agents.launch.localEndpoint")}</div>
                    <div className="field-value">{profile.proxy_url || t("agents.launch.localProxy")}</div>
                  </div>
                  <div className="scheme-field">
                    <div className="field-label"><Icon name="key" />{t("agents.apiKey")}</div>
                    <div className="field-value">{profile.api_key ? maskKey(profile.api_key) : t("agents.launch.autoKey")}</div>
                  </div>
                  <div className="rose-scheme-flow" aria-hidden="true">
                    <div className="rose-flow-node rose-flow-node--entry">
                      <span className="rose-flow-icon"><Icon name="play" /></span>
                      <span className="rose-flow-copy">
                        <strong>{t("agents.launch.entry")}</strong>
                        <small>{t("agents.launch.appRequest")}</small>
                      </span>
                    </div>
                    <span className="rose-flow-link" />
                    <div className="rose-flow-node">
                      <span className="rose-flow-icon"><Icon name="server" /></span>
                      <span className="rose-flow-copy">
                        <strong>{t("agents.launch.localProxy")}</strong>
                        <small>{flowEndpoint}</small>
                      </span>
                      {isRunning ? <em className="rose-flow-chip">{t("agents.connected")}</em> : null}
                    </div>
                    <span className="rose-flow-link" />
                    <div className="rose-flow-node">
                      <span className="rose-flow-icon"><Icon name="route" /></span>
                      <span className="rose-flow-copy">
                        <strong>{t("agents.launch.routeStrategy")}</strong>
                        <small>{profile.name}</small>
                      </span>
                      {isRunning ? <em className="rose-flow-status">{t("agents.launch.running")}</em> : null}
                    </div>
                    <span className="rose-flow-link" />
                    <div className="rose-flow-node">
                      <span className="rose-flow-icon"><Icon name="layers" /></span>
                      <span className="rose-flow-copy">
                        <strong>{t("agents.launch.modelProvider")}</strong>
                        <small>{flowModel}</small>
                      </span>
                    </div>
                    <span className="rose-flow-link" />
                    <div className="rose-flow-node">
                      <span className="rose-flow-icon"><Icon name="key" /></span>
                      <span className="rose-flow-copy">
                        <strong>{t("agents.apiKey")}</strong>
                        <small>{flowKey}</small>
                      </span>
                    </div>
                    <span className="rose-flow-link" />
                    <div className="rose-flow-node">
                      <span className="rose-flow-icon"><Icon name="cube" /></span>
                      <span className="rose-flow-copy">
                        <strong>{t("agents.launch.targetModel")}</strong>
                        <small>{flowModel}</small>
                      </span>
                    </div>
                  </div>
                  <div className="scheme-actions">
                    {isRunning ? (
                      <button
                        className="btn danger small"
                        type="button"
                        onClick={() => void stopActiveProfile()}
                        disabled={launchBusy}
                      >
                        <Icon name="stop" />
                        {launchBusy ? t("agents.launch.working") : t("agents.launch.stop")}
                      </button>
                    ) : (
                      <button
                        className="btn primary small"
                        type="button"
                        onClick={() => requestStartProfile(profile)}
                        disabled={isStarting || launchBusy}
                      >
                        <Icon name="play" />
                        {isStarting ? t("agents.launch.launching") : t("agents.launch.start")}
                      </button>
                    )}
                    <button
                      className="btn small"
                      type="button"
                      onClick={() => openEditProfile(profile)}
                      disabled={isRunning}
                    >
                      <Icon name="edit" />
                      {t("common.edit")}
                    </button>
                    <button
                      className="btn ghost-danger small"
                      type="button"
                      onClick={() => deleteProfile(profile)}
                      disabled={isRunning}
                    >
                      <Icon name="trash" />
                      {t("common.delete")}
                    </button>
                  </div>
                  {pending ? (
                    <div className="scheme-confirm">
                      <div className="scheme-warn-list">
                        {pending.warnings.map((warning, index) => (
                          <span key={index}>• {warning}</span>
                        ))}
                      </div>
                      <div className="scheme-confirm-actions">
                        <button className="btn primary small" type="button" onClick={() => void doStartProfile(profile)}>
                          {t("agents.launch.startAnyway")}
                        </button>
                        <button className="btn small" type="button" onClick={() => setPendingStart(null)}>
                          {t("common.cancel")}
                        </button>
                      </div>
                    </div>
                  ) : null}
                </div>
              );
            })}
          </div>
        ) : null}

        {profileDraft ? (
          <div ref={profileFormRef} className="scheme-form">
            <div className="settings-form-grid">
              <label>
                {t("agents.launch.profileName")}
                <input
                  value={profileDraft.name}
                  placeholder={t("agents.launch.profileNamePlaceholder")}
                  onChange={(event) => setProfileDraft((draft) => (draft ? { ...draft, name: event.target.value } : draft))}
                />
              </label>
              <label>
                {t("agents.launch.mode")}
                <Select
                  value={profileDraft.launch_mode}
                  options={[
                    { value: "app", label: t("agents.launch.modeApp") },
                    { value: "cli", label: t("agents.launch.modeCli") },
                  ]}
                  onChange={(value) => setProfileDraft((draft) => (draft ? { ...draft, launch_mode: value } : draft))}
                />
              </label>
              {isWindows && profileDraft.launch_mode === "app" ? (
                <div className="dream-skin-option">
                  <div className="dream-skin-copy">
                    <strong>{t("agents.launch.dreamSkin", "Dream Skin")}</strong>
                    <small>
                      {t(
                        "agents.launch.dreamSkinDesc",
                        t("hardcoded.w069"),
                      )}
                    </small>
                  </div>
                  <Switch
                    on={profileDraft.dream_skin_enabled}
                    label={t("agents.launch.dreamSkin", "Dream Skin")}
                    onChange={() =>
                      setProfileDraft((draft) =>
                        draft ? { ...draft, dream_skin_enabled: !draft.dream_skin_enabled } : draft,
                      )
                    }
                  />
                </div>
              ) : null}
              {isWindows && profileDraft.launch_mode === "app" && profileDraft.dream_skin_enabled ? (
                <label className="dream-skin-theme-field">
                  {t("agents.launch.dreamSkinTheme")}
                  <div className="dream-skin-theme-controls">
                    <Select
                      value={profileDraft.dream_skin_theme_id}
                      options={dreamSkinThemes}
                      onChange={(value) =>
                        setProfileDraft((draft) => (draft ? { ...draft, dream_skin_theme_id: value } : draft))
                      }
                    />
                    <button
                      className="btn small"
                      type="button"
                      disabled={dreamSkinImportBusy}
                      onClick={() => void importDreamSkinImage()}
                    >
                      {dreamSkinImportBusy
                        ? t("agents.launch.dreamSkinImporting")
                        : t("agents.launch.dreamSkinImport")}
                    </button>
                  </div>
                  <span className="codex-reasoning-status">
                    {t(
                      "agents.launch.dreamSkinThemeHint",
                      t("hardcoded.w070"),
                    )}
                  </span>
                </label>
              ) : null}
              <label>
                {t("agents.launch.account")}
                <Select
                  value={profileDraft.bound_account}
                  options={[
                    { value: "", label: t("agents.launch.accountPick") },
                    ...codexAccounts.map((account) => ({
                      value: account.key,
                      label: account.disabled ? `${account.email} (${t("agents.disabledSuffix")})` : account.email,
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
                  placeholder={t("agents.launch.apiKeyPlaceholder")}
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
                {t("agents.codexModel")}
                <Select
                  value={profileDraft.model}
                  options={[
                    { value: "", label: t("agents.unspecified") },
                    // 保底:方案里已选的模型若不在当前模型列表中(代理没跑 → 回退到内置列表,
                    // 或列表还没刷新到),也要保留为选项 —— 否则编辑时下拉显示空白,一保存就把
                    // 模型悄悄丢了。
                    ...(profileDraft.model.trim() && !codexModelList.includes(profileDraft.model.trim())
                      ? [{ value: profileDraft.model.trim(), label: profileDraft.model.trim() }]
                      : []),
                    ...codexModelList.map((model) => ({ value: model, label: model })),
                  ]}
                  onChange={(value) => setProfileDraft((draft) => (draft ? { ...draft, model: value } : draft))}
                />
              </label>
              <label>
                {t("agents.codexReasoning")}
                <Select
                  value={profileDraft.reasoning}
                  options={reasoningOptions}
                  onChange={(value) => setProfileDraft((draft) => (draft ? { ...draft, reasoning: value } : draft))}
                />
                {modelLevelsLoading ? (
                  <span className="codex-reasoning-status" role="status" aria-live="polite">
                    {t("agents.reasoningLoading")}
                  </span>
                ) : modelLevelsError ? (
                  <span className="codex-reasoning-status err" role="alert" aria-live="polite">
                    {t("agents.reasoningLoadFailed")}
                  </span>
                ) : null}
              </label>
            </div>
            <p className="codex-launch-hint">
              {t("agents.launch.accountHint")}
            </p>
            <div className="inline-actions">
              <button className="primary-action" type="button" onClick={submitProfileDraft}>
                {t("agents.launch.saveProfile")}
              </button>
              <button className="secondary-action" type="button" onClick={() => setProfileDraft(null)}>
                {t("common.cancel")}
              </button>
            </div>
          </div>
        ) : null}

        {launchMsg ? (
          <div className={`scheme-status ${launchMsg.ok ? "ok" : "err"}`}>{launchMsg.text}</div>
        ) : null}
      </section>
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
              {t("agents.codexModel")}
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

  // ---- 顶部指标 + 运行概览的派生数据(全部来自现有状态,不新增后端接口)----
  const detectedCount = sortedAgents.length;
  const runningCount = activeProfileId ? 1 : 0;
  const codexInstalled = installedAgents.some((status) => status.agent.id === "codex");
  const expandedConfigStatus =
    installedAgents.find((status) => status.agent.id !== "codex" && status.agent.id === expandedId) ?? null;
  const proxyRunning = appState.proxy.status === "running";
  const endpointHost = (appState.proxy.endpoint || "").replace(/^https?:\/\//, "");
  const activeProfile = codexProfiles.find((profile) => profile.id === activeProfileId) ?? null;

  return (
    <section className="section-page agents-page agents-redesign">
      <header className="page-topbar" data-tauri-drag-region>
        <h1 data-tauri-drag-region="false">{t("nav.agents")}</h1>
        <div className="rose-agents-portrait" aria-hidden="true">
          <img src="/rose/character-avatar.png" alt="" />
        </div>
        <button
          className={`btn${isBusy ? " is-busy" : ""}`}
          type="button"
          onClick={onRefreshAgents}
          disabled={isBusy}
          title={t("agents.refresh")}
          aria-label={t("agents.refresh")}
        >
          <Icon name="refresh" />
          {t("agents.refreshStatus")}
        </button>
      </header>

      <p className="page-subtitle">{t("agents.pageSubtitle")}</p>

      <section className="metrics" aria-label={t("agents.overview")}>
        <article className="metric">
          <div className="metric-icon"><Icon name="layers" /></div>
          <div>
            <div className="metric-label">{t("agents.metricDetected")}</div>
            <div className="metric-value">{detectedCount}</div>
          </div>
        </article>
        <article className="metric installed">
          <div className="metric-icon"><Icon name="package" /></div>
          <div>
            <div className="metric-label">{t("agents.installed")}</div>
            <div className="metric-value">{installedAgents.length}</div>
          </div>
        </article>
        <article className="metric configured">
          <div className="metric-icon"><Icon name="check" /></div>
          <div>
            <div className="metric-label">{t("agents.configured")}</div>
            <div className="metric-value">{configuredAgents.length}</div>
          </div>
        </article>
        <article className="metric running">
          <div className="metric-icon"><Icon name="play" /></div>
          <div>
            <div className="metric-label">{t("agents.launch.running")}</div>
            <div className="metric-value">{runningCount}</div>
          </div>
        </article>
      </section>

      <section className="top-grid">
        <article className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{t("agents.connectedTitle")}</h2>
            <span className="panel-note">
              {installedAgents.length} {t("agents.installedToolsUnit")}
            </span>
          </div>
          <div className="agent-list">
            {installedAgents.length === 0 ? (
              <p className="agent-empty">{t("agents.noneInstalled")}</p>
            ) : (
              installedAgents.map((status) => (
                <AgentCard
                  key={status.agent.id}
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
              ))
            )}
          </div>
          {installedAgents.length > 0 ? (
            <div className="panel-footer-link">
              <span>{t("agents.viewAllConnected")}</span>
              <span aria-hidden="true">›</span>
            </div>
          ) : null}
        </article>

        <article className="panel runtime-panel">
          <div className="panel-head">
            <h2 className="panel-title">{t("agents.runtimeTitle")}</h2>
            <span className="badge running">
              <i className="tiny-dot" />
              {t("agents.live")}
            </span>
          </div>
          <div className="runtime-list">
            <div className="runtime-row">
              <Icon name="server" />
              <span className="runtime-label">{t("agents.runtime.proxy")}</span>
              <span className={`runtime-value${proxyRunning ? " healthy" : ""}`}>
                <i className="tiny-dot" />
                {proxyRunning
                  ? (
                      <span className="runtime-value-copy">
                        <strong>{t("agents.runtime.proxyOk")}</strong>
                        {endpointHost ? <small>{endpointHost}</small> : null}
                      </span>
                    )
                  : t("agents.runtime.proxyDown")}
              </span>
            </div>
            <div className="runtime-row">
              <Icon name="cube" />
              <span className="runtime-label">{t("agents.runtime.currentAgent")}</span>
              <span className="runtime-value">
                <i className="tiny-dot" />
                {activeProfileId ? "Codex" : t("agents.runtime.none", "—")}
              </span>
            </div>
            <div className="runtime-row">
              <Icon name="play" />
              <span className="runtime-label">{t("agents.runtime.currentScheme")}</span>
              <span className="runtime-value">
                <i className="tiny-dot" />
                {activeProfile?.name ?? t("agents.runtime.none", "—")}
              </span>
            </div>
            <div className="runtime-row">
              <Icon name="route" />
              <span className="runtime-label">{t("agents.runtime.route")}</span>
              <span className={`runtime-value${proxyRunning ? " healthy" : ""}`}>
                {proxyRunning ? t("agents.runtime.ready") : t("agents.runtime.notReady")}
              </span>
            </div>
          </div>
          <img
            className="route-asset"
            src="/agents/route-flow.svg"
            alt={t("agents.runtime.routeAlt")}
          />
        </article>
      </section>

      {expandedConfigStatus ? (
        <section className="panel config-panel">
          <div className="panel-head">
            <h2 className="panel-title">
              {expandedConfigStatus.agent.display_name} · {t("agents.configure")}
            </h2>
            <button className="btn small" type="button" onClick={() => setExpandedId(null)}>
              {t("common.collapse")}
            </button>
          </div>
          {configForm(expandedConfigStatus)}
        </section>
      ) : null}

      {codexInstalled ? codexLaunchPanel() : null}

      {notInstalledAgents.length > 0 ? (
        <section className="panel discovery-panel">
          <div className="panel-head">
            <h2 className="panel-title">{t("agents.discoverTitle")}</h2>
            <span className="panel-note">{t("agents.discoverNote")}</span>
          </div>
          <div className="discovery-grid">
            {notInstalledAgents.map((status) => {
              const accent = agentAccent(status.agent.id);
              const initial = status.agent.display_name.trim().charAt(0).toUpperCase() || "?";
              return (
                <article className="discovery-card" key={status.agent.id}>
                  <span
                    className="discovery-mark"
                    style={{ color: `#${accent}`, background: `#${accent}14` }}
                    aria-hidden="true"
                  >
                    {initial}
                  </span>
                  <div className="discovery-info">
                    <strong>{status.agent.display_name}</strong>
                    <span>{t("agents.discoverDesc")}</span>
                  </div>
                  <button className="detect-btn" type="button" onClick={onRefreshAgents} disabled={isBusy}>
                    {t("agents.autoDetect")}
                  </button>
                </article>
              );
            })}
          </div>
          <div className="discovery-footer">
            <span>
              <Icon name="info" />
              {t("agents.discoverFooter")}
            </span>
            <span className="install-link">{t("agents.installGuide")}　›</span>
          </div>
        </section>
      ) : null}

      {action === "detect_agents" ? <p className="detecting-note">{t("agents.detecting")}</p> : null}
    </section>
  );
}

function AgentCard({
  status,
  accent,
  expanded = false,
  isBusy,
  onConfigure,
  onRepairVisibility,
  repairBusy = false,
}: {
  status: AgentStatus;
  accent: string;
  expanded?: boolean;
  isBusy: boolean;
  onConfigure?: () => void;
  onRepairVisibility?: () => void;
  repairBusy?: boolean;
}) {
  const t = useT();
  const initial = status.agent.display_name.trim().charAt(0).toUpperCase() || "?";

  return (
    <div className="agent-row">
      <span
        className="agent-mark"
        style={{ color: `#${accent}`, background: `#${accent}14`, borderColor: `#${accent}29` }}
        aria-hidden="true"
      >
        {initial}
      </span>
      <div className="agent-row-info">
        <div className="agent-name-line">
          <span className="agent-name">{status.agent.display_name}</span>
          {status.configured ? (
            <span className="badge success">{t("agents.configured")}</span>
          ) : (
            <>
              <span className="badge warning">{t("agents.installed")}</span>
              <span className="badge warning">{t("agents.needConfig")}</span>
            </>
          )}
        </div>
        {status.binary_path ? <div className="path">{status.binary_path}</div> : null}
      </div>
      <div className="agent-row-actions">
        {onRepairVisibility ? (
          <button className="btn small" type="button" onClick={onRepairVisibility} disabled={repairBusy}>
            {repairBusy ? t("agents.launch.repairingVisibility") : t("agents.launch.repairVisibility")}
          </button>
        ) : null}
        {onConfigure ? (
          <button className="btn small" type="button" onClick={onConfigure} disabled={isBusy}>
            {expanded ? t("common.collapse") : status.configured ? t("agents.reconfigure") : t("agents.configure")}
          </button>
        ) : null}
      </div>
    </div>
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
          <dd>{configuration.base_url ?? t("hardcoded.w071")}</dd>
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
          <dd>{result.auth_path ?? t("hardcoded.w072")}</dd>
        </div>
        <div>
          <dt>{t("agents.backup")}</dt>
          <dd>{result.backup_path ?? t("hardcoded.w073")}</dd>
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
          <h3>{t("hardcoded.w074")}</h3>
        </div>
        <button className="ghost-action" type="button" onClick={onRefresh} disabled={isBusy}>
          {t("common.refresh")}
        </button>
      </div>
      {backups.length === 0 ? (
        <p className="empty-copy">{t("hardcoded.w075")}</p>
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
                {t("common.restore")}
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
