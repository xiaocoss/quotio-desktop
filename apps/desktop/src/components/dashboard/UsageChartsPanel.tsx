import { useMemo, useState } from "react";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { UsageModelBreakdownRow, UsageTimeSeriesPoint } from "../../types";
import { formatCompactNumber, formatCost } from "../../lib/format";
import { useT } from "../../i18n";

type ChartTab = "trend" | "tokens" | "models";

type UsageChartsPanelProps = {
  timeseries: UsageTimeSeriesPoint[];
  modelBreakdown: UsageModelBreakdownRow[];
  loading: boolean;
};

type TrendPoint = UsageTimeSeriesPoint & {
  label: string;
};

type ModelChartRow = UsageModelBreakdownRow & {
  cost_value: number;
};

const COST_COLOR = "#ff8a00";
const UNCACHED_COLOR = "#2e7bff";
const CACHED_COLOR = "#8b5cf6";
const OUTPUT_COLOR = "#20b86e";

function numeric(value: unknown): number {
  const next = typeof value === "number" ? value : Number(value);
  return Number.isFinite(next) ? next : 0;
}

function formatTokenTooltip(value: unknown): string {
  return formatCompactNumber(numeric(value));
}

function formatCostTooltip(value: unknown): string {
  if (value === null || value === undefined) return "--";
  return formatCost(numeric(value));
}

function formatTimeLabel(ms: number): string {
  if (!ms || ms <= 0) return "--";
  const date = new Date(ms);
  return date.toLocaleString(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function UsageChartsPanel({ timeseries, modelBreakdown, loading }: UsageChartsPanelProps) {
  const t = useT();
  const [tab, setTab] = useState<ChartTab>("trend");

  const trendData = useMemo<TrendPoint[]>(
    () =>
      timeseries.map((point) => ({
        ...point,
        label: point.bucket || formatTimeLabel(point.bucket_start_ms),
      })),
    [timeseries],
  );

  const modelData = useMemo<ModelChartRow[]>(
    () =>
      modelBreakdown.map((row) => ({
        ...row,
        cost_value: row.estimated_cost ?? 0,
      })),
    [modelBreakdown],
  );

  const isEmpty = tab === "models" ? modelData.length === 0 : trendData.length === 0;
  const tabs: { key: ChartTab; label: string }[] = [
    { key: "trend", label: t("dash.charts.trend") },
    { key: "tokens", label: t("dash.charts.tokens") },
    { key: "models", label: t("dash.charts.models") },
  ];

  return (
    <article className={loading ? "panel chart-panel chart-panel--loading" : "panel chart-panel"}>
      <div className="panel-head">
        <div>
          <h2 className="panel-title">{t("dash.charts.title")}</h2>
          <p className="panel-desc">{t("dash.charts.desc")}</p>
        </div>
        <div className="segmented" role="tablist" aria-label={t("dash.charts.title")}>
          {tabs.map((item) => (
            <button
              key={item.key}
              type="button"
              role="tab"
              aria-selected={tab === item.key}
              className={tab === item.key ? "active" : undefined}
              onClick={() => setTab(item.key)}
            >
              {item.label}
            </button>
          ))}
        </div>
      </div>

      {isEmpty ? (
        <div className="chart-empty">{t("dash.charts.empty")}</div>
      ) : (
        <div className="chart-canvas">
          {tab === "trend" ? <CostTrendChart data={trendData} costLabel={t("dash.kpi.cost")} /> : null}
          {tab === "tokens" ? (
            <TokenMixChart
              data={trendData}
              uncachedLabel={t("dash.charts.uncachedInput")}
              cachedLabel={t("dash.charts.cachedInput")}
              outputLabel={t("dash.charts.output")}
            />
          ) : null}
          {tab === "models" ? <ModelCostChart data={modelData} costLabel={t("dash.kpi.cost")} /> : null}
        </div>
      )}
    </article>
  );
}

function CostTrendChart({ data, costLabel }: { data: TrendPoint[]; costLabel: string }) {
  return (
    <ResponsiveContainer width="100%" height="100%">
      <AreaChart data={data} margin={{ top: 10, right: 18, left: 0, bottom: 0 }}>
        <defs>
          <linearGradient id="costTrendFill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor={COST_COLOR} stopOpacity={0.24} />
            <stop offset="95%" stopColor={COST_COLOR} stopOpacity={0.03} />
          </linearGradient>
        </defs>
        <CartesianGrid strokeDasharray="3 3" vertical={false} />
        <XAxis dataKey="label" minTickGap={24} tickLine={false} axisLine={false} />
        <YAxis tickLine={false} axisLine={false} width={62} tickFormatter={(value) => formatCost(numeric(value))} />
        <Tooltip formatter={(value) => formatCostTooltip(value)} labelFormatter={(label) => `${label}`} />
        <Area
          type="monotone"
          name={costLabel}
          dataKey="estimated_cost"
          stroke={COST_COLOR}
          fill="url(#costTrendFill)"
          strokeWidth={3}
          dot={{ r: 4, fill: COST_COLOR, stroke: "#fff", strokeWidth: 2 }}
          activeDot={{ r: 6 }}
          connectNulls
        />
      </AreaChart>
    </ResponsiveContainer>
  );
}

function TokenMixChart({
  data,
  uncachedLabel,
  cachedLabel,
  outputLabel,
}: {
  data: TrendPoint[];
  uncachedLabel: string;
  cachedLabel: string;
  outputLabel: string;
}) {
  return (
    <ResponsiveContainer width="100%" height="100%">
      <BarChart data={data} margin={{ top: 10, right: 18, left: 0, bottom: 0 }}>
        <CartesianGrid strokeDasharray="3 3" vertical={false} />
        <XAxis dataKey="label" minTickGap={24} tickLine={false} axisLine={false} />
        <YAxis tickLine={false} axisLine={false} width={56} tickFormatter={formatTokenTooltip} />
        <Tooltip formatter={(value) => formatTokenTooltip(value)} labelFormatter={(label) => `${label}`} />
        <Legend />
        <Bar name={uncachedLabel} dataKey="uncached_input_tokens" stackId="tokens" fill={UNCACHED_COLOR} radius={[4, 4, 0, 0]} />
        <Bar name={cachedLabel} dataKey="cached_tokens" stackId="tokens" fill={CACHED_COLOR} radius={[4, 4, 0, 0]} />
        <Bar name={outputLabel} dataKey="output_tokens" stackId="tokens" fill={OUTPUT_COLOR} radius={[4, 4, 0, 0]} />
      </BarChart>
    </ResponsiveContainer>
  );
}

function ModelCostChart({ data, costLabel }: { data: ModelChartRow[]; costLabel: string }) {
  return (
    <ResponsiveContainer width="100%" height="100%">
      <BarChart data={data} layout="vertical" margin={{ top: 10, right: 28, left: 34, bottom: 0 }}>
        <CartesianGrid strokeDasharray="3 3" horizontal={false} />
        <XAxis type="number" tickLine={false} axisLine={false} tickFormatter={(value) => formatCost(numeric(value))} />
        <YAxis type="category" dataKey="model" width={130} tickLine={false} axisLine={false} />
        <Tooltip formatter={(value) => formatCostTooltip(value)} labelFormatter={(label) => `${label}`} />
        <Bar name={costLabel} dataKey="cost_value" fill={COST_COLOR} radius={[0, 8, 8, 0]} />
      </BarChart>
    </ResponsiveContainer>
  );
}
