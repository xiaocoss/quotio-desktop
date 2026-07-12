import { useEffect, useMemo, useState } from "react";
import type { AccountSummaryRow } from "../../types";
import { Select, type SelectOption } from "../Select";
import { RefreshIcon } from "../icons";
import { formatCompactNumber, formatCost, formatRelativeTime, maskEmail } from "../../lib/format";
import { useT } from "../../i18n";

type AccountSummaryPanelProps = {
  rows: AccountSummaryRow[];
  loading: boolean;
  onRefresh: () => void;
  onPickAccount: (account: string) => void;
  onManagePrices: () => void;
};

type SortKey = "cost" | "requests" | "tokens" | "recent" | "successRate";
type ViewMode = "table" | "card";
type StatusTone = "good" | "warn" | "bad" | "neutral";

const PAGE_SIZE = 8;

function statusTone(row: AccountSummaryRow): StatusTone {
  if (row.total_requests === 0) return "neutral";
  if (row.success_rate >= 90) return "good";
  if (row.success_rate >= 50) return "warn";
  return "bad";
}

function sortRows(rows: AccountSummaryRow[], key: SortKey): AccountSummaryRow[] {
  const sorted = [...rows];
  sorted.sort((a, b) => {
    switch (key) {
      case "cost":
        return (b.estimated_cost ?? 0) - (a.estimated_cost ?? 0);
      case "requests":
        return b.total_requests - a.total_requests;
      case "tokens":
        return b.total_tokens - a.total_tokens;
      case "recent":
        return b.last_request_ms - a.last_request_ms;
      case "successRate":
        return b.success_rate - a.success_rate;
    }
  });
  return sorted;
}

