import { useState } from "react";
import type { AccountQuota, AppState, ProviderSummary, QuotaModelUsage } from "../../types";
import { maskEmail, quotaTone, parsePlan, planTier } from "../../lib/format";
import { RefreshIcon } from "../icons";
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
  const groups = buildGroups(appState.quotas, appState.providers);
  const [activeId, setActiveId] = useState<string | null>(null);
  const active = groups.find((group) => group.id === activeId) ?? groups[0] ?? null;

  return (
    <section className="section-page quota-page">
      <header className="page-topbar" data-tauri-drag-region>
        <h1>{t("nav.quota")}</h1>
        <button
          className={isQuotaBusy ? "icon-button icon-button--spinning" : "icon-button"}
          type="button"
          onClick={onRefreshQuotas}
          disabled={isQuotaBusy}
          title="刷新额度"
          aria-label="刷新额度"
        >
          <RefreshIcon />
        </button>
      </header>

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
              <AccountQuotaCard key={account.account_key} account={account} />
            ))}
          </div>
        </>
      )}
    </section>
  );
}

function AccountQuotaCard({ account }: { account: AccountQuota }) {
  const t = useT();
  // The Codex fetcher encodes the subscription tier + expiry into status_message
  // as "plan: <tier> | until: <YYYY-MM-DD>"; surface them as a badge + date.
  const statusMessage = account.status_message ?? "";
  const plan = parsePlan(statusMessage);
  const expiry = statusMessage.match(/until:\s*([^|]+)/i)?.[1]?.trim();
  // Each tier gets its own badge color; Plus is the base style.
  const tier = plan ? planTier(plan) : null;
  const planClass =
    tier && tier !== "plus"
      ? `quota-type-pill quota-plan-pill quota-plan-pill--${tier}`
      : "quota-type-pill quota-plan-pill";
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
          {account.is_forbidden ? <span className="quota-pill quota-pill--bad">{t("quota.forbidden")}</span> : null}
          {account.warming_up ? <span className="quota-pill quota-pill--warn">{t("quota.warmup")}</span> : null}
          {account.in_use ? <span className="quota-pill quota-pill--blue">{t("quota.useInIde")}</span> : null}
        </div>
      </div>

      <div className="quota-usage-head">
        <span>{t("quota.usage")}</span>
      </div>

      <div className="quota-models">
        {account.models.map((model) => (
          <ModelQuotaRow key={model.model} model={model} />
        ))}
      </div>
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
