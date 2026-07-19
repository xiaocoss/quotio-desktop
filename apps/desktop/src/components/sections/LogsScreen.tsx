import { useEffect, useMemo, useRef, useState } from "react";
import type { AppState, RequestLogEntry } from "../../types";
import { Select } from "../Select";
import { maskEmail } from "../../lib/format";
import { useT } from "../../i18n";
import { invoke } from "../../lib/tauri";
import "./logs.css";
import "./logs-rose.css";

type LogsScreenProps = {
  appState: AppState;
  isManagementBusy: boolean;
  managementAction: string | null;
  onRefreshManagement: () => void;
  onClearLogs: () => void;
  onClearRequests: () => void;
  onRunManagementStateAction: (command: string, args?: Record<string, unknown>) => void;
  // 从额度卡片「查看该账号日志」跳来时携带的账号邮箱(一次性,读后即清)。
  initialAccount?: string | null;
  onFocusConsumed?: () => void;
};

type LogTab = "requests" | "proxy";
type StatusFilter = "all" | "2xx" | "4xx" | "5xx";
type LevelFilter = "all" | "info" | "warn" | "error";
type DateRange = "today" | "7d" | "all";

// 内联的 SVG 符号图标(素材见 public/logs/log-icons.svg)。
function Icon({ id }: { id: string }) {
  return (
    <svg className="lr-icon" aria-hidden="true">
      <use href={`/logs/log-icons.svg#${id}`} />
    </svg>
  );
}

