import { useMemo, useState } from "react";
import type { AccountQuota, AppState, AuthFile, ProviderSummary, QuotaModelUsage } from "../../types";
import { maskEmail, quotaTone, parsePlan, planTier, matchAuthFile } from "../../lib/format";
import { RefreshIcon } from "../icons";
import { HealthDots } from "../HealthDots";
import { useT } from "../../i18n";

type QuotaScreenProps = {
  appState: AppState;
  isManagementBusy: boolean;
  isQuotaBusy: boolean;
  managementAction: string | null;
  onRefreshManagement: () => void;
  onRefreshQuotas: () => void;
  onRunManagementStateAction: (command: string, args?: Record<string, unknown>) => void;
};

type QuotaGroup = {
  id: string;
  label: string;
  colorHex: string;
  accounts: AccountQuota[];
};

export function QuotaScreen({ appState, isQuotaBusy, onRefreshQuotas }: QuotaScreenProps) {
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
              <AccountQuotaCard key={account.account_key} account={account} authFiles={authFiles} />
            ))}
          </div>
        </>
      )}
    </section>
  );
}

function AccountQuotaCard({ account, authFiles }: { account: AccountQuota; authFiles: AuthFile[] }) {
  const t = useT();
  // The Codex fetcher encodes the subscription tier + expiry into status_message
  // as "plan: <tier> | until: <YYYY-MM-DD>"; surface them as a badge + date.
  const statusMessage = account.status_message ?? "";
  const plan = parsePlan(statusMessage);
  const expiry = statusMessage.match(/until:\s*([^|]+)/i)?.[1]?.trim();
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
          </div>
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
