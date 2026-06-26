import { useEffect, useMemo, useState } from "react";
import type { AppState, RequestLogEntry } from "../../types";
import { RefreshIcon, TrashIcon } from "../icons";
import { Select } from "../Select";
import { maskEmail } from "../../lib/format";
import { useT } from "../../i18n";
import { invoke } from "../../lib/tauri";

type LogsScreenProps = {
  appState: AppState;
  isManagementBusy: boolean;
  managementAction: string | null;
  onRefreshManagement: () => void;
  onClearLogs: () => void;
  onClearRequests: () => void;
  onRunManagementStateAction: (command: string, args?: Record<string, unknown>) => void;
};

type LogTab = "requests" | "proxy";

export function LogsScreen({ appState, isManagementBusy, onRefreshManagement, onClearLogs, onClearRequests }: LogsScreenProps) {
  const t = useT();
  const requests = appState.logs ?? [];
  const proxyLines = appState.management.logs?.lines ?? [];

  const [tab, setTab] = useState<LogTab>("requests");
  const [query, setQuery] = useState("");
  const [provider, setProvider] = useState("all");
  const [todayOnly, setTodayOnly] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [page, setPage] = useState(0);
  // null = 无弹窗;数字 = 待确认清空的请求记录总数(显示在二次确认里)。
  const [pendingClear, setPendingClear] = useState<number | null>(null);
  const PAGE_SIZE = 20;

  async function handleRefresh() {
    if (refreshing) return;
    setRefreshing(true);
    const startedAt = Date.now();
    try {
      await Promise.resolve(onRefreshManagement());
    } catch {
      // errors surface via app state
    } finally {
      const elapsed = Date.now() - startedAt;
      window.setTimeout(() => setRefreshing(false), Math.max(0, 600 - elapsed));
    }
  }

  const providers = useMemo(
    () =>
      Array.from(
        new Set(requests.map((entry) => entry.provider ?? entry.resolved_provider).filter((name): name is string => Boolean(name))),
      ),
    [requests],
  );

  const filteredRequests = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return requests.filter((entry) => {
      if (todayOnly && !isToday(entry.timestamp)) return false;
      const entryProvider = entry.provider ?? entry.resolved_provider ?? "";
      if (provider !== "all" && entryProvider !== provider) return false;
      if (!normalized) return true;
      const entryModel = entry.model ?? entry.resolved_model ?? "";
      return entryModel.toLowerCase().includes(normalized) || entryProvider.toLowerCase().includes(normalized);
    });
  }, [requests, query, provider, todayOnly]);

  const stats = useMemo(() => computeStats(filteredRequests), [filteredRequests]);

  const pageCount = Math.max(1, Math.ceil(filteredRequests.length / PAGE_SIZE));
  const safePage = Math.min(page, pageCount - 1);
  const pagedRequests = filteredRequests.slice(safePage * PAGE_SIZE, (safePage + 1) * PAGE_SIZE);
  useEffect(() => {
    setPage(0);
  }, [query, provider, tab, todayOnly]);

  const filteredProxy = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return proxyLines;
    return proxyLines.filter((line) => line.toLowerCase().includes(normalized));
  }, [proxyLines, query]);

  return (
    <section className="section-page logs-page">
      <header className="page-topbar" data-tauri-drag-region>
        <h1>{t("nav.logs")}</h1>
        <div className="topbar-actions">
          <input
            className="log-search"
            type="text"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={tab === "requests" ? t("logs.searchRequests") : t("logs.searchLogs")}
          />
          <button
            className={refreshing ? "icon-button icon-button--spinning" : "icon-button"}
            type="button"
            onClick={handleRefresh}
            disabled={isManagementBusy}
            title="刷新"
            aria-label="刷新"
          >
            <RefreshIcon />
          </button>
          <button
            className="icon-button"
            type="button"
            onClick={async () => {
              if (tab === "requests") {
                // 「请求」日志=SQLite 全部用量历史(也是仪表盘数据)。删前查真实总条数,
                // 弹 app 内二次确认(不用容易点穿的系统弹窗),如实告知会删多少、不可恢复。
                let count = filteredRequests.length;
                try {
                  count = await invoke<number>("count_request_logs");
                } catch {
                  /* 拿不到精确总数就退回可见条数 */
                }
                setPendingClear(count);
              } else {
                onClearLogs();
              }
            }}
            disabled={isManagementBusy}
            title={tab === "requests" ? "清空请求日志" : "清空代理日志"}
            aria-label={tab === "requests" ? "清空请求日志" : "清空代理日志"}
          >
            <TrashIcon />
          </button>
        </div>
      </header>

      <div className="log-tabs">
        <button className={tab === "requests" ? "log-tab log-tab--active" : "log-tab"} type="button" onClick={() => setTab("requests")}>
          {t("logs.requests")}
        </button>
        <button className={tab === "proxy" ? "log-tab log-tab--active" : "log-tab"} type="button" onClick={() => setTab("proxy")}>
          {t("logs.proxyLogs")}
        </button>
      </div>

      {tab === "requests" ? (
        <article className="panel logs-panel">
          <div className="log-stats">
            <div className="log-stat">
              <span>{t("logs.total")}</span>
              <strong>{stats.total}</strong>
            </div>
            <div className="log-stat">
              <span>{t("logs.success")}</span>
              <strong>{stats.successRate}%</strong>
            </div>
            <div className="log-stat">
              <span>{t("logs.tokens")}</span>
              <strong>{formatCompact(stats.totalTokens)}</strong>
            </div>
            <div className="log-stat">
              <span>{t("logs.avgTime")}</span>
              <strong>{stats.avgMs}ms</strong>
            </div>
            <div className="log-stats-spacer" />
            <div className="view-toggle log-range-toggle">
              <button
                type="button"
                className={todayOnly ? "view-toggle-btn view-toggle-btn--active" : "view-toggle-btn"}
                onClick={() => setTodayOnly(true)}
              >
                {t("logs.today", "今天")}
              </button>
              <button
                type="button"
                className={!todayOnly ? "view-toggle-btn view-toggle-btn--active" : "view-toggle-btn"}
                onClick={() => setTodayOnly(false)}
              >
                {t("logs.all", "全部")}
              </button>
            </div>
            <label className="log-provider">
              <span>{t("logs.provider")}</span>
              <Select
                value={provider}
                options={[{ value: "all", label: "All Providers" }, ...providers.map((name) => ({ value: name, label: name }))]}
                onChange={setProvider}
              />
            </label>
          </div>

          {filteredRequests.length === 0 ? (
            <p className="empty-copy">{t("logs.emptyRequests")}</p>
          ) : (
            <>
              <div className="log-req-list">
                <div className="log-req-row log-req-head">
                  <span className="log-req-time">{t("logs.colTime")}</span>
                  <span className="log-req-status">{t("logs.colStatus")}</span>
                  <div className="log-req-model">{t("logs.colModel")}</div>
                  <span className="log-req-duration">{t("logs.colDuration")}</span>
                  <span className="log-req-tokens">{t("logs.colTokens")}</span>
                </div>
                {pagedRequests.map((entry, index) => (
                  <RequestRow key={`${safePage}-${index}`} entry={entry} />
                ))}
              </div>
              {pageCount > 1 ? (
                <div className="log-pagination">
                  <button
                    type="button"
                    className="log-page-btn"
                    disabled={safePage === 0}
                    onClick={() => setPage(safePage - 1)}
                  >
                    {t("logs.prevPage", "上一页")}
                  </button>
                  <span className="log-page-info">
                    {safePage + 1} / {pageCount}
                  </span>
                  <button
                    type="button"
                    className="log-page-btn"
                    disabled={safePage >= pageCount - 1}
                    onClick={() => setPage(safePage + 1)}
                  >
                    {t("logs.nextPage", "下一页")}
                  </button>
                </div>
              ) : null}
            </>
          )}
        </article>
      ) : (
        <article className="panel logs-panel log-panel">
          {filteredProxy.length === 0 ? (
            <p className="empty-copy">{t("logs.emptyProxy")}</p>
          ) : (
            <div className="log-list">
              {filteredProxy
                .slice(-200)
                .reverse()
                .map((line, index) => (
                  <pre className={isErrorLog(line) ? "log-line log-line--error" : "log-line"} key={`${index}-${line}`}>
                    {line}
                  </pre>
                ))}
            </div>
          )}
        </article>
      )}

      {pendingClear !== null ? (
        <div className="modal-overlay" onClick={() => setPendingClear(null)}>
          <div className="close-dialog" onClick={(event) => event.stopPropagation()}>
            <strong className="close-dialog-title">清空全部请求日志?</strong>
            <p className="close-dialog-desc">
              这将<strong>永久删除 {pendingClear.toLocaleString()} 条</strong>请求记录——它们也是
              <strong>仪表盘的历史用量数据</strong>,删除后<strong>无法恢复</strong>。
            </p>
            <div className="close-dialog-actions">
              <button type="button" className="ghost-action" onClick={() => setPendingClear(null)}>
                取消
              </button>
              <button
                type="button"
                className="danger-action"
                onClick={() => {
                  setPendingClear(null);
                  onClearRequests();
                }}
              >
                永久删除
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </section>
  );
}

function RequestRow({ entry }: { entry: RequestLogEntry }) {
  const t = useT();
  const status = entry.status_code ?? 0;
  const ok = status >= 200 && status < 300;
  const provider = entry.provider ?? entry.resolved_provider ?? "—";
  const model = entry.model ?? entry.resolved_model ?? "—";
  const account = entry.account?.trim() ? maskEmail(entry.account) : null;
  const reason = reasoningInfo(entry.reasoning_effort, t);

  return (
    <div className="log-req-row">
      <span className="log-req-time">{formatLogTime(entry.timestamp)}</span>
      <span className={ok ? "log-req-status log-req-status--ok" : "log-req-status log-req-status--err"}>{status || "—"}</span>
      <div className="log-req-model">
        <span className="log-req-head-line">
          <strong>{provider}</strong>
          {account ? <span className="log-req-account">{account}</span> : null}
        </span>
        <span className="log-req-model-sub">
          <small>{model}</small>
          {reason ? (
            <span className={reason.deep ? "log-req-reason log-req-reason--deep" : "log-req-reason"}>{reason.label}</span>
          ) : null}
        </span>
      </div>
      <span className="log-req-duration">{(entry.duration_ms / 1000).toFixed(2)}s</span>
      <span className="log-req-tokens">
        {formatCompact(entry.input_tokens ?? 0)} → {formatCompact(entry.output_tokens ?? 0)}
      </span>
    </div>
  );
}

function reasoningInfo(
  effort: string | null | undefined,
  t: (key: string) => string,
): { label: string; deep: boolean } | null {
  if (!effort) return null;
  const normalized = effort.trim().toLowerCase();
  if (!normalized) return null;
  const table: Record<string, { key: string; deep: boolean }> = {
    minimal: { key: "logs.rsMinimal", deep: false },
    none: { key: "logs.rsMinimal", deep: false },
    low: { key: "logs.rsLow", deep: false },
    medium: { key: "logs.rsMedium", deep: false },
    high: { key: "logs.rsHigh", deep: true },
    xhigh: { key: "logs.rsXHigh", deep: true },
    very_high: { key: "logs.rsXHigh", deep: true },
    "very-high": { key: "logs.rsXHigh", deep: true },
  };
  const found = table[normalized];
  const level = found ? t(found.key) : effort;
  return { label: `${t("logs.reasoning")} ${level}`, deep: found?.deep ?? false };
}

function computeStats(entries: RequestLogEntry[]) {
  const total = entries.length;
  const success = entries.filter((entry) => (entry.status_code ?? 0) >= 200 && (entry.status_code ?? 0) < 300).length;
  const totalTokens = entries.reduce((sum, entry) => sum + (entry.input_tokens ?? 0) + (entry.output_tokens ?? 0), 0);
  const totalMs = entries.reduce((sum, entry) => sum + entry.duration_ms, 0);
  return {
    total,
    successRate: total > 0 ? Math.round((success / total) * 100) : 0,
    totalTokens,
    avgMs: total > 0 ? Math.round(totalMs / total) : 0,
  };
}

function formatLogTime(timestamp: string): string {
  if (!timestamp.includes("T")) return timestamp;
  const parsed = new Date(timestamp);
  return Number.isNaN(parsed.getTime()) ? timestamp : parsed.toLocaleTimeString();
}

// Whether a request's timestamp falls on the local calendar day. Unparseable
// timestamps (e.g. dev-mock time-only strings) are kept rather than hidden.
function isToday(timestamp: string): boolean {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return true;
  const now = new Date();
  return (
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate()
  );
}

function isErrorLog(line: string) {
  const value = line.toLowerCase();
  return value.includes("error") || value.includes("failed") || value.includes("panic") || value.includes("exception");
}

function formatCompact(value: number) {
  // Force en-US compact units (K/M/B/T) so tokens read as "166K", not "16.6万".
  return new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}
