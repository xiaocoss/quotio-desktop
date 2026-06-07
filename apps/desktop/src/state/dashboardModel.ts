import type { AccountQuota, AppState, AuthFile, ProviderSummary, ProxyStatusKind } from "../types";
import type { TranslateFn } from "../i18n";

export type DashboardMode = "missing_binary" | "stopped" | "running" | "remote" | "error";

export type KpiIconKey = "accounts" | "requests" | "tokens" | "success";

export type DashboardKpi = {
  title: string;
  value: string;
  caption: string;
  tone: "blue" | "green" | "purple" | "orange" | "red";
  iconKey: KpiIconKey;
};

export type ProviderDashboardItem = ProviderSummary & {
  accountCount: number;
  readyCount: number;
  isConnected: boolean;
};

export type DashboardModel = {
  mode: DashboardMode;
  title: string;
  subtitle: string;
  statusLabel: string;
  statusTone: "good" | "warn" | "bad" | "neutral";
  kpis: DashboardKpi[];
  connectedProviders: ProviderDashboardItem[];
  availableProviders: ProviderDashboardItem[];
  providerCount: number;
  monitorCount: number;
  accountsTotal: number;
  accountsReady: number;
  latestLogLine: string;
  nextDebug: boolean;
  nextRoutingStrategy: "round-robin" | "fill-first";
  nextRequestLog: boolean;
  nextRetryCount: number;
};

export function buildDashboardModel(
  appState: AppState,
  t: TranslateFn,
  localAccounts: AuthFile[] = [],
): DashboardModel {
  const proxyAuthFiles = appState.management.auth_files ?? [];
  // Use the local auth dir when the proxy's /auth-files is empty so accounts
  // show immediately (no waiting on the network quota fetch).
  const authFiles = proxyAuthFiles.length > 0 ? proxyAuthFiles : localAccounts;
  const quotas = appState.quotas ?? [];
  const requestLogs = appState.logs ?? [];
  const usage = appState.management.usage?.usage;
  const config = appState.management.config;
  // CLIProxyAPI no longer ships built-in usage stats, so derive real request
  // metrics from the structured request logs (drained from /usage-queue).
  const hasLogs = requestLogs.length > 0;
  const totalRequests = hasLogs ? requestLogs.length : usage?.total_requests ?? 0;
  const successCount = hasLogs
    ? requestLogs.filter((entry) => {
        const code = entry.status_code ?? 0;
        return code >= 200 && code < 300;
      }).length
    : usage?.success_count ?? 0;
  const failureCount = hasLogs
    ? Math.max(0, totalRequests - successCount)
    : appState.management.usage?.failed_requests ?? usage?.failure_count ?? 0;
  const inputTokens = hasLogs
    ? requestLogs.reduce((sum, entry) => sum + (entry.input_tokens ?? 0), 0)
    : usage?.input_tokens ?? 0;
  const outputTokens = hasLogs
    ? requestLogs.reduce((sum, entry) => sum + (entry.output_tokens ?? 0), 0)
    : usage?.output_tokens ?? 0;
  const totalTokens = hasLogs ? inputTokens + outputTokens : usage?.total_tokens ?? inputTokens + outputTokens;
  // Accounts: prefer real quota accounts (read from the auth dir) over the
  // proxy's auth-files list, which is empty when the proxy isn't connected.
  const accountsTotal = quotas.length > 0 ? quotas.length : authFiles.length;
  const accountsReady =
    quotas.length > 0 ? quotas.filter((account) => !account.is_forbidden).length : authFiles.filter(isReadyAccount).length;
  const successRate = totalRequests > 0 ? Math.round((successCount / totalRequests) * 100) : 0;
  const providerItems = appState.providers.map((provider) => providerItem(provider, authFiles, quotas));
  const connectedProviders = providerItems.filter((provider) => provider.role === "provider" && provider.isConnected);
  const availableProviders = providerItems.filter((provider) => provider.role === "provider" && !provider.isConnected);
  const logLines = appState.management.logs?.lines ?? [];

  return {
    mode: dashboardMode(appState),
    ...dashboardCopy(appState.proxy.status, appState.settings.connection_mode),
    kpis: [
      {
        title: t("dash.kpi.accounts"),
        value: String(accountsTotal),
        caption: `${accountsReady} ${t("dash.ready")}`,
        tone: "blue",
        iconKey: "accounts",
      },
      {
        title: t("dash.kpi.requests"),
        value: formatCompact(totalRequests),
        caption: `${failureCount} ${t("dash.failed")}`,
        tone: "green",
        iconKey: "requests",
      },
      {
        title: t("dash.kpi.tokens"),
        value: formatCompact(totalTokens),
        caption: `${formatCompact(inputTokens)} ${t("dash.in")} · ${formatCompact(outputTokens)} ${t("dash.out")}`,
        tone: "purple",
        iconKey: "tokens",
      },
      {
        title: t("dash.kpi.success"),
        value: `${successRate}%`,
        caption: `${successCount} ${t("dash.successful")}`,
        tone: successRate >= 90 || totalRequests === 0 ? "orange" : "red",
        iconKey: "success",
      },
    ],
    connectedProviders,
    availableProviders,
    providerCount: appState.providers.filter((provider) => provider.role === "provider").length,
    monitorCount: appState.providers.filter((provider) => provider.role === "monitor").length,
    accountsTotal,
    accountsReady,
    latestLogLine: logLines.length > 0 ? logLines[logLines.length - 1] : "暂无日志快照",
    nextDebug: !(config?.debug ?? false),
    nextRoutingStrategy: config?.routing_strategy === "fill-first" ? "round-robin" : "fill-first",
    nextRequestLog: !(config?.request_log ?? false),
    nextRetryCount: config?.request_retry === 3 ? 4 : 3,
  };
}

