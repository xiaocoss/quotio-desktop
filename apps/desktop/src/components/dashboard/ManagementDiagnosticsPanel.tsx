import type { DashboardModel } from "../../state/dashboardModel";
import type { AppState } from "../../types";

type ManagementDiagnosticsPanelProps = {
  appState: AppState;
  model: DashboardModel;
  managementAction: string | null;
  proxyUrlDraft: string;
  isManagementBusy: boolean;
  onProxyUrlDraftChange: (value: string) => void;
  onRefreshProxyUrlDraft: () => void;
  onRunManagementStateAction: (command: string, args?: Record<string, unknown>) => void;
};

export function ManagementDiagnosticsPanel({
  appState,
  model,
  managementAction,
  proxyUrlDraft,
  isManagementBusy,
  onProxyUrlDraftChange,
  onRefreshProxyUrlDraft,
  onRunManagementStateAction,
}: ManagementDiagnosticsPanelProps) {
  const config = appState.management.config;

  return (
    <article className="panel management-panel">
      <div className="panel-header">
        <div>
          <p className="eyebrow">Diagnostics</p>
          <h2>管理接口诊断</h2>
        </div>
        <span className="count-pill">{appState.proxy.management_endpoint}</span>
      </div>

      <dl className="detail-list management-details">
        <div>
          <dt>Debug</dt>
          <dd>{formatBool(config?.debug)}</dd>
        </div>
        <div>
          <dt>Routing</dt>
          <dd>{config?.routing_strategy ?? "未刷新"}</dd>
        </div>
        <div>
          <dt>Proxy URL</dt>
          <dd>{config?.proxy_url || "直连"}</dd>
        </div>
        <div>
          <dt>Request retry</dt>
          <dd>{config?.request_retry ?? "未刷新"}</dd>
        </div>
        <div>
          <dt>Request log</dt>
          <dd>{formatBool(config?.request_log)}</dd>
        </div>
        <div>
          <dt>Latest log</dt>
          <dd>{model.latestLogLine}</dd>
        </div>
      </dl>

      <div className="management-actions">
        <button
          className="secondary-action"
          type="button"
          onClick={() => onRunManagementStateAction("refresh_management_state")}
          disabled={isManagementBusy}
        >
          {managementAction === "refresh_management_state" ? "刷新中..." : "刷新管理快照"}
        </button>
        <button
          className="secondary-action"
          type="button"
          onClick={() => onRunManagementStateAction("set_management_debug", { enabled: model.nextDebug })}
          disabled={isManagementBusy}
        >
          {managementAction === "set_management_debug" ? "写入中..." : `Debug ${model.nextDebug ? "开启" : "关闭"}`}
        </button>
        <button
          className="secondary-action"
          type="button"
          onClick={() => onRunManagementStateAction("set_management_routing_strategy", { strategy: model.nextRoutingStrategy })}
          disabled={isManagementBusy}
        >
          {managementAction === "set_management_routing_strategy" ? "写入中..." : `切到 ${model.nextRoutingStrategy}`}
        </button>
        <button
          className="secondary-action"
          type="button"
          onClick={() => onRunManagementStateAction("set_management_request_log", { enabled: model.nextRequestLog })}
          disabled={isManagementBusy}
        >
          {managementAction === "set_management_request_log" ? "写入中..." : `Request log ${model.nextRequestLog ? "开启" : "关闭"}`}
        </button>
        <button
          className="secondary-action"
          type="button"
          onClick={() => onRunManagementStateAction("set_management_request_retry", { count: model.nextRetryCount })}
          disabled={isManagementBusy}
        >
          {managementAction === "set_management_request_retry" ? "写入中..." : `Retry ${model.nextRetryCount}`}
        </button>
        <button
          className="secondary-action"
          type="button"
          onClick={() => onRunManagementStateAction("clear_management_logs")}
          disabled={isManagementBusy}
        >
          {managementAction === "clear_management_logs" ? "清理中..." : "清空日志"}
        </button>
      </div>

      <div className="proxy-url-row">
        <input
          type="text"
          value={proxyUrlDraft}
          onChange={(event) => onProxyUrlDraftChange(event.target.value)}
          placeholder="http://127.0.0.1:7890"
        />
        <button className="secondary-action" type="button" onClick={onRefreshProxyUrlDraft} disabled={isManagementBusy}>
          读取 Proxy URL
        </button>
        <button
          className="secondary-action"
          type="button"
          onClick={() => onRunManagementStateAction("set_management_proxy_url", { url: proxyUrlDraft })}
          disabled={isManagementBusy || proxyUrlDraft.trim().length === 0}
        >
          写入 Proxy URL
        </button>
        <button
          className="secondary-action"
          type="button"
          onClick={() => onRunManagementStateAction("clear_management_proxy_url")}
          disabled={isManagementBusy}
        >
          清空 Proxy URL
        </button>
      </div>
    </article>
  );
}

function formatBool(value: boolean | null | undefined) {
  if (value === null || value === undefined) return "未刷新";
  return value ? "已开启" : "已关闭";
}