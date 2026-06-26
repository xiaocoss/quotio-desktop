import { useEffect, useMemo, useState } from "react";
import type { AccountQuota, AppSettings, AppState, AuthFile, ProviderSummary, QuotaModelUsage } from "../../types";
import { maskEmail, quotaTone, parsePlan, parseResetCredits, planTier, matchAuthFile } from "../../lib/format";
import { RefreshIcon } from "../icons";
import { HealthDots } from "../HealthDots";
import { Switch } from "../Switch";
import { useT } from "../../i18n";
import { invoke } from "../../lib/tauri";

type CustomProviderBrief = {
  id: string;
  name: string;
  kind: string;
  keys: { id: string; label: string; api_key: string; enabled: boolean; weight: number }[];
};

type QuotaScreenProps = {
  appState: AppState;
  isManagementBusy: boolean;
  isQuotaBusy: boolean;
  managementAction: string | null;
  onRefreshManagement: () => void;
  onRefreshQuotas: () => void;
  onRunManagementStateAction: (command: string, args?: Record<string, unknown>) => void;
  onSaveSettings: (settings: AppSettings) => void;
};

type QuotaGroup = {
  id: string;
  label: string;
  colorHex: string;
  accounts: AccountQuota[];
};

export function QuotaScreen({ appState, isQuotaBusy, onRefreshQuotas, onSaveSettings }: QuotaScreenProps) {
  const t = useT();
  const groups = useMemo(() => buildGroups(appState.quotas, appState.providers), [appState.quotas, appState.providers]);
  const authFiles = appState.management.auth_files ?? appState.auth_files ?? [];
  const [customProviders, setCustomProviders] = useState<CustomProviderBrief[]>([]);
  useEffect(() => {
    void invoke<CustomProviderBrief[]>("list_custom_providers").then(setCustomProviders).catch((err) => console.warn("[QuotaScreen] list_custom_providers failed:", err));
  }, []);

  const [activeId, setActiveId] = useState<string | null>(null);
  const active = groups.find((group) => group.id === activeId) ?? groups[0] ?? null;
  const activeCustom = customProviders.find((cp) => `cp:${cp.id}` === activeId) ?? null;
  // Heuristic proxy-unreachable hint: a refresh finished but every account came
  // back blank (no quota, not exhausted, not auth-failed) — almost always the
  // upstream proxy being wrong/down rather than a real per-account state.
  const proxyUnreachable =
    !isQuotaBusy &&
    appState.quotas.length > 0 &&
    appState.quotas.every(
      (account) => account.models.length === 0 && !account.is_forbidden && account.status_message !== "auth_failed",
    );

  return (
    <section className="section-page quota-page">
      <header className="page-topbar" data-tauri-drag-region>
        <h1>{t("nav.quota")}</h1>
        <button
          className="icon-button"
          type="button"
          onClick={onRefreshQuotas}
          disabled={isQuotaBusy}
          title="刷新额度"
          aria-label="刷新额度"
        >
          <RefreshIcon />
        </button>
      </header>

      <SchedulerCard appState={appState} onSaveSettings={onSaveSettings} activeProviderId={active?.id ?? null} />

      {proxyUnreachable ? (
        <div className="state-banner state-banner--warn">
          <strong>{t("quota.proxyUnreachable.title", "未获取到额度")}</strong>
          <p>{t("quota.proxyUnreachable.desc", "代理可能不通——请检查「设置」里的代理地址,以及代理是否已启动。")}</p>
        </div>
      ) : null}

      {groups.length === 0 && customProviders.length === 0 ? (
        <div className="state-banner state-banner--warn">
          <strong>{t("quota.empty.title")}</strong>
          <p>{t("quota.empty.desc")}</p>
        </div>
      ) : (
        <>
          <div className="quota-tabs" role="tablist">
            {groups.map((group) => {
              const isActive = active?.id === group.id && !activeCustom;
              return (
                <button
                  key={group.id}
                  type="button"
                  role="tab"
                  aria-selected={isActive}
                  className={isActive ? "quota-tab quota-tab--active" : "quota-tab"}
                  style={isActive ? { borderColor: `#${group.colorHex}59`, background: `#${group.colorHex}14` } : undefined}
                  onClick={() => setActiveId(group.id)}
                >
                  <span className="quota-tab-dot" style={{ backgroundColor: `#${group.colorHex}` }} />
                  <span className="quota-tab-name">{group.label}</span>
                  <span className="quota-tab-count">{group.accounts.length}</span>
                </button>
              );
            })}
            {customProviders.map((cp) => {
              const tabId = `cp:${cp.id}`;
              const isActive = activeId === tabId;
              const color = cp.kind === "gemini" ? "#4285F4" : cp.kind === "claude" ? "#D97757" : "#10a37f";
              const enabledCount = cp.keys.filter((k) => k.enabled).length;
              return (
                <button
                  key={tabId}
                  type="button"
                  role="tab"
                  aria-selected={isActive}
                  className={isActive ? "quota-tab quota-tab--active" : "quota-tab"}
                  style={isActive ? { borderColor: `${color}59`, background: `${color}14` } : undefined}
                  onClick={() => setActiveId(tabId)}
                >
                  <span className="quota-tab-dot" style={{ backgroundColor: color }} />
                  <span className="quota-tab-name">{cp.name}</span>
                  <span className="quota-tab-count">{enabledCount}/{cp.keys.length}</span>
                </button>
              );
            })}
          </div>

          {activeCustom ? (
            <CustomProviderKeyPool provider={activeCustom} />
          ) : (
          <div className="quota-accounts">
            {active?.accounts.map((account) => (
              <AccountQuotaCard
                key={account.account_key}
                account={account}
                authFiles={authFiles}
                onRefreshQuotas={onRefreshQuotas}
              />
            ))}
          </div>
          )}
        </>
      )}
    </section>
  );
}