function dashboardMode(appState: AppState): DashboardMode {
  if (appState.settings.connection_mode === "remote") return "remote";
  if (appState.proxy.status === "missing_binary") return "missing_binary";
  if (appState.proxy.status === "running") return "running";
  if (appState.proxy.status === "crashed" || appState.proxy.status === "error") return "error";
  return "stopped";
}

function dashboardCopy(status: ProxyStatusKind, connectionMode: string) {
  if (connectionMode === "remote") {
    return {
      title: "远程管理 Dashboard",
      subtitle: "当前通过远程 Management API 汇总代理、账号和请求状态。",
      statusLabel: "remote",
      statusTone: "neutral" as const,
    };
  }

  switch (status) {
    case "missing_binary":
      return {
        title: "需要安装代理核心",
        subtitle: "当前平台缺少 CLIProxyAPI 二进制，放入资源目录后即可启动本地代理。",
        statusLabel: "missing binary",
        statusTone: "bad" as const,
      };
    case "running":
      return {
        title: "Dashboard",
        subtitle: "本地代理正在运行，可以刷新管理快照查看账号、请求和配置状态。",
        statusLabel: "running",
        statusTone: "good" as const,
      };
    case "crashed":
    case "error":
      return {
        title: "代理状态异常",
        subtitle: "代理进程异常退出或状态不可读，可以重启后再刷新健康检查。",
        statusLabel: status,
        statusTone: "bad" as const,
      };
    default:
      return {
        title: "启动本地代理",
        subtitle: "代理尚未运行。启动后 Dashboard 会展示 Management API 返回的实时状态。",
        statusLabel: status,
        statusTone: "warn" as const,
      };
  }
}

function providerItem(
  provider: ProviderSummary,
  authFiles: AuthFile[],
  quotas: AccountQuota[],
): ProviderDashboardItem {
  const authAccounts = authFiles.filter((file) => file.provider === provider.id || provider.id.includes(file.provider));
  const quotaAccounts = quotas.filter((account) => account.provider_id === provider.id);
  const accountCount = Math.max(authAccounts.length, quotaAccounts.length);
  const readyCount =
    quotaAccounts.length > 0
      ? quotaAccounts.filter((account) => !account.is_forbidden).length
      : authAccounts.filter(isReadyAccount).length;
  return {
    ...provider,
    accountCount,
    readyCount,
    isConnected: accountCount > 0,
  };
}

function isReadyAccount(file: AuthFile) {
  return (file.status === "ready" || file.status === "local") && !file.disabled && !file.unavailable;
}

function formatCompact(value: number) {
  return new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }).format(value);
}