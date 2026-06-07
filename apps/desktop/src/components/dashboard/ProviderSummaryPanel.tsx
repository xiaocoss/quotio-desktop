import type { DashboardModel, ProviderDashboardItem } from "../../state/dashboardModel";
import { useT } from "../../i18n";

type ProviderSummaryPanelProps = {
  model: DashboardModel;
};

export function ProviderSummaryPanel({ model }: ProviderSummaryPanelProps) {
  const t = useT();
  const providers = [...model.connectedProviders, ...model.availableProviders];

  return (
    <article className="panel providers-panel">
      <div className="panel-label">
        <span className="eyebrow">{t("nav.providers")}</span>
      </div>

      <div className="provider-pills">
        {providers.map((provider) => (
          <ProviderPill key={provider.id} provider={provider} muted={!provider.isConnected} />
        ))}
      </div>
    </article>
  );
}

function ProviderPill({ provider, muted }: { provider: ProviderDashboardItem; muted: boolean }) {
  const initial = provider.display_name.trim().charAt(0).toUpperCase() || "?";
  const accent = muted
    ? undefined
    : { color: `#${provider.color_hex}`, background: `#${provider.color_hex}22` };

  return (
    <span className={muted ? "provider-pill provider-pill--muted" : "provider-pill"}>
      <span className="provider-pill-logo" style={accent} aria-hidden="true">
        {initial}
      </span>
      <span className="provider-pill-name">{provider.display_name}</span>
      {provider.accountCount > 1 ? (
        <span className="provider-pill-count" style={accent}>
          {provider.accountCount}
        </span>
      ) : null}
    </span>
  );
}
