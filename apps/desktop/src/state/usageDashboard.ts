// Dashboard usage-stats state: owns the time range, filters, search and
// auto-refresh, and fetches aggregated KPIs + the account summary from the
// SQLite-backed usage store via the Tauri commands. Also subscribes to the
// collector's `usage-updated` event for near-real-time refreshes.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../lib/tauri";
import { rangeBounds, type TimeRangeKey } from "./usageRange";
import type {
  AccountSummaryRow,
  ModelPrice,
  UsageAggregate,
  UsageChartBucket,
  UsageFilterOptions,
  UsageModelBreakdownRow,
  UsageQuery,
  UsageStatusFilter,
  UsageTimeSeriesPoint,
} from "../types";

export type { TimeRangeKey } from "./usageRange";

export type UsageFilters = {
  provider: string;
  model: string;
  account: string;
  channel: string;
  apiKeyHash: string;
  status: UsageStatusFilter;
  search: string;
};

export const EMPTY_FILTERS: UsageFilters = {
  provider: "",
  model: "",
  account: "",
  channel: "",
  apiKeyHash: "",
  status: "all",
  search: "",
};

const EMPTY_OPTIONS: UsageFilterOptions = {
  accounts: [],
  providers: [],
  models: [],
  channels: [],
  api_keys: [],
};

function chartBucketForRange(range: TimeRangeKey): UsageChartBucket {
  return range === "today" ? "twenty_minute" : "day";
}

export function useUsageDashboard() {
  const [range, setRange] = useState<TimeRangeKey>("today");
  const [customStart, setCustomStart] = useState("");
  const [customEnd, setCustomEnd] = useState("");
  const [filters, setFilters] = useState<UsageFilters>(EMPTY_FILTERS);
  const [autoRefreshSec, setAutoRefreshSec] = useState(10);

  const [stats, setStats] = useState<UsageAggregate | null>(null);
  const [summary, setSummary] = useState<AccountSummaryRow[]>([]);
  const [timeseries, setTimeseries] = useState<UsageTimeSeriesPoint[]>([]);
  const [modelBreakdown, setModelBreakdown] = useState<UsageModelBreakdownRow[]>([]);
  const [options, setOptions] = useState<UsageFilterOptions>(EMPTY_OPTIONS);
  const [prices, setPrices] = useState<ModelPrice[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 请求序号:并发刷新(筛选变化 + 自动刷新 + 事件触发)时,丢弃过期请求的结果,
  // 防止较慢的旧查询在较新查询之后返回、把新数据覆盖回旧值。
  const requestSeq = useRef(0);

  const query = useMemo<UsageQuery>(() => {
    const { start, end } = rangeBounds(range, customStart, customEnd);
    return {
      start_ms: start,
      end_ms: end,
      provider: filters.provider || null,
      model: filters.model || null,
      account: filters.account || null,
      api_key_hash: filters.apiKeyHash || null,
      channel: filters.channel || null,
      status: filters.status,
      search: filters.search.trim() || null,
    };
  }, [range, customStart, customEnd, filters]);

  const refresh = useCallback(async () => {
    const seq = ++requestSeq.current;
    setLoading(true);
    try {
      const bucket = chartBucketForRange(range);
      const [nextStats, nextSummary, nextTimeseries, nextModelBreakdown] = await Promise.all([
        invoke<UsageAggregate>("query_usage_stats", { query }),
        invoke<AccountSummaryRow[]>("query_account_summary", { query }),
        invoke<UsageTimeSeriesPoint[]>("query_usage_timeseries", { query, bucket }),
        invoke<UsageModelBreakdownRow[]>("query_usage_model_breakdown", { query, limit: 10 }),
      ]);
      // 过期请求:更晚的查询已在飞行中,丢弃旧结果,别覆盖新数据。
      if (seq !== requestSeq.current) return;
      setStats(nextStats);
      setSummary(nextSummary);
      setTimeseries(nextTimeseries);
      setModelBreakdown(nextModelBreakdown);
      setError(null);
    } catch (err) {
      if (seq !== requestSeq.current) return;
      console.warn("[useUsageDashboard] refresh failed:", err);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      if (seq === requestSeq.current) setLoading(false);
    }
  }, [query, range]);

  // 始终持有最新的 refresh,供只想绑定一次的定时器/事件监听调用,避免它们随
  // refresh(每次筛选/搜索变化都重建)而反复重建。
  const latestRefresh = useRef(refresh);
  useEffect(() => {
    latestRefresh.current = refresh;
  }, [refresh]);

  const refreshOptions = useCallback(async () => {
    try {
      const [nextOptions, nextPrices] = await Promise.all([
        invoke<UsageFilterOptions>("list_usage_filter_options"),
        invoke<ModelPrice[]>("get_model_prices"),
      ]);
      setOptions(nextOptions);
      setPrices(nextPrices);
    } catch (err) {
      console.warn("[useUsageDashboard] refreshOptions failed:", err);
    }
  }, []);

  // Re-query whenever the effective query (range/filters/search) changes.
  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    void refreshOptions();
  }, [refreshOptions]);

  // Periodic auto-refresh (0 = off). 只依赖间隔本身:通过 ref 取最新 refresh,
  // 否则每次筛选/搜索按键(refresh 重建)都会清掉重建定时器、刷新计时被反复重置。
  useEffect(() => {
    if (autoRefreshSec <= 0) return;
    const id = window.setInterval(() => void latestRefresh.current(), autoRefreshSec * 1000);
    return () => window.clearInterval(id);
  }, [autoRefreshSec]);

  // Near-real-time: the background collector emits "usage-updated" when it
  // persists new events. Debounce a burst of inserts into one refresh (Tauri).
  // Only the (cheap, indexed) stats/summary queries run here — the filter
  // options (DISTINCT scans) change slowly, so they refresh on mount / manual
  // refresh only, not on every 1.5s data tick.
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    let timer: number | null = null;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen("usage-updated", () => {
          if (timer !== null) return;
          timer = window.setTimeout(() => {
            timer = null;
            void latestRefresh.current();
          }, 800);
        }),
      )
      .then((fn) => {
        // 卸载早于异步 listen 完成时,立即解绑,避免监听器泄漏到卸载之后。
        if (disposed) fn();
        else unlisten = fn;
      });
    return () => {
      disposed = true;
      if (unlisten) unlisten();
      if (timer !== null) window.clearTimeout(timer);
    };
  }, []);

  const saveModelPrices = useCallback(
    async (next: ModelPrice[]) => {
      const saved = await invoke<ModelPrice[]>("set_model_prices", { prices: next });
      setPrices(saved);
      void refresh();
      return saved;
    },
    [refresh],
  );

  const resetFilters = useCallback(() => setFilters(EMPTY_FILTERS), []);

  const hasActiveFilters = useMemo(
    () =>
      filters.provider !== "" ||
      filters.model !== "" ||
      filters.account !== "" ||
      filters.channel !== "" ||
      filters.apiKeyHash !== "" ||
      filters.status !== "all" ||
      filters.search.trim() !== "",
    [filters],
  );

  return {
    range,
    setRange,
    customStart,
    setCustomStart,
    customEnd,
    setCustomEnd,
    filters,
    setFilters,
    resetFilters,
    hasActiveFilters,
    autoRefreshSec,
    setAutoRefreshSec,
    stats,
    summary,
    timeseries,
    modelBreakdown,
    options,
    prices,
    loading,
    error,
    refresh,
    refreshOptions,
    saveModelPrices,
  };
}
