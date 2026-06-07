import { useEffect, useState } from "react";
import { buildDashboardModel } from "../../state/dashboardModel";
import type { AppState, AuthFile } from "../../types";
import { KpiCard } from "./KpiCard";
import { ProviderSummaryPanel } from "./ProviderSummaryPanel";
import { CheckIcon, CopyIcon, RefreshIcon } from "../icons";
import { useT } from "../../i18n";
import { invoke } from "../../lib/tauri";

type DashboardScreenProps = {
  appState: AppState;
  onRefreshState: () => void;
  onRefreshQuotas: () => void;
  onRefreshManagement: () => void;
};

export function DashboardScreen({
  appState,
  onRefreshState,
  onRefreshQuotas,
  onRefreshManagement,
}: DashboardScreenProps) {
  const t = useT();
  const [localAccounts, setLocalAccounts] = useState<AuthFile[]>([]);
  const [spinning, setSpinning] = useState(false);
  const reloadLocalAccounts = () => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    void invoke<AuthFile[]>("list_local_accounts").then(setLocalAccounts).catch(() => {});
  };
  useEffect(() => {
    reloadLocalAccounts();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [appState.management.auth_files]);
  const model = buildDashboardModel(appState, t, localAccounts);
  const endpoint = `${appState.proxy.endpoint.replace(/\/+$/, "")}/v1`;

  function handleRefresh() {
    setSpinning(true);
    onRefreshState();
    onRefreshQuotas();
    onRefreshManagement();
    reloadLocalAccounts();
    window.setTimeout(() => setSpinning(false), 1200);
  }

  return (
    <section className="dashboard-content">
      <header className="page-topbar" data-tauri-drag-region>
        <h1>{t("nav.dashboard")}</h1>
        <button
          className={spinning ? "icon-button icon-button--spinning" : "icon-button"}
          type="button"
          onClick={handleRefresh}
          title="刷新状态"
          aria-label="刷新状态"
        >
          <RefreshIcon />
        </button>
      </header>

      <section className="kpi-grid">
        {model.kpis.map((kpi) => (
          <KpiCard key={kpi.title} kpi={kpi} />
        ))}
      </section>

      <ProviderSummaryPanel model={model} />

      <EndpointCard endpoint={endpoint} />
    </section>
  );
}

function EndpointCard({ endpoint }: { endpoint: string }) {
  const t = useT();
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(endpoint);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch {
      setCopied(false);
    }
  }

  return (
    <article className="panel endpoint-card">
      <div className="panel-label">
        <span className="eyebrow">{t("dash.apiEndpoint")}</span>
      </div>
      <div className="endpoint-row">
        <code className="endpoint-url">{endpoint}</code>
        <button className="icon-button" type="button" onClick={copy} title="复制地址" aria-label="复制地址">
          {copied ? <CheckIcon /> : <CopyIcon />}
        </button>
      </div>
    </article>
  );
}