export function LogsScreen({ appState, isManagementBusy, onRefreshManagement, onClearLogs, onClearRequests, initialAccount, onFocusConsumed }: LogsScreenProps) {
  const t = useT();
  const requests = appState.logs ?? [];
  const proxyLines = appState.management.logs?.lines ?? [];
  const proxyConfig = appState.management.config;
  const lineCount = appState.management.logs?.line_count ?? proxyLines.length;

  const [tab, setTab] = useState<LogTab>("requests");
  const [query, setQuery] = useState("");
  const [provider, setProvider] = useState("all");
  // 以下模型/状态/模式 + 代理页的等级/来源/自动滚动,都是纯前端视图筛选:只对已经拿到的
  // requests / proxyLines 做客户端过滤,不触碰任何数据层(types / useAppState / 后端命令)。
  const [model, setModel] = useState("all");
  const [status, setStatus] = useState<StatusFilter>("all");
  const [mode, setMode] = useState("all");
  const [range, setRange] = useState<DateRange>("today");
  const [refreshing, setRefreshing] = useState(false);
  const [page, setPage] = useState(0);
  // null = 无弹窗;数字 = 待确认清空的请求记录总数(显示在二次确认里)。
  const [pendingClear, setPendingClear] = useState<number | null>(null);
  const [level, setLevel] = useState<LevelFilter>("all");
  const [source, setSource] = useState("all");
  const [autoScroll, setAutoScroll] = useState(true);
  const PAGE_SIZE = 20;

  // 从额度卡片「查看该账号日志」跳来 → 切到请求标签、按该账号邮箱搜索、放宽到「全部」时间
  // 范围,并立即消费一次性焦点(避免下次手动进日志页残留旧搜索)。
  useEffect(() => {
    if (!initialAccount) return;
    setTab("requests");
    setQuery(initialAccount);
    setRange("all");
    onFocusConsumed?.();
  }, [initialAccount, onFocusConsumed]);

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

  async function handleClear() {
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
  }

  function handleExport() {
    const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
    const content = tab === "requests" ? requestCsv(filteredRequests) : filteredProxy.map((entry) => entry.raw).join("\n");
    const blob = new Blob([content], { type: tab === "requests" ? "text/csv;charset=utf-8" : "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `quotio-${tab}-logs-${stamp}.${tab === "requests" ? "csv" : "log"}`;
    anchor.click();
    URL.revokeObjectURL(url);
  }
  const providers = useMemo(
    () =>
      Array.from(
        new Set(requests.map((entry) => entry.provider ?? entry.resolved_provider).filter((name): name is string => Boolean(name))),
      ),
    [requests],
  );

  // 模型 / 模式下拉的可选项:从已有请求字段派生(与 providers 同一套路)。
  const models = useMemo(
    () =>
      Array.from(
        new Set(requests.map((entry) => entry.model ?? entry.resolved_model).filter((name): name is string => Boolean(name))),
      ),
    [requests],
  );
  const modes = useMemo(
    () =>
      Array.from(
        new Set(requests.map((entry) => (entry.reasoning_effort ?? "").trim()).filter((value) => value.length > 0)),
      ),
    [requests],
  );

  const filteredRequests = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return requests.filter((entry) => {
      if (range === "today" && !isToday(entry.timestamp)) return false;
      if (range === "7d" && !isWithinDays(entry.timestamp, 7)) return false;
      const entryProvider = entry.provider ?? entry.resolved_provider ?? "";
      if (provider !== "all" && entryProvider !== provider) return false;
      const entryModel = entry.model ?? entry.resolved_model ?? "";
      if (model !== "all" && entryModel !== model) return false;
      if (status !== "all" && statusBucket(entry.status_code) !== status) return false;
      if (mode !== "all" && (entry.reasoning_effort ?? "").trim() !== mode) return false;
      if (!normalized) return true;
      const entryAccount = (entry.account ?? "").toLowerCase();
      return (
        entryModel.toLowerCase().includes(normalized) ||
        entryProvider.toLowerCase().includes(normalized) ||
        entryAccount.includes(normalized)
      );
    });
  }, [requests, query, provider, model, status, mode, range]);

  const stats = useMemo(() => computeStats(filteredRequests), [filteredRequests]);
  const health = useMemo(() => computeHealth(filteredRequests), [filteredRequests]);

  const pageCount = Math.max(1, Math.ceil(filteredRequests.length / PAGE_SIZE));
  const safePage = Math.min(page, pageCount - 1);
  const pagedRequests = filteredRequests.slice(safePage * PAGE_SIZE, (safePage + 1) * PAGE_SIZE);
  useEffect(() => {
    setPage(0);
  }, [query, provider, model, status, mode, tab, range]);

  function resetRequestFilters() {
    setQuery("");
    setProvider("all");
    setModel("all");
    setStatus("all");
    setMode("all");
    setRange("today");
  }

  // 代理日志按行解析(纯展示派生:行号 / 时间 / 等级 / 来源 / 正文),原始 proxyLines 不变。
  const parsedProxy = useMemo(() => proxyLines.map((line, index) => ({ n: index + 1, ...parseProxyLine(line) })), [proxyLines]);
  const proxySources = useMemo(
    () => Array.from(new Set(parsedProxy.map((entry) => entry.source).filter((value) => value.length > 0))),
    [parsedProxy],
  );
  const errorCount = useMemo(() => parsedProxy.reduce((sum, entry) => sum + (entry.level === "error" ? 1 : 0), 0), [parsedProxy]);
  const latestTime = parsedProxy.length > 0 ? parsedProxy[parsedProxy.length - 1].time : "";

  const filteredProxy = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return parsedProxy.filter((entry) => {
      if (level !== "all" && entry.level !== level) return false;
      if (source !== "all" && entry.source !== source) return false;
      if (normalized && !entry.raw.toLowerCase().includes(normalized)) return false;
      return true;
    });
  }, [parsedProxy, level, source, query]);
  const visibleProxy = useMemo(() => filteredProxy.slice(-200), [filteredProxy]);

  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (tab !== "proxy" || !autoScroll) return;
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [tab, autoScroll, visibleProxy]);

  function resetProxyFilters() {
    setLevel("all");
    setSource("all");
    setQuery("");
  }

  const proxyRunning = appState.proxy.status === "running";
  const proxyHost = appState.proxy.endpoint.replace(/^https?:\/\//, "");

  return (
    <section className="section-page logs-redesign">
      <header className="lr-header" data-tauri-drag-region>
        <div data-tauri-drag-region="false">
          <h1 className="lr-title">{t("nav.logs")}</h1>
          <p className="lr-subtitle">{t("logs.subtitle", "追踪请求、代理事件与性能异常")}</p>
        </div>
        <div className="lr-tabs" role="tablist" aria-label={t("logs.viewSwitch", "日志类型")}>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "requests"}
            className={tab === "requests" ? "lr-tab lr-tab--active" : "lr-tab"}
            onClick={() => setTab("requests")}
          >
            <Icon id="request" />
            {t("logs.requests")}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "proxy"}
            className={tab === "proxy" ? "lr-tab lr-tab--active" : "lr-tab"}
            onClick={() => setTab("proxy")}
          >
            <Icon id="proxy" />
            {t("logs.proxyLogs")}
          </button>
        </div>
        <div aria-hidden="true" />
        <div className="lr-actions">
          <label className="lr-search">
            <Icon id="search" />
            <input
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={tab === "requests" ? t("logs.searchRequests") : t("logs.searchLogs")}
            />
          </label>
          <button
            className={refreshing ? "lr-btn lr-btn--spin" : "lr-btn"}
            type="button"
            onClick={handleRefresh}
            disabled={isManagementBusy}
            title={t("logs.refresh", "刷新")}
          >
            <Icon id="refresh" />
            {t("logs.refresh", "刷新")}
          </button>
          <button className="lr-btn" type="button" onClick={handleExport} title={t("logs.export", "导出")}>
            <Icon id="export" />
            {t("logs.export", "导出")}
          </button>
          <button
            className="lr-btn lr-btn--danger"
            type="button"
            onClick={handleClear}
            disabled={isManagementBusy}
            title={tab === "requests" ? "清空请求日志" : "清空代理日志"}
          >
            <Icon id="trash" />
            {t("logs.clear", "清空")}
          </button>
        </div>
      </header>

      {tab === "requests" ? (
        <>
          <section className="lr-metrics" aria-label={t("logs.overview", "日志概览")}>
            <article className="lr-panel lr-metric">
              <div>
                <div className="lr-metric-label">{t("logs.total")}</div>
                <div className="lr-metric-value">{stats.total}</div>
              </div>
              <div className="lr-metric-icon">
                <Icon id="activity" />
              </div>
            </article>
            <article className="lr-panel lr-metric lr-metric--success">
              <div>
                <div className="lr-metric-label">{t("logs.success")}</div>
                <div className="lr-metric-value">{stats.successRate}%</div>
              </div>
              <div className="lr-metric-icon">
                <Icon id="check" />
              </div>
            </article>
            <article className="lr-panel lr-metric lr-metric--latency">
              <div>
                <div className="lr-metric-label">{t("logs.avgTime")}</div>
                <div className="lr-metric-value">{formatSeconds(stats.avgMs)}</div>
              </div>
              <img className="lr-sparkline" src="/logs/latency-sparkline.svg" alt="" aria-hidden="true" />
            </article>
            <article className="lr-panel lr-metric lr-metric--tokens">
              <div>
                <div className="lr-metric-label">{t("logs.totalTokens", "总 Tokens")}</div>
                <div className="lr-metric-value">{formatCompact(stats.totalTokens)}</div>
              </div>
              <div className="lr-metric-icon">
                <Icon id="layers" />
              </div>
            </article>
          </section>

          <section className="lr-panel lr-health" aria-label={t("logs.health", "请求健康")}>
            <span className="lr-health-title">{t("logs.health", "请求健康")}</span>
            <div className="lr-health-bar" aria-hidden="true">
              <span className="lr-health-seg lr-health-seg--ok" style={{ width: `${health.okPct}%` }} />
              <span className="lr-health-seg lr-health-seg--warn" style={{ width: `${health.warnPct}%` }} />
              <span className="lr-health-seg lr-health-seg--err" style={{ width: `${health.errPct}%` }} />
            </div>
            <div className="lr-health-legend">
              <span className="lr-legend-item">
                <i className="lr-legend-dot lr-legend-dot--ok" />
                2xx {health.ok} <span className="lr-legend-pct">({health.okPct.toFixed(1)}%)</span>
              </span>
              <span className="lr-legend-item">
                <i className="lr-legend-dot lr-legend-dot--warn" />
                4xx {health.warn} <span className="lr-legend-pct">({health.warnPct.toFixed(1)}%)</span>
              </span>
              <span className="lr-legend-item">
                <i className="lr-legend-dot lr-legend-dot--err" />
                5xx {health.err} <span className="lr-legend-pct">({health.errPct.toFixed(1)}%)</span>
              </span>
            </div>
            <span className="lr-health-slow">
              <Icon id="clock" />
              {t("logs.slowest", "最慢")} {formatSeconds(health.slowest)}
            </span>
          </section>

          <section className="lr-filters" aria-label={t("logs.filters", "日志筛选")}>
            <div className="lr-segmented">
              <button
                type="button"
                className={range === "today" ? "lr-segment lr-segment--active" : "lr-segment"}
                onClick={() => setRange("today")}
              >
                {t("logs.today", "今天")}
              </button>
              <button
                type="button"
                className={range === "7d" ? "lr-segment lr-segment--active" : "lr-segment"}
                onClick={() => setRange("7d")}
              >
                {t("logs.last7Days", "7 天")}
              </button>
              <button
                type="button"
                className={range === "all" ? "lr-segment lr-segment--active" : "lr-segment"}
                onClick={() => setRange("all")}
              >
                {t("logs.all", "全部")}
              </button>
            </div>
            <div className="lr-filter-select">
              <Select
                value={provider}
                options={[{ value: "all", label: t("logs.allProviders", "全部服务商") }, ...providers.map((name) => ({ value: name, label: name }))]}
                onChange={setProvider}
              />
            </div>
            <div className="lr-filter-select">
              <Select
                value={model}
                options={[{ value: "all", label: t("logs.allModels", "全部模型") }, ...models.map((name) => ({ value: name, label: name }))]}
                onChange={setModel}
              />
            </div>
            <div className="lr-filter-select">
              <Select
                value={status}
                options={[
                  { value: "all", label: t("logs.allStatus", "全部状态") },
                  { value: "2xx", label: "2xx" },
                  { value: "4xx", label: "4xx" },
                  { value: "5xx", label: "5xx" },
                ]}
                onChange={(value) => setStatus(value as StatusFilter)}
              />
            </div>
            <div className="lr-filter-select">
              <Select
                value={mode}
                options={[
                  { value: "all", label: t("logs.allModes", "全部模式") },
                  ...modes.map((value) => ({ value, label: reasoningLabel(value, t) })),
                ]}
                onChange={setMode}
              />
            </div>
            <button type="button" className="lr-reset" onClick={resetRequestFilters}>
              <Icon id="reset" />
              {t("logs.resetFilters", "重置筛选")}
            </button>
          </section>

          <section className="lr-panel lr-table-panel">
            {filteredRequests.length === 0 ? (
              <p className="lr-empty">{t("logs.emptyRequests")}</p>
            ) : (
              <>
                <div className="lr-table-scroll">
                  <table className="lr-table">
                    <colgroup>
                      <col style={{ width: "96px" }} />
                      <col style={{ width: "78px" }} />
                      <col style={{ width: "250px" }} />
                      <col style={{ width: "240px" }} />
                      <col style={{ width: "105px" }} />
                      <col style={{ width: "120px" }} />
                      <col style={{ width: "120px" }} />
                      <col style={{ width: "110px" }} />
                      <col style={{ width: "150px" }} />
                    </colgroup>
                    <thead>
                      <tr>
                        <th>{t("logs.colTime")}</th>
                        <th>{t("logs.colStatus")}</th>
                        <th>{t("logs.colProviderAccount", "服务商 / 账号")}</th>
                        <th>{t("logs.colModelMode", "模型 / 模式")}</th>
                        <th>{t("logs.colDuration")}</th>
                        <th className="lr-th-num">{t("logs.colInput", "输入 Tokens")}</th>
                        <th className="lr-th-num">{t("logs.colOutput", "输出 Tokens")}</th>
                        <th className="lr-th-num">{t("logs.colTotal", "总计")}</th>
                        <th>{t("logs.colActions", "操作")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {pagedRequests.map((entry, index) => (
                        <RequestRow key={`${safePage}-${index}`} entry={entry} />
                      ))}
                    </tbody>
                  </table>
                </div>
                <footer className="lr-table-footer">
                  <span>{t("logs.totalCount", "共 {n} 条").replace("{n}", String(filteredRequests.length))}</span>
                  <div className="lr-pagination">
                    <button
                      type="button"
                      className="lr-page-btn"
                      disabled={safePage === 0}
                      onClick={() => setPage(safePage - 1)}
                    >
                      {t("logs.prevPage", "上一页")}
                    </button>
                    <span className="lr-page-info">
                      {safePage + 1} / {pageCount}
                    </span>
                    <button
                      type="button"
                      className="lr-page-btn"
                      disabled={safePage >= pageCount - 1}
                      onClick={() => setPage(safePage + 1)}
                    >
                      {t("logs.nextPage", "下一页")}
                    </button>
                  </div>
                  {/* 每页条数固定 20(PAGE_SIZE),渲染为只读视觉控件。 */}
                  <span className="lr-page-size">{t("logs.pageSize", "{n} 条/页").replace("{n}", String(PAGE_SIZE))}</span>
                </footer>
              </>
            )}
          </section>
        </>
      ) : (
        <>
          <section className="lr-metrics" aria-label={t("logs.proxyOverview", "代理日志概览")}>
            <article className="lr-panel lr-metric">
              <div>
                <div className="lr-metric-label">{t("logs.lineCount", "日志行数")}</div>
                <div className="lr-metric-value">{lineCount.toLocaleString()}</div>
              </div>
              <div className="lr-metric-icon">
                <Icon id="file" />
              </div>
            </article>
            <article className="lr-panel lr-metric lr-metric--status">
              <div>
                <div className="lr-metric-label">{t("logs.proxyStatus", "代理状态")}</div>
                <div className={proxyRunning ? "lr-metric-value lr-metric-value--healthy" : "lr-metric-value lr-metric-value--error"}>
                  {proxyStatusLabel(appState.proxy.status, t)}
                </div>
              </div>
              <div className="lr-metric-icon">
                <Icon id="shield" />
              </div>
            </article>
            <article className="lr-panel lr-metric lr-metric--errors">
              <div>
                <div className="lr-metric-label">{t("logs.errors", "错误")}</div>
                <div className="lr-metric-value lr-metric-value--error">{errorCount}</div>
              </div>
              <div className="lr-metric-icon">
                <Icon id="alert" />
              </div>
            </article>
            <article className="lr-panel lr-metric">
              <div>
                <div className="lr-metric-label">{t("logs.latest", "最新日志")}</div>
                <div className="lr-metric-value">{latestTime || "—"}</div>
              </div>
              <div className="lr-metric-icon">
                <Icon id="clock" />
              </div>
            </article>
          </section>

          <section className="lr-panel lr-runtime" aria-label={t("logs.proxyRuntime", "代理运行状态")}>
            <div className="lr-runtime-item">
              <span className={proxyRunning ? "lr-runtime-dot" : "lr-runtime-dot lr-runtime-dot--bad"} />
              <div className="lr-runtime-text">
                <strong>{t("logs.localProxy", "本地代理")}</strong>
                <small>{proxyHost}</small>
              </div>
              <span />
            </div>
            <div className="lr-runtime-item">
              <Icon id="file" />
              <div className="lr-runtime-text">
                <strong>{t("logs.fileLog", "文件日志")}</strong>
                <small>{proxyConfig?.logging_to_file ? t("logs.on", "已开启") : t("logs.off", "已关闭")}</small>
              </div>
              <span />
            </div>
            <div className="lr-runtime-item">
              <Icon id="bug" />
              <div className="lr-runtime-text">
                <strong>{t("logs.debugMode", "调试模式")}</strong>
                <small>{proxyConfig?.debug ? t("logs.on", "已开启") : t("logs.off", "已关闭")}</small>
              </div>
              <span />
            </div>
            <div className="lr-runtime-item">
              <Icon id="pulse" />
              <div className="lr-runtime-text">
                <strong>{t("logs.autoScroll", "自动滚动")}</strong>
                <small>{autoScroll ? t("logs.on", "已开启") : t("logs.off", "已关闭")}</small>
              </div>
              <button
                type="button"
                className={autoScroll ? "lr-switch lr-switch--on" : "lr-switch"}
                onClick={() => setAutoScroll((value) => !value)}
                aria-pressed={autoScroll}
                aria-label={t("logs.autoScroll", "自动滚动")}
              />
            </div>
          </section>

          <section className="lr-filters" aria-label={t("logs.proxyFilters", "代理日志筛选")}>
            {(
              [
                { id: "all", label: t("logs.levelAll", "全部"), cls: "" },
                { id: "info", label: "INFO", cls: " lr-level--info" },
                { id: "warn", label: "WARN", cls: " lr-level--warn" },
                { id: "error", label: "ERROR", cls: " lr-level--error" },
              ] as { id: LevelFilter; label: string; cls: string }[]
            ).map((item) => (
              <button
                key={item.id}
                type="button"
                className={`lr-level${item.cls}${level === item.id ? " lr-level--active" : ""}`}
                onClick={() => setLevel(item.id)}
              >
                {item.label}
              </button>
            ))}
            <div className="lr-filter-select">
              <Select
                value={source}
                options={[{ value: "all", label: t("logs.allSources", "全部来源") }, ...proxySources.map((name) => ({ value: name, label: name }))]}
                onChange={setSource}
              />
            </div>
            <button type="button" className="lr-reset" onClick={resetProxyFilters}>
              <Icon id="reset" />
              {t("logs.resetFilters", "重置筛选")}
            </button>
          </section>

          <section className="lr-panel lr-stream">
            <header className="lr-stream-head">
              {t("logs.liveTitle", "实时代理日志")} <span className="lr-live">LIVE</span>
            </header>
            {visibleProxy.length === 0 ? (
              <p className="lr-empty">{t("logs.emptyProxy")}</p>
            ) : (
              <>
                <div className="lr-log-scroll" role="log" aria-live="polite" ref={scrollRef}>
                  {visibleProxy.map((entry) => (
                    <div
                      key={entry.n}
                      className={
                        entry.level === "error"
                          ? "lr-log-line lr-log-line--error"
                          : entry.level === "warn"
                            ? "lr-log-line lr-log-line--warn"
                            : "lr-log-line"
                      }
                    >
                      <span className="lr-line-no">{entry.n}</span>
                      <span className="lr-line-time">{entry.time}</span>
                      <span className={`lr-line-level lr-line-level--${entry.level}`}>{entry.level.toUpperCase()}</span>
                      <span className="lr-line-source">{entry.source}</span>
                      <span className="lr-line-msg">{entry.message}</span>
                    </div>
                  ))}
                </div>
                <footer className="lr-stream-footer">
                  <span>
                    {t("logs.showingRecent", "显示最近 {shown} / {total} 行")
                      .replace("{shown}", String(visibleProxy.length))
                      .replace("{total}", lineCount.toLocaleString())}
                  </span>
                  <span className="lr-stream-footer-right">
                    <span>
                      {t("logs.latestShort", "最新")} {latestTime || "—"}
                    </span>
                    <span className={autoScroll ? "lr-scrolling" : "lr-scrolling lr-scrolling--off"}>
                      <i className="lr-scrolling-dot" />
                      {autoScroll ? t("logs.autoScrolling", "自动滚动中") : t("logs.autoScrollPaused", "已暂停")}
                    </span>
                  </span>
                </footer>
              </>
            )}
          </section>
        </>
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
  const [open, setOpen] = useState(false);
  const status = entry.status_code ?? 0;
  const bucket = statusBucket(entry.status_code);
  const statusCls =
    bucket === "2xx" ? "lr-status lr-status--ok" : bucket === "4xx" ? "lr-status lr-status--warn" : "lr-status lr-status--error";
  const provider = entry.provider ?? entry.resolved_provider ?? "—";
  const model = entry.model ?? entry.resolved_model ?? "—";
  const account = entry.account?.trim() ? maskEmail(entry.account) : null;
  const reason = reasoningInfo(entry.reasoning_effort, t);
  const inTokens = entry.input_tokens ?? 0;
  const outTokens = entry.output_tokens ?? 0;
  // 显示用启发式:超过 100s 视为慢请求(与设计一致 —— 107.03s 标黄、97.96s 不标)。
  const slow = entry.duration_ms >= 100000;

  return (
    <>
      <tr className={slow ? "lr-row-slow" : undefined}>
        <td>{formatLogTime(entry.timestamp)}</td>
        <td>
          <span className={statusCls}>{status || "—"}</span>
        </td>
        <td>
          <span className="lr-provider">{provider}</span>
          {account ? <span className="lr-account">{account}</span> : null}
        </td>
        <td>
          <span className="lr-model">{model}</span>
          {reason ? <span className={reason.deep ? "lr-mode lr-mode--deep" : "lr-mode"}>{reason.label}</span> : null}
        </td>
        <td className={slow ? "lr-duration-slow" : undefined}>{(entry.duration_ms / 1000).toFixed(2)}s</td>
        <td className="lr-token">{formatCompact(inTokens)}</td>
        <td className="lr-token">{formatCompact(outTokens)}</td>
        <td className="lr-token">{formatCompact(inTokens + outTokens)}</td>
        <td>
          <div className="lr-actions-cell">
            <button
              type="button"
              className={open ? "lr-details lr-details--open" : "lr-details"}
              onClick={() => setOpen((v) => !v)}
              aria-expanded={open}
            >
              <Icon id="eye" />
              {t("logs.details", "查看详情")}
            </button>
          </div>
        </td>
      </tr>
      {open ? (
        <tr className="lr-detail-row">
          <td colSpan={9}>
            <dl className="lr-detail">
              <div>
                <dt>{t("logs.detail.endpoint", "接口")}</dt>
                <dd className="lr-detail-mono">
                  {entry.method} {entry.endpoint}
                </dd>
              </div>
              <div>
                <dt>{t("logs.detail.status", "状态码")}</dt>
                <dd>{entry.status_code ?? "—"}</dd>
              </div>
              <div>
                <dt>{t("logs.detail.time", "时间")}</dt>
                <dd>{entry.timestamp}</dd>
              </div>
              <div>
                <dt>{t("logs.detail.resolved", "解析服务商 / 模型")}</dt>
                <dd>
                  {(entry.resolved_provider ?? provider) || "—"} / {(entry.resolved_model ?? model) || "—"}
                </dd>
              </div>
              {account ? (
                <div>
                  <dt>{t("logs.detail.account", "账号")}</dt>
                  <dd>{account}</dd>
                </div>
              ) : null}
              <div>
                <dt>{t("logs.detail.size", "请求 / 响应")}</dt>
                <dd>
                  {formatCompact(entry.request_size)} / {formatCompact(entry.response_size)} B
                </dd>
              </div>
              {reason ? (
                <div>
                  <dt>{t("logs.detail.reasoning", "推理档位")}</dt>
                  <dd>{reason.label}</dd>
                </div>
              ) : null}
            </dl>
            {entry.error_message ? <div className="lr-detail-error">{entry.error_message}</div> : null}
          </td>
        </tr>
      ) : null}
    </>
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

// 下拉里的模式标签:复用 reasoningInfo 的等级映射(去掉「推理」前缀,只留档位名)。
function reasoningLabel(effort: string, t: (key: string) => string): string {
  const info = reasoningInfo(effort, t);
  if (!info) return effort;
  const prefix = `${t("logs.reasoning")} `;
  return info.label.startsWith(prefix) ? info.label.slice(prefix.length) : info.label;
}

function statusBucket(code: number | null | undefined): StatusFilter | "other" {
  const value = code ?? 0;
  if (value >= 200 && value < 300) return "2xx";
  if (value >= 400 && value < 500) return "4xx";
  if (value >= 500 && value < 600) return "5xx";
  return "other";
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

// 2xx / 4xx / 5xx(含其它非成功)分布 + 最慢耗时,全部由已过滤请求派生,仅供健康条展示。
function computeHealth(entries: RequestLogEntry[]) {
  let ok = 0;
  let warn = 0;
  let err = 0;
  let slowest = 0;
  for (const entry of entries) {
    const bucket = statusBucket(entry.status_code);
    if (bucket === "2xx") ok += 1;
    else if (bucket === "4xx") warn += 1;
    else err += 1;
    if (entry.duration_ms > slowest) slowest = entry.duration_ms;
  }
  const total = entries.length || 1;
  return {
    ok,
    warn,
    err,
    slowest,
    okPct: (ok / total) * 100,
    warnPct: (warn / total) * 100,
    errPct: (err / total) * 100,
  };
}

function formatLogTime(timestamp: string): string {
  if (!timestamp.includes("T")) return timestamp;
  const parsed = new Date(timestamp);
  return Number.isNaN(parsed.getTime()) ? timestamp : parsed.toLocaleTimeString();
}

// 平均耗时 / 最慢耗时按秒展示(对齐设计的「28.5s」「107.03s」);毫秒来源不变。
function formatSeconds(ms: number): string {
  const seconds = ms / 1000;
  if (seconds >= 100) return `${seconds.toFixed(2)}s`;
  return `${seconds.toFixed(1)}s`;
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

// 最近 N 天(含今天)。无法解析的时间戳保留(与 isToday 一致),不因解析失败被隐藏。
function isWithinDays(timestamp: string, days: number): boolean {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return true;
  return date.getTime() >= Date.now() - days * 86400000;
}

// 代理日志来源识别用的已知关键字(用于「来源」下拉与来源列;识别不到就留空,只在「全部来源」下出现)。
const PROXY_SOURCES = ["proxy", "route", "upstream", "auth", "retry", "config", "server", "request", "scheduler", "credential"];

// 把一行原始日志字符串尽力拆成 { 时间, 等级, 来源, 正文 }。拆不出的字段留空,原文保留在 raw。
function parseProxyLine(line: string): {
  time: string;
  level: LevelFilter;
  source: string;
  message: string;
  raw: string;
} {
  const raw = line;
  const lower = line.toLowerCase();
  const level: LevelFilter =
    /error|fail|panic|exception|fatal/.test(lower) ? "error" : /\bwarn/.test(lower) ? "warn" : "info";

  let message = line.trim();
  let time = "";
  // 剥掉行首的时间戳(ISO 日期时间 或裸 HH:MM:SS[.mmm]),放进时间列。
  const timeMatch = message.match(
    /^\[?(?:\d{4}[-/]\d{1,2}[-/]\d{1,2}[ T])?(\d{1,2}:\d{2}:\d{2}(?:[.,]\d+)?)(?:Z|[+-]\d{2}:?\d{2})?\]?\s+/,
  );
  if (timeMatch) {
    time = timeMatch[1].replace(",", ".");
    message = message.slice(timeMatch[0].length);
  }
  // 剥掉行首的等级 token(INFO/WARN/ERROR/…),避免和等级列重复。
  message = message.replace(/^\[?(?:INFO|WARN(?:ING)?|ERROR|ERR|DEBUG|TRACE|FATAL|PANIC)\]?[:\s]+/i, "").trim();

  // 来源:整行里出现的第一个已知来源关键字(作为词);识别不到就留空。
  const source = PROXY_SOURCES.find((name) => new RegExp(`\\b${name}\\b`, "i").test(line)) ?? "";

  return { time, level, source, message: message || raw.trim(), raw };
}

function proxyStatusLabel(status: string, t: (key: string, fallback?: string) => string): string {
  switch (status) {
    case "running":
      return t("logs.proxyRunning", "运行正常");
    case "starting":
      return t("logs.proxyStarting", "启动中");
    case "stopping":
      return t("logs.proxyStopping", "停止中");
    case "stopped":
      return t("logs.proxyStopped", "已停止");
    case "missing_binary":
      return t("logs.proxyMissing", "未安装");
    case "crashed":
      return t("logs.proxyCrashed", "已崩溃");
    default:
      return t("logs.proxyError", "异常");
  }
}

function formatCompact(value: number) {
  // Force en-US compact units (K/M/B/T) so tokens read as "166K", not "16.6万".
  return new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}

function requestCsv(entries: RequestLogEntry[]) {
  const cells = (values: unknown[]) => values.map((value) => `"${String(value ?? "").replace(/"/g, '""')}"`).join(",");
  const header = cells(["time", "status", "provider", "account", "model", "mode", "duration_ms", "input_tokens", "output_tokens"]);
  const rows = entries.map((entry) => cells([
    entry.timestamp,
    entry.status_code,
    entry.provider ?? entry.resolved_provider,
    entry.account,
    entry.model ?? entry.resolved_model,
    entry.reasoning_effort,
    entry.duration_ms,
    entry.input_tokens,
    entry.output_tokens,
  ]));
  return `\uFEFF${[header, ...rows].join("\n")}`;
}