/// 智能调度卡片:开关 + 当前选号状态。覆盖所有服务商。
function SchedulerCard({
  appState,
  onSaveSettings,
  activeProviderId,
}: {
  appState: AppState;
  onSaveSettings: (settings: AppSettings) => void;
  activeProviderId: string | null;
}) {
  const t = useT();
  const scheduler = appState.scheduler;
  const schedulerOn = (appState.settings.scheduler_rule || "off") !== "off";

  const providerEntry = useMemo(() => {
    if (!activeProviderId || !scheduler?.providers) return null;
    return scheduler.providers.find((e) => e.provider_id === activeProviderId) ?? null;
  }, [activeProviderId, scheduler?.providers]);

  const providerLabel = useMemo(() => {
    if (!activeProviderId) return "";
    const p = appState.providers.find((p) => p.id === activeProviderId);
    return p?.display_name ?? activeProviderId;
  }, [activeProviderId, appState.providers]);

  function toggleScheduler() {
    onSaveSettings({
      ...appState.settings,
      scheduler_rule: schedulerOn ? "off" : "reset_soonest",
      remote_management_key: null,
    });
  }

  let resetText: string | null = null;
  const resetAtUnix = providerEntry?.target_reset_at_unix ?? scheduler?.target_reset_at_unix;
  if (resetAtUnix) {
    const secs = resetAtUnix - Math.floor(Date.now() / 1000);
    if (secs > 0) {
      const hours = Math.floor(secs / 3600);
      const minutes = Math.max(1, Math.floor((secs % 3600) / 60));
      resetText = hours > 0 ? `${hours}h ${minutes}m` : `${minutes}m`;
    }
  }

  const activeLabel = providerEntry?.target_label ?? scheduler?.target_label;
  const activeStandby = providerEntry?.standby_count ?? scheduler?.standby_count ?? 0;
  const totalScheduled = scheduler?.providers?.length ?? 0;

  let statusText: string;
  if (!schedulerOn) {
    statusText = t(
      "quota.scheduler.descOff",
      "When enabled, only accounts with the closest refresh time enter the proxy pool, while others wait; automatically switches accounts when time is up so quotas aren't wasted.",
    );
  } else if (activeLabel) {
    const windowText = resetText
      ? t("quota.scheduler.resetIn", "{time} 后刷新").replace("{time}", resetText)
      : t("quota.scheduler.idleWindow", "闲置窗口(使用后开新窗口)");
    const standby = t("quota.scheduler.standby", "待命 {count} 个").replace(
      "{count}",
      String(activeStandby),
    );
    statusText = `${t("quota.scheduler.current", "当前选中")}:${activeLabel} · ${windowText} · ${standby}`;
  } else if (totalScheduled > 0) {
    statusText = t("quota.scheduler.noProviderMatch", "此服务商账号不足 2 个,无需调度。");
  } else {
    statusText = t("quota.scheduler.pending", "已开启,等待下一次额度刷新后选号(可点右上角立即刷新)。");
  }

  const tagText = schedulerOn && totalScheduled > 0
    ? t("quota.scheduler.rule", "Refresh closest priority") + (providerEntry ? ` · ${providerLabel}` : ` · Total ${totalScheduled} providers`)
    : t("quota.scheduler.ruleGeneric", "Refresh closest priority");

  return (
    <div className="scheduler-block">
      <div className="scheduler-head">
        <strong>{t("quota.scheduler.title", "智能调度")}</strong>
        <span className="scheduler-tag">{tagText}</span>
        <div className="scheduler-switch">
          <Switch on={schedulerOn} onChange={toggleScheduler} label="scheduler" />
        </div>
      </div>
      <p className="scheduler-desc">{statusText}</p>
    </div>
  );
}