export function AccountSummaryPanel({
  rows,
  loading,
  onRefresh,
  onPickAccount,
  onManagePrices,
}: AccountSummaryPanelProps) {
  const t = useT();
  const [view, setView] = useState<ViewMode>("table");
  const [sortKey, setSortKey] = useState<SortKey>("requests");
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(1);

  const sortOptions: SelectOption[] = [
    { value: "requests", label: t("dash.sort.requests") },
    { value: "cost", label: t("dash.sort.cost") },
    { value: "tokens", label: t("dash.sort.tokens") },
    { value: "successRate", label: t("dash.sort.successRate") },
    { value: "recent", label: t("dash.sort.recent") },
  ];

  const visibleRows = useMemo(() => {
    const term = search.trim().toLowerCase();
    const filtered = term
      ? rows.filter(
          (row) =>
            row.account.toLowerCase().includes(term) ||
            (row.provider ?? "").toLowerCase().includes(term),
        )
      : rows;
    return sortRows(filtered, sortKey);
  }, [rows, search, sortKey]);

  const total = visibleRows.length;
  const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));

  // 行数/筛选/排序变化后夹紧当前页,避免停在空页。
  useEffect(() => {
    setPage((current) => Math.min(current, pageCount));
  }, [pageCount]);

  const currentPage = Math.min(page, pageCount);
  const start = (currentPage - 1) * PAGE_SIZE;
  const pagedRows = visibleRows.slice(start, start + PAGE_SIZE);
  const rangeStart = total === 0 ? 0 : start + 1;
  const rangeEnd = Math.min(start + PAGE_SIZE, total);
  const countText = t("dash.pageTotal", "共 {n} 条").replace("{n}", String(total));

  const statusBadge = (row: AccountSummaryRow) => {
    const tone = statusTone(row);
    return <span className={`badge badge--${tone}`}>{t(`dash.health.${tone}`)}</span>;
  };

  return (
    <article className="panel table-panel account-summary-panel">
      <div className="table-tools">
        <h2 className="panel-title">
          {t("dash.accountSummary")}
          <span className="table-count">{rows.length}</span>
        </h2>
        <div className="table-actions">
          <div className="mini-search">
            <svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true">
              <circle cx="7" cy="7" r="4.5" />
              <path d="M10.5 10.5L14 14" />
            </svg>
            <input
              type="text"
              value={search}
              placeholder={t("dash.searchAccount")}
              onChange={(event) => setSearch(event.target.value)}
            />
          </div>
          <button
            type="button"
            className={loading ? "icon-button icon-button--spinning" : "icon-button"}
            onClick={onRefresh}
            title={t("common.refresh")}
            aria-label={t("common.refresh")}
          >
            <RefreshIcon />
          </button>
          <span className="sort-label">{t("dash.sortBy")}</span>
          <Select value={sortKey} options={sortOptions} onChange={(value) => setSortKey(value as SortKey)} minWidth="130px" />
          <button type="button" className="ghost-action" onClick={onManagePrices}>
            {t("dash.managePrices")}
          </button>
          <div className="view-toggle">
            <button
              type="button"
              className={view === "table" ? "active" : undefined}
              onClick={() => setView("table")}
            >
              {t("dash.view.table")}
            </button>
            <button
              type="button"
              className={view === "card" ? "active" : undefined}
              onClick={() => setView("card")}
            >
              {t("dash.view.card")}
            </button>
          </div>
        </div>
      </div>

      {total === 0 ? (
        <div className="account-summary-empty">
          <strong>{t("dash.empty.title")}</strong>
          <p>{t("dash.empty.hint")}</p>
        </div>
      ) : view === "table" ? (
        <div className="account-table-wrap">
          <table className="account-table">
            <thead>
              <tr>
                <th>{t("dash.col.account")}</th>
                <th>{t("dash.col.status")}</th>
                <th className="num">{t("dash.col.total")}</th>
                <th className="num">{t("dash.col.success")}</th>
                <th className="num">{t("dash.col.failed")}</th>
                <th className="num">{t("dash.col.successRate")}</th>
                <th className="num">{t("dash.col.tokens")}</th>
                <th className="num">{t("dash.col.cost")}</th>
                <th>{t("dash.col.lastRequest")}</th>
                <th>{t("dash.col.actions")}</th>
              </tr>
            </thead>
            <tbody>
              {pagedRows.map((row) => (
                <tr key={`${row.account}-${row.provider ?? ""}`}>
                  <td>
                    <div className="account-cell">
                      <span className="account-name">{maskEmail(row.account)}</span>
                      {row.provider ? <span className="account-provider">{row.provider}</span> : null}
                    </div>
                  </td>
                  <td>{statusBadge(row)}</td>
                  <td className="num">{formatCompactNumber(row.total_requests)}</td>
                  <td className="num">{formatCompactNumber(row.success_requests)}</td>
                  <td className="num">{formatCompactNumber(row.failed_requests)}</td>
                  <td className="num">{row.success_rate.toFixed(1)}%</td>
                  <td className="num">{formatCompactNumber(row.total_tokens)}</td>
                  <td className="num">{formatCost(row.estimated_cost)}</td>
                  <td>{formatRelativeTime(row.last_request_ms)}</td>
                  <td>
                    <button type="button" className="link-action" onClick={() => onPickAccount(row.account)}>
                      {t("dash.filterByAccount")}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <div className="acct-card-grid">
          {pagedRows.map((row) => (
            <div key={`${row.account}-${row.provider ?? ""}`} className="acct-card">
              <div className="acct-card-head">
                <span className="account-name">{maskEmail(row.account)}</span>
                {statusBadge(row)}
              </div>
              {row.provider ? <span className="account-provider">{row.provider}</span> : null}
              <div className="acct-card-metrics">
                <div>
                  <dt>{t("dash.col.total")}</dt>
                  <dd>{formatCompactNumber(row.total_requests)}</dd>
                </div>
                <div>
                  <dt>{t("dash.col.successRate")}</dt>
                  <dd>{row.success_rate.toFixed(1)}%</dd>
                </div>
                <div>
                  <dt>{t("dash.col.tokens")}</dt>
                  <dd>{formatCompactNumber(row.total_tokens)}</dd>
                </div>
                <div>
                  <dt>{t("dash.col.cost")}</dt>
                  <dd>{formatCost(row.estimated_cost)}</dd>
                </div>
              </div>
              <div className="acct-card-foot">
                <span>{formatRelativeTime(row.last_request_ms)}</span>
                <button type="button" className="link-action" onClick={() => onPickAccount(row.account)}>
                  {t("dash.filterByAccount")}
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {total > 0 ? (
        <div className="dash-pagination">
          <div className="dash-pagination-pages">
            <button
              type="button"
              className="dash-page-btn"
              onClick={() => setPage((p) => Math.max(1, p - 1))}
              disabled={currentPage <= 1}
              aria-label={t("dash.page.prev", "上一页")}
            >
              ‹
            </button>
            {Array.from({ length: pageCount }, (_, index) => index + 1).map((n) => (
              <button
                key={n}
                type="button"
                className={n === currentPage ? "dash-page-btn dash-page-btn--active" : "dash-page-btn"}
                onClick={() => setPage(n)}
              >
                {n}
              </button>
            ))}
            <button
              type="button"
              className="dash-page-btn"
              onClick={() => setPage((p) => Math.min(pageCount, p + 1))}
              disabled={currentPage >= pageCount}
              aria-label={t("dash.page.next", "下一页")}
            >
              ›
            </button>
          </div>
          <span className="dash-pagination-count">
            {rangeStart}-{rangeEnd}　{countText}
          </span>
        </div>
      ) : null}
    </article>
  );
}
