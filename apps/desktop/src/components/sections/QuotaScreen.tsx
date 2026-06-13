import { useMemo, useState } from "react";
import type { AccountQuota, AppSettings, AppState, AuthFile, ProviderSummary, QuotaModelUsage } from "../../types";
import { maskEmail, quotaTone, parsePlan, parseResetCredits, planTier, matchAuthFile } from "../../lib/format";
import { RefreshIcon } from "../icons";
import { HealthDots } from "../HealthDots";
import { Switch } from "../Switch";
import { useT } from "../../i18n";
import { invoke } from "../../lib/tauri";

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
  const authFiles = appState.auth_files ?? [];
  const [activeId, setActiveId] = useState<string | null>(null);
  const active = groups.find((group) => group.id === activeId) ?? groups[0] ?? null;
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

      <SchedulerCard appState={appState} onSaveSettings={onSaveSettings} />

      {proxyUnreachable ? (
        <div className="state-banner state-banner--warn">
          <strong>{t("quota.proxyUnreachable.title", "未获取到额度")}</strong>
          <p>{t("quota.proxyUnreachable.desc", "代理可能不通——请检查「设置」里的代理地址,以及代理是否已启动。")}</p>
        </div>
      ) : null}

      {groups.length === 0 ? (
        <div className="state-banner state-banner--warn">
          <strong>{t("quota.empty.title")}</strong>
          <p>{t("quota.empty.desc")}</p>
        </div>
      ) : (
        <>
          <div className="quota-tabs" role="tablist">
            {groups.map((group) => {
              const isActive = active?.id === group.id;
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
          </div>

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
        </>
      )}
    </section>
  );
}

/// 智能调度卡片:开关 + 当前选号状态。规则「临近刷新优先」——只让 5h 窗口
/// 最快刷新的 Codex 账号进代理池,其余临时待命,窗口到点自动换号。
function SchedulerCard({
  appState,
  onSaveSettings,
}: {
  appState: AppState;
  onSaveSettings: (settings: AppSettings) => void;
}) {
  const t = useT();
  const scheduler = appState.scheduler;
  const schedulerOn = (appState.settings.scheduler_rule || "off") !== "off";

  function toggleScheduler() {
    onSaveSettings({
      ...appState.settings,
      scheduler_rule: schedulerOn ? "off" : "reset_soonest",
      remote_management_key: null,
    });
  }

  // 5h 窗口刷新倒计时(给人看的粗粒度文本)。
  let resetText: string | null = null;
  if (scheduler?.target_reset_at_unix) {
    const secs = scheduler.target_reset_at_unix - Math.floor(Date.now() / 1000);
    if (secs > 0) {
      const hours = Math.floor(secs / 3600);
      const minutes = Math.max(1, Math.floor((secs % 3600) / 60));
      resetText = hours > 0 ? `${hours}h ${minutes}m` : `${minutes}m`;
    }
  }

  let statusText: string;
  if (!schedulerOn) {
    statusText = t(
      "quota.scheduler.descOff",
      "开启后只让「5h 窗口最快刷新」的 Codex 账号进代理池,其余临时待命;窗口到点自动换号,余量不浪费。",
    );
  } else if (scheduler?.target_label) {
    const windowText = resetText
      ? t("quota.scheduler.resetIn", "5h 窗口 {time} 后刷新").replace("{time}", resetText)
      : t("quota.scheduler.idleWindow", "闲置窗口(使用后开新 5h 窗口)");
    const standby = t("quota.scheduler.standby", "待命 {count} 个").replace(
      "{count}",
      String(scheduler.standby_count),
    );
    statusText = `${t("quota.scheduler.current", "当前选中")}:${scheduler.target_label} · ${windowText} · ${standby}`;
  } else {
    statusText = t("quota.scheduler.pending", "已开启,等待下一次额度刷新后选号(可点右上角立即刷新)。");
  }

  return (
    <div className="scheduler-block">
      <div className="scheduler-head">
        <strong>{t("quota.scheduler.title", "智能调度")}</strong>
        <span className="scheduler-tag">{t("quota.scheduler.rule", "临近刷新优先 · 仅 Codex")}</span>
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
              title={t("quota.schedulerStandby.desc", "被智能调度临时移出代理池;轮到它或关闭调度时自动恢复。")}
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
