import type { ReactNode } from "react";
import type { DashboardKpi, KpiIconKey } from "../../state/dashboardModel";

type KpiCardProps = {
  kpi: DashboardKpi;
};

const icons: Record<KpiIconKey, ReactNode> = {
  accounts: (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="6" cy="5" r="2.3" />
      <path d="M2 13.2c0-2.2 1.8-3.5 4-3.5s4 1.3 4 3.5" />
      <path d="M10.6 3.1a2.3 2.3 0 0 1 0 4.1M11.4 9.9c1.7.3 2.9 1.5 2.9 3.3" />
    </svg>
  ),
  requests: (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M5 12.5V3.5M5 3.5 2.7 5.8M5 3.5l2.3 2.3" />
      <path d="M11 3.5v9M11 12.5l2.3-2.3M11 12.5l-2.3-2.3" />
    </svg>
  ),
  tokens: (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M8 2.2l1.7 3.9 4.1 1.9-4.1 1.9L8 13.8l-1.7-3.9L2.2 8l4.1-1.9z" />
    </svg>
  ),
  success: (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="8" cy="8" r="5.8" />
      <path d="M5.4 8.2l1.8 1.8 3.4-3.8" />
    </svg>
  ),
};

export function KpiCard({ kpi }: KpiCardProps) {
  return (
    <article className={`kpi-card kpi-card--${kpi.tone}`}>
      <div className="kpi-card-head">
        <span className={`kpi-icon kpi-icon--${kpi.tone}`} aria-hidden="true">
          {icons[kpi.iconKey]}
        </span>
        <span>{kpi.title}</span>
      </div>
      <strong>{kpi.value}</strong>
      <p>{kpi.caption}</p>
    </article>
  );
}
