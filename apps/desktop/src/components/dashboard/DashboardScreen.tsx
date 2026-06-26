import { useState } from "react";
import type { AppState } from "../../types";
import { UsageFilterBar } from "./UsageFilterBar";
import { UsageKpiGrid } from "./UsageKpiGrid";
import { AccountSummaryPanel } from "./AccountSummaryPanel";
import { ModelPricesDialog } from "./ModelPricesDialog";
import { UsageChartsPanel } from "./UsageChartsPanel";
import { useUsageDashboard } from "../../state/usageDashboard";
import { CheckIcon, CopyIcon } from "../icons";
import { useT } from "../../i18n";

type DashboardScreenProps = {
  appState: AppState;
};

export function DashboardScreen({ appState }: DashboardScreenProps) {
  const t = useT();
  const dash = useUsageDashboard();
  const [pricesOpen, setPricesOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const endpoint = `${appState.proxy.endpoint.replace(/\/+$/, "")}/v1`;

  // Single refresh lives in the filter bar (next to auto-refresh), matching the
  // reference layout — it re-queries the stats + filter options and shows one
  // loading card, kept up briefly so it's visible. Only this explicit click
  // shows the card; the silent auto-refresh / filter changes don't.
  const refreshAll = () => {
    setRefreshing(true);
    void Promise.all([dash.refresh(), dash.refreshOptions()]).finally(() =>
      window.setTimeout(() => setRefreshing(false), 600),
    );
  };

  return (
    <section className="dashboard-content dashboard-content--fixed">
      {/* Title bar is a FIXED header above the scroll body — content never
          slides under it, so there's no masking band and no per-frame sticky
          repaint (smoother scrolling). The window stays draggable via the bar. */}
      <header className="page-topbar" data-tauri-drag-region>
        <h1>{t("nav.dashboard")}</h1>
      </header>

      <div className="dashboard-scroll">
        <UsageFilterBar
          range={dash.range}
          onRangeChange={dash.setRange}
          customStart={dash.customStart}
          onCustomStartChange={dash.setCustomStart}
          customEnd={dash.customEnd}
          onCustomEndChange={dash.setCustomEnd}
          filters={dash.filters}
          onFiltersChange={dash.setFilters}
          options={dash.options}
          autoRefreshSec={dash.autoRefreshSec}
          onAutoRefreshChange={dash.setAutoRefreshSec}
          onRefresh={refreshAll}
          loading={dash.loading || refreshing}
          hasActiveFilters={dash.hasActiveFilters}
          onReset={dash.resetFilters}
        />

        {dash.error ? (
          <div className="apikey-router-warning" role="alert">
            ⚠ {t("dash.loadFailed", "加载用量数据失败")}:{dash.error}
          </div>
        ) : null}

        <UsageKpiGrid stats={dash.stats} />

        <UsageChartsPanel
          timeseries={dash.timeseries}
          modelBreakdown={dash.modelBreakdown}
          loading={dash.loading}
        />

        <AccountSummaryPanel
          rows={dash.summary}
          loading={dash.loading}
          onRefresh={() => void dash.refresh()}
          onPickAccount={(account) => dash.setFilters({ ...dash.filters, account })}
          onManagePrices={() => setPricesOpen(true)}
        />

        <EndpointCard endpoint={endpoint} />
      </div>

      {refreshing ? (
        <div className="closing-overlay">
          <div className="loading-card">
            <div className="boot-bar" aria-hidden="true">
              <span />
            </div>
            <p>{t("common.refreshing", "正在刷新…")}</p>
          </div>
        </div>
      ) : null}

      {pricesOpen ? (
        <ModelPricesDialog
          prices={dash.prices}
          knownModels={dash.options.models}
          onSave={dash.saveModelPrices}
          onClose={() => setPricesOpen(false)}
        />
      ) : null}
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
        <button className="icon-button" type="button" onClick={copy} title={t("common.copy")} aria-label={t("common.copy")}>
          {copied ? <CheckIcon /> : <CopyIcon />}
        </button>
      </div>
    </article>
  );
}