function AccountQuotaCard({
  account,
  authFiles,
  onRefreshQuotas,
}: {
  account: AccountQuota;
  authFiles: AuthFile[];
  onRefreshQuotas: () => void;
}) {
  const t = useT();
  // The Codex fetcher encodes the subscription tier + expiry + reset credits into
  // status_message as "plan: <tier> | until: <YYYY-MM-DD> | resets: <N>"; surface
  // them as a badge + date + the "主动重置次数" pill.
  const statusMessage = account.status_message ?? "";
  const plan = parsePlan(statusMessage);
  const expiry = statusMessage.match(/until:\s*([^|]+)/i)?.[1]?.trim();
  // 主动重置次数: only Codex reports it; null means "not a Codex account / absent".
  const isCodex = account.provider_id === "codex";
  const resetCredits = isCodex ? parseResetCredits(statusMessage) : null;
  const hasResetCredits = resetCredits != null;
  const [resetting, setResetting] = useState(false);
  const [confirmReset, setConfirmReset] = useState(false);
  const [resetError, setResetError] = useState<string | null>(null);

  // Two-step inline confirm (matches the app's "清空" pattern): first click arms
  // it, second click within 4s spends a credit and force-resets the 5h window.
  async function handleReset() {
    if (resetting) return;
    if (!confirmReset) {
      setConfirmReset(true);
      window.setTimeout(() => setConfirmReset(false), 4000);
      return;
    }
    setConfirmReset(false);
    setResetting(true);
    setResetError(null);
    try {
      await invoke("consume_codex_reset_credit", { accountKey: account.account_key });
      onRefreshQuotas();
    } catch (error) {
      setResetError(typeof error === "string" ? error : t("quota.resetFailed", "重置失败"));
    } finally {
      setResetting(false);
    }
  }
  // Codex accounts whose 401 couldn't be refreshed are flagged "auth_failed" by
  // the backend — mark them here too (matches the Providers list).
  const authFailed = statusMessage === "auth_failed";
  // Each tier gets its own badge color; Plus is the base style.
  const tier = plan ? planTier(plan) : null;
  const planClass =
    tier && tier !== "plus"
      ? `quota-type-pill quota-plan-pill quota-plan-pill--${tier}`
      : "quota-type-pill quota-plan-pill";
  const file = matchAuthFile(account, authFiles);
  const isCodexLoginOnly = file?.quotio_bound_login_only === true;
  const isSchedulerStandby = file?.quotio_scheduler_standby === true && file?.disabled === true;
  const recent = file?.recent_requests ?? [];
  return (
    <article className="panel quota-card">
      <div className="quota-card-head">
        {plan ? (
          <span className={planClass}>{plan.toUpperCase()}</span>
        ) : account.account_type ? (
          <span className="quota-type-pill">{account.account_type}</span>
        ) : null}
        <span className="quota-account-label">{maskEmail(account.account_label)}</span>
        <div className="quota-card-actions">
          {expiry ? (
            <span className="quota-expiry">
              {t("quota.expires")} {expiry}
            </span>
          ) : null}
          {authFailed ? <span className="quota-pill quota-pill--bad">{t("providers.stateNeedsReauth", "需重新授权")}</span> : null}
          {account.is_forbidden ? <span className="quota-pill quota-pill--bad">{t("quota.forbidden")}</span> : null}
          {/* Weekly window maxed but the account still serves via the session
              window — a soft heads-up, not the alarming "exhausted" pill. */}
          {!account.is_forbidden &&
          account.models.some((model) => /weekly/i.test(model.model) && model.remaining_percent <= 0) ? (
            <span className="quota-pill quota-pill--warn">{t("quota.weeklyUsedUp", "本周已用尽")}</span>
          ) : null}
          {account.warming_up ? <span className="quota-pill quota-pill--warn">{t("quota.warmup")}</span> : null}
          {account.in_use ? <span className="quota-pill quota-pill--blue">{t("quota.useInIde")}</span> : null}
          {isCodexLoginOnly ? (
            <span
              className="quota-pill quota-pill--blue"
              title={t("quota.codexLoginOnly.desc", "该账号仅用于启动 Codex，不参与 Quotio 代理池调用。")}
            >
              {t("quota.codexLoginOnly", "Codex 登录专用")}
            </span>
          ) : null}
          {isSchedulerStandby ? (
            <span
              className="quota-pill quota-pill--blue"
              title={t("quota.schedulerStandby.desc", "Temporarily removed from the proxy pool by the smart scheduler; automatically restored when its turn comes or scheduling is disabled.")}
            >
              {t("quota.schedulerStandby", "调度待命")}
            </span>
          ) : null}
        </div>
      </div>

      {recent.length > 0 ? (
        <div className="quota-health">
          <div className="quota-health-head">
            <span>{t("quota.health", "健康状态")}</span>
            <span className="quota-health-counts">
              ✓{file?.success ?? 0} ·{" "}
              <span className={(file?.failed ?? 0) > 0 ? "quota-health-fail" : undefined}>✗{file?.failed ?? 0}</span>
            </span>
          </div>
          <HealthDots recent={recent} />
        </div>
      ) : null}

      {account.models.length > 0 ? (
        <>
          <div className="quota-usage-head">
            <span>{t("quota.usage")}</span>
            {hasResetCredits ? (
              <span className="quota-reset-group">
                <span className="quota-reset-credits" title={t("quota.resetCredits", "主动重置次数")}>
                  {t("quota.resetCredits", "主动重置次数")} {resetCredits}
                </span>
                <button
                  type="button"
                  className={confirmReset ? "quota-reset-button quota-reset-button--confirm" : "quota-reset-button"}
                  disabled={resetting || (resetCredits ?? 0) <= 0}
                  onClick={handleReset}
                  title={
                    (resetCredits ?? 0) <= 0
                      ? t("quota.resetNoCredits", "没有可用的主动重置次数")
                      : t("quota.resetButton", "重置")
                  }
                >
                  {resetting
                    ? t("quota.resetting", "重置中…")
                    : confirmReset
                      ? t("quota.resetConfirm", "确认重置?")
                      : t("quota.resetButton", "重置")}
                </button>
              </span>
            ) : null}
          </div>
          {resetError ? <p className="quota-reset-error">{resetError}</p> : null}
          <div className="quota-models">
            {account.models.map((model) => (
              <ModelQuotaRow key={model.model} model={model} />
            ))}
          </div>
        </>
      ) : (
        <p className="quota-empty-note">
          {authFailed
            ? t("quota.needsReauthNote", "需重新授权,请到服务商页重新登录")
            : t("quota.fetchFailed", "额度获取失败,仅显示健康状态")}
        </p>
      )}
    </article>
  );
}

