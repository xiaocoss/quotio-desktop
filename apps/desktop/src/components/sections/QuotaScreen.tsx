import { useCallback, useEffect, useMemo, useState } from "react";
import type { AccountQuota, AppSettings, AppState, AuthFile, ProviderSummary, QuotaModelUsage, SchedulerOrderItem } from "../../types";
import { maskEmail, quotaTone, parsePlan, parseResetCredits, planTier, matchAuthFile, servingFile } from "../../lib/format";
import { RefreshIcon } from "../icons";
import { HealthDots } from "../HealthDots";
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

export function QuotaScreen({ appState, isQuotaBusy, onRefreshQuotas, onSaveSettings, onRunManagementStateAction }: QuotaScreenProps) {
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

  // 排序型调度(智能调度 / 顺序故障转移)算出的请求顺序:file_name → 顺序项。用来把额度
  // 卡片按该顺序排 + 画圆圈序号徽章;关闭调度时为空,卡片保持原序、无徽章。
  const orderByFile = useMemo(() => {
    const map = new Map<string, SchedulerOrderItem>();
    const sched = appState.scheduler;
    if (sched && (sched.rule === "reset_soonest" || sched.rule === "priority_failover")) {
      for (const entry of sched.providers ?? []) {
        const order = entry.order ?? [];
        // 「主用」高亮跟着真正在服务(近期成功最多)的号走;无近期流量时保留后端 active。
        const serving = servingFile(order.map((i) => i.file_name), authFiles);
        for (const item of order) {
          map.set(item.file_name, serving ? { ...item, active: item.file_name === serving } : item);
        }
      }
    }
    return map;
  }, [appState.scheduler, authFiles]);

  const orderForAccount = useCallback(
    (account: AccountQuota): SchedulerOrderItem | null => {
      if (orderByFile.size === 0) return null;
      const file = matchAuthFile(account, authFiles);
      return (file && orderByFile.get(file.name)) || null;
    },
    [orderByFile, authFiles],
  );

  // 故障转移 / 智能调度开启时,额度卡片按请求顺序排(无序号的绑定号垫后)。每账号只匹配
  // 一次、预存位置,避免在比较器里反复跑 matchAuthFile。
  const sortedAccounts = useMemo(() => {
    const accounts = active?.accounts ?? [];
    if (orderByFile.size === 0) return accounts;
    const positionByKey = new Map(
      accounts.map((a) => [a.account_key, orderForAccount(a)?.position ?? Number.MAX_SAFE_INTEGER]),
    );
    return [...accounts].sort(
      (a, b) =>
        (positionByKey.get(a.account_key) ?? Number.MAX_SAFE_INTEGER) -
        (positionByKey.get(b.account_key) ?? Number.MAX_SAFE_INTEGER),
    );
  }, [active, orderByFile, orderForAccount]);
  // Heuristic proxy-unreachable hint: a refresh finished but every account came
  // back blank (no quota, not exhausted, not auth-failed) — almost always the
  // upstream proxy being wrong/down rather than a real per-account state.
  // Only codex lists a probe-failed account as a present blank (other providers
  // return None → absent), so a non-codex blank now means "healthy, no usage
  // data", NOT a dead proxy — it must not trip this hint.
  const proxyUnreachable =
    !isQuotaBusy &&
    appState.quotas.length > 0 &&
    appState.quotas.every(
      (account) =>
        account.provider_id === "codex" &&
        account.models.length === 0 &&
        !account.is_forbidden &&
        account.status_message !== "auth_failed",
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

      <SchedulerCard appState={appState} onSaveSettings={onSaveSettings} onRunManagementStateAction={onRunManagementStateAction} activeProviderId={active?.id ?? null} />

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
            {sortedAccounts.map((account) => (
              <AccountQuotaCard
                key={account.account_key}
                account={account}
                authFiles={authFiles}
                order={orderForAccount(account)}
                colorHex={active?.colorHex ?? "8a8a8e"}
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
  onRunManagementStateAction,
  activeProviderId,
}: {
  appState: AppState;
  onSaveSettings: (settings: AppSettings) => void;
  onRunManagementStateAction: (command: string, args?: Record<string, unknown>) => void;
  activeProviderId: string | null;
}) {
  const t = useT();
  const scheduler = appState.scheduler;
  const rule = appState.settings.scheduler_rule || "off";
  const schedulerOn = rule !== "off";

  const providerEntry = useMemo(() => {
    if (!activeProviderId || !scheduler?.providers) return null;
    return scheduler.providers.find((e) => e.provider_id === activeProviderId) ?? null;
  }, [activeProviderId, scheduler?.providers]);

  const providerLabel = useMemo(() => {
    if (!activeProviderId) return "";
    const p = appState.providers.find((p) => p.id === activeProviderId);
    return p?.display_name ?? activeProviderId;
  }, [activeProviderId, appState.providers]);

  function selectMode(mode: "off" | "reset_soonest" | "priority_failover") {
    if (mode === rule) return;
    const isFailover = mode === "priority_failover";
    // 顺序故障转移要让代理按 attributes.priority 顺位用号 → fill-first;其余模式回 round-robin。
    // 同时该模式要关掉冷却(坏号只临时绕过、不被惩罚性冷落,否则一次 5xx 就把高优先级号锁死最长
    // 30 分钟)——「生效冷却」由后端按 scheduler_rule 派生(failover 即关 = 用户手动 OR failover),
    // 不在这里写 disable_cooling,免得覆盖用户在「设置」里的手动开关。
    const nextRouting = isFailover ? "fill-first" : "round-robin";
    // 进 / 出失败转移这条边界:fill-first 路由 + 关冷却都只能靠 config.yaml + 重启生效,save_settings
    // 后端正是在这条边界上重启代理、一次性应用两者。所以这种切换把路由交给那次重启(别再单独热推、
    // 免得和重启抢)。判定必须和后端**完全一致**(同为「是否跨 failover 边界」),否则会出现「前端
    // 以为后端重启而跳过推路由、后端却不重启 → 路由永不生效」。
    const failoverChanged = isFailover !== (rule === "priority_failover");
    onSaveSettings({
      ...appState.settings,
      scheduler_rule: mode,
      routing_strategy: nextRouting,
      remote_management_key: null,
    });
    // 只有不跨 failover 边界(off ↔ 智能调度,后端不会重启)时,才按需把路由热推给活代理。
    // 代理没运行时不推、按活代理真实路由比对以免漏推。
    const liveConfig = appState.management.config;
    if (!failoverChanged && liveConfig && liveConfig.routing_strategy !== nextRouting) {
      onRunManagementStateAction("set_management_routing_strategy", { strategy: nextRouting });
    }
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

  // 「主用」跟着真正在服务(近期成功 √ 最多)的号:后端 target_label 是优先级最高的启用号,
  // 但它可能正被上游抖动临时绕过。无近期流量时回退后端值。
  const schedAuthFiles = appState.management.auth_files ?? appState.auth_files ?? [];
  const servingName = providerEntry
    ? servingFile((providerEntry.order ?? []).map((i) => i.file_name), schedAuthFiles)
    : null;
  const servingLabel = servingName
    ? (providerEntry?.order ?? []).find((i) => i.file_name === servingName)?.label ?? null
    : null;
  const activeLabel = servingLabel ?? providerEntry?.target_label ?? scheduler?.target_label;
  const activeStandby = providerEntry?.standby_count ?? scheduler?.standby_count ?? 0;
  const totalScheduled = scheduler?.providers?.length ?? 0;

  // 当前服务商有数据就只数它自己(order 缺失算 0);没有选中服务商时才退回汇总所有服务商,
  // 避免「providerEntry 在、但 order 缺失」时 ?? 跳到跨服务商求和、报出虚高的数字。
  const failoverChainCount = providerEntry
    ? providerEntry.order?.filter((o) => o.eligible).length ?? 0
    : (scheduler?.providers ?? []).reduce(
        (n, p) => n + (p.order?.filter((o) => o.eligible).length ?? 0),
        0,
      );

  let statusText: string;
  if (!schedulerOn) {
    statusText = t(
      "quota.scheduler.descOff",
      "智能调度:只让「额度最快刷新」的号进池,余者待命、到点自动换号。顺序故障转移:按你排的顺序用号,坏一个无感切下一个(不报错)。",
    );
  } else if (rule === "priority_failover") {
    if (activeLabel) {
      const chain = t(
        "quota.scheduler.failoverChain",
        "共 {count} 个按序待命,坏一个自动顺延(不返回错误)",
      ).replace("{count}", String(failoverChainCount));
      statusText = `${t("quota.scheduler.failoverPrimary", "主用")}:${activeLabel} · ${chain}`;
    } else if (totalScheduled > 0) {
      statusText = t("quota.scheduler.noProviderMatch", "此服务商账号不足 2 个,无需调度。");
    } else {
      statusText = t(
        "quota.scheduler.failoverPending",
        "已开启,按你设定的账号顺序请求,坏号自动顺延到下一个(在「服务商」页拖动排序)。",
      );
    }
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

  let tagText: string | null = null;
  if (rule === "priority_failover") {
    tagText =
      t("quota.scheduler.ruleFailover", "按序故障转移") +
      (providerEntry ? ` · ${providerLabel}` : totalScheduled > 0 ? ` · ${totalScheduled} 个服务商` : "");
  } else if (rule === "reset_soonest") {
    tagText =
      t("quota.scheduler.rule", "临近刷新优先") +
      (providerEntry ? ` · ${providerLabel}` : totalScheduled > 0 ? ` · ${totalScheduled} 个服务商` : "");
  }

  const modes: { id: "off" | "reset_soonest" | "priority_failover"; label: string }[] = [
    { id: "off", label: t("quota.scheduler.modeOff", "关闭") },
    { id: "reset_soonest", label: t("quota.scheduler.modeResetSoonest", "智能调度") },
    { id: "priority_failover", label: t("quota.scheduler.modeFailover", "顺序故障转移") },
  ];

  return (
    <div className="scheduler-block">
      <div className="scheduler-head">
        <strong>{t("quota.scheduler.cardTitle", "账号调度")}</strong>
        {tagText && <span className="scheduler-tag">{tagText}</span>}
        <div
          className="scheduler-modes"
          role="group"
          aria-label={t("quota.scheduler.cardTitle", "账号调度")}
        >
          {modes.map((m) => (
            <button
              key={m.id}
              type="button"
              className={`scheduler-mode-btn${rule === m.id ? " is-active" : ""}`}
              onClick={() => selectMode(m.id)}
            >
              {m.label}
            </button>
          ))}
        </div>
      </div>
      <p className="scheduler-desc">{statusText}</p>
    </div>
  );
}

function AccountQuotaCard({
  account,
  authFiles,
  order,
  colorHex,
  onRefreshQuotas,
}: {
  account: AccountQuota;
  authFiles: AuthFile[];
  order?: SchedulerOrderItem | null;
  colorHex?: string;
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
        {order ? (
          <span
            className={`account-order-badge${order.active ? " account-order-badge--active" : order.eligible ? " account-order-badge--eligible" : " account-order-badge--skipped"}`}
            style={order.active ? { background: `#${colorHex}`, borderColor: `#${colorHex}` } : { borderColor: `#${colorHex}`, color: `#${colorHex}` }}
            title={order.active ? `当前主用 · 请求顺序 #${order.position}` : order.eligible ? `请求顺序 #${order.position}` : `请求顺序 #${order.position} · 暂不可用,本轮跳过`}
            aria-label={`请求顺序 ${order.position}`}
          >
            {order.position}
          </span>
        ) : null}
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
          {authFailed || account.is_forbidden
            ? t("quota.needsReauthNote", "需重新授权,请到服务商页重新登录")
            : account.provider_id === "codex"
              ? t("quota.fetchFailed", "额度获取失败,仅显示健康状态")
              : t("quota.noUsageData", "暂无额度数据,账号健康")}
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
