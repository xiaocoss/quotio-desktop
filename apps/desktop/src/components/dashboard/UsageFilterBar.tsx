import type { UsageFilterOptions, UsageStatusFilter } from "../../types";
import type { TimeRangeKey, UsageFilters } from "../../state/usageDashboard";
import { Select, type SelectOption } from "../Select";
import { maskEmail } from "../../lib/format";
import { useT } from "../../i18n";

type UsageFilterBarProps = {
  range: TimeRangeKey;
  onRangeChange: (range: TimeRangeKey) => void;
  customStart: string;
  onCustomStartChange: (value: string) => void;
  customEnd: string;
  onCustomEndChange: (value: string) => void;
  filters: UsageFilters;
  onFiltersChange: (filters: UsageFilters) => void;
  options: UsageFilterOptions;
  hasActiveFilters: boolean;
  onReset: () => void;
};

const RANGE_KEYS: TimeRangeKey[] = ["today", "7d", "14d", "30d", "all", "custom"];

export function UsageFilterBar({
  range,
  onRangeChange,
  customStart,
  onCustomStartChange,
  customEnd,
  onCustomEndChange,
  filters,
  onFiltersChange,
  options,
  hasActiveFilters,
  onReset,
}: UsageFilterBarProps) {
  const t = useT();

  const setFilter = (patch: Partial<UsageFilters>) => onFiltersChange({ ...filters, ...patch });

  const withAll = (label: string, values: string[], render?: (value: string) => string): SelectOption[] => [
    { value: "", label },
    ...values.map((value) => ({ value, label: render ? render(value) : value })),
  ];

  const apiKeyOptions: SelectOption[] = [
    { value: "", label: t("dash.filter.allApiKeys") },
    ...options.api_keys.map((key) => ({
      value: key.hash,
      label: key.alias ?? `${key.hash.slice(0, 8)}…`,
    })),
  ];

  const statusOptions: SelectOption[] = [
    { value: "all", label: t("dash.filter.allStatus") },
    { value: "success", label: t("dash.status.success") },
    { value: "failed", label: t("dash.status.failed") },
  ];

  return (
    <article className="panel usage-filter-bar">
      <div className="range-tabs" role="tablist" aria-label={t("dash.timeRange")}>
        {RANGE_KEYS.map((key) => (
          <button
            key={key}
            type="button"
            role="tab"
            aria-selected={range === key}
            className={range === key ? "range-tab range-tab--active" : "range-tab"}
            onClick={() => onRangeChange(key)}
          >
            {t(`dash.range.${key}`)}
          </button>
        ))}
      </div>

      {range === "custom" ? (
        <div className="custom-range-row">
          <input
            type="datetime-local"
            value={customStart}
            onChange={(event) => onCustomStartChange(event.target.value)}
            aria-label={t("dash.range.start")}
          />
          <span className="custom-range-sep">→</span>
          <input
            type="datetime-local"
            value={customEnd}
            onChange={(event) => onCustomEndChange(event.target.value)}
            aria-label={t("dash.range.end")}
          />
        </div>
      ) : null}

      <div className="usage-filter-row">
        <Select
          className="usage-filter-control usage-filter-control--account"
          value={filters.account}
          options={withAll(t("dash.filter.allAccounts"), options.accounts, maskEmail)}
          onChange={(value) => setFilter({ account: value })}
          minWidth="160px"
        />
        <Select
          className="usage-filter-control usage-filter-control--provider"
          value={filters.provider}
          options={withAll(t("dash.filter.allProviders"), options.providers)}
          onChange={(value) => setFilter({ provider: value })}
          minWidth="150px"
        />
        <Select
          className="usage-filter-control usage-filter-control--model"
          value={filters.model}
          options={withAll(t("dash.filter.allModels"), options.models)}
          onChange={(value) => setFilter({ model: value })}
          minWidth="150px"
        />
        <Select
          className="usage-filter-control usage-filter-control--channel"
          value={filters.channel}
          options={withAll(t("dash.filter.allChannels"), options.channels)}
          onChange={(value) => setFilter({ channel: value })}
          minWidth="150px"
        />
        <Select
          className="usage-filter-control usage-filter-control--api-key"
          value={filters.apiKeyHash}
          options={apiKeyOptions}
          onChange={(value) => setFilter({ apiKeyHash: value })}
          minWidth="170px"
        />
        <Select
          className="usage-filter-control usage-filter-control--status"
          value={filters.status}
          options={statusOptions}
          onChange={(value) => setFilter({ status: value as UsageStatusFilter })}
          minWidth="120px"
        />
      </div>

      <div className="usage-search-row">
        <div className="usage-search">
          <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true">
            <circle cx="7" cy="7" r="4.5" />
            <path d="M10.5 10.5L14 14" />
          </svg>
          <input
            type="text"
            value={filters.search}
            placeholder={t("dash.searchPlaceholder")}
            onChange={(event) => setFilter({ search: event.target.value })}
          />
        </div>
        <button type="button" className="ghost-action" onClick={onReset} disabled={!hasActiveFilters}>
          {t("dash.clearFilters")}
        </button>
      </div>
    </article>
  );
}