function ModelQuotaRow({ model }: { model: QuotaModelUsage }) {
  const t = useT();
  const tone = quotaTone(model.remaining_percent);

  return (
    <div className="quota-model">
      <div className="quota-model-head">
        <span className="quota-model-name">{model.model}</span>
        {model.count != null ? <span className="quota-model-count">{model.count}</span> : null}
        <span className={`quota-model-left quota-model-left--${tone}`}>{Math.round(model.remaining_percent)}% {t("quota.left")}</span>
        {model.reset_at ? <span className="quota-model-reset">{model.reset_at}</span> : null}
      </div>
      <div className="quota-bar">
        <div className={`quota-bar-fill quota-bar-fill--${tone}`} style={{ width: `${Math.max(0, Math.min(100, model.remaining_percent))}%` }} />
      </div>
    </div>
  );
}

function CustomProviderKeyPool({ provider }: { provider: CustomProviderBrief }) {
  const enabledKeys = provider.keys.filter((k) => k.enabled);
  const disabledKeys = provider.keys.filter((k) => !k.enabled);
  function maskKey(key: string): string {
    if (key.length <= 8) return "****";
    return key.slice(0, 4) + "****" + key.slice(-4);
  }
  return (
    <div className="quota-accounts">
      <div className="panel cp-quota-panel">
        <div className="panel-label">
          <span className="eyebrow">{provider.name}</span>
          <span className="count-pill">{enabledKeys.length}/{provider.keys.length} 启用</span>
        </div>
        {provider.keys.length === 0 ? (
          <p className="empty-copy">该服务商暂无密钥。前往「服务商」页面添加。</p>
        ) : (
          <div className="cp-quota-key-list">
            {enabledKeys.map((k) => (
              <div className="cp-quota-key-row" key={k.id}>
                <span className="cp-key-dot cp-key-dot--on" />
                <span className="cp-quota-key-label">{k.label || "未命名"}</span>
                <code className="cp-quota-key-value">{maskKey(k.api_key)}</code>
                {k.weight !== 1 ? <span className="cp-quota-key-weight">权重 {k.weight}</span> : null}
              </div>
            ))}
            {disabledKeys.map((k) => (
              <div className="cp-quota-key-row cp-quota-key-row--disabled" key={k.id}>
                <span className="cp-key-dot" />
                <span className="cp-quota-key-label">{k.label || "未命名"}</span>
                <code className="cp-quota-key-value">{maskKey(k.api_key)}</code>
                <span className="cp-quota-key-badge">已禁用</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function buildGroups(quotas: AccountQuota[], providers: ProviderSummary[]): QuotaGroup[] {
  const groups: QuotaGroup[] = [];
  const index = new Map<string, number>();

  for (const quota of quotas) {
    let position = index.get(quota.provider_id);
    if (position === undefined) {
      const provider = providers.find((item) => item.id === quota.provider_id || item.id.includes(quota.provider_id));
      position = groups.length;
      index.set(quota.provider_id, position);
      groups.push({
        id: quota.provider_id,
        label: provider?.display_name ?? quota.provider_id,
        colorHex: provider?.color_hex ?? "8a8a8e",
        accounts: [],
      });
    }
    groups[position].accounts.push(quota);
  }

  return groups;
}
