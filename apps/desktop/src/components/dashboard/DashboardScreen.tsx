import { useEffect, useState } from "react";
import { UsageFilterBar } from "./UsageFilterBar";
import { UsageKpiGrid } from "./UsageKpiGrid";
import { AccountSummaryPanel } from "./AccountSummaryPanel";
import { ModelPricesDialog } from "./ModelPricesDialog";
import { UsageChartsPanel } from "./UsageChartsPanel";
import { useUsageDashboard } from "../../state/usageDashboard";
import { Select, type SelectOption } from "../Select";
import { RefreshIcon } from "../icons";
import { useT } from "../../i18n";
import "./dashboard.css";

const AUTO_REFRESH_SECONDS = [0, 5, 10, 30, 60];

type DashboardScreenProps = {
  // 从额度卡片「查看该账号用量」跳来时携带的账号邮箱(一次性,读后即清)。
  initialAccount?: string | null;
  onFocusConsumed?: () => void;
};

export function DashboardScreen({ initialAccount, onFocusConsumed }: DashboardScreenProps = {}) {
  const t = useT();
  const dash = useUsageDashboard();
  const [pricesOpen, setPricesOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  // 挂载时若带了聚焦账号,写进筛选并立即消费,避免下次手动进仪表盘时残留旧筛选。
  const { setFilters } = dash;
  useEffect(() => {
    if (!initialAccount) return;
    setFilters((prev) => ({ ...prev, account: initialAccount }));
    onFocusConsumed?.();
  }, [initialAccount, setFilters, onFocusConsumed]);

  // Single explicit refresh (title-bar button) re-queries stats + filter options
  // and briefly shows one loading card. The silent auto-refresh / filter changes
  // don't show it.
  const refreshAll = () => {
    setRefreshing(true);
    void Promise.all([dash.refresh(), dash.refreshOptions()]).finally(() =>
      window.setTimeout(() => setRefreshing(false), 600),
    );
  };

  const autoRefreshOptions: SelectOption[] = AUTO_REFRESH_SECONDS.map((seconds) => ({
    value: String(seconds),
    label: seconds === 0 ? t("dash.autoRefresh.off") : `${seconds}${t("dash.seconds")}`,
  }));

  const busy = dash.loading || refreshing;

  return (
    <section className="dashboard-content dashboard-content--fixed usage-dash">
      {/* Title bar is a FIXED header above the scroll body. It stays draggable via
          the empty area + title; the toolbar opts out so its controls stay clickable. */}
      <header className="page-topbar" data-tauri-drag-region>
        <h1 data-tauri-drag-region="false">{t("nav.dashboard")}</h1>
        <div className="dash-toolbar" data-tauri-drag-region="false">
          <span className="dash-toolbar-label">{t("dash.autoRefresh")}</span>
          <Select
            value={String(dash.autoRefreshSec)}
            options={autoRefreshOptions}
            onChange={(value) => dash.setAutoRefreshSec(Number(value))}
            minWidth="96px"
          />
          <button
            type="button"
            className={busy ? "dash-refresh dash-refresh--busy" : "dash-refresh"}
            onClick={refreshAll}
          >
            <RefreshIcon />
            <span>{t("common.refresh")}</span>
          </button>
        </div>
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
          hasActiveFilters={dash.hasActiveFilters}
          onReset={dash.resetFilters}
        />

        {dash.error ? (
          <div className="dash-error" role="alert">
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
