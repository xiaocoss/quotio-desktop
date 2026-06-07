import type { DashboardModel } from "../../state/dashboardModel";
import type { AppState, ProxyCommand } from "../../types";

type ProxyStatusPanelProps = {
  appState: AppState;
  model: DashboardModel;
  isProxyBusy: boolean;
  proxyAction: string | null;
  onRunProxyAction: (command: ProxyCommand) => void;
};

export function ProxyStatusPanel({ appState, model, isProxyBusy, proxyAction, onRunProxyAction }: ProxyStatusPanelProps) {
  const canStart = appState.proxy.status !== "running" && appState.proxy.status !== "missing_binary";
  const canStop = appState.proxy.status === "running";

  return (
    <article className="panel proxy-panel">
      <div className="panel-header">
        <div>
          <p className="eyebrow">Runtime</p>
          <h2>代理状态</h2>
        </div>
        <span className={`status-pill status-pill--${appState.proxy.status}`}>{appState.proxy.status}</span>
      </div>

      <div className={`state-banner state-banner--${model.statusTone}`}>
        <strong>{model.title}</strong>
        <p>{model.subtitle}</p>
      </div>

      <dl className="detail-list compact-details">
        <div>
          <dt>代理地址</dt>
          <dd>{appState.proxy.endpoint}</dd>
        </div>
        <div>
          <dt>管理接口</dt>
          <dd>{appState.proxy.management_endpoint}</dd>
        </div>
        <div>
          <dt>进程</dt>
          <dd>{appState.proxy.pid ? `PID ${appState.proxy.pid}` : "未运行"}</dd>
        </div>
        <div>
          <dt>健康状态</dt>
          <dd>{appState.proxy.health.message}</dd>
        </div>
        <div>
          <dt>资源目录</dt>
          <dd>{appState.proxy.resource_dir ?? "未解析"}</dd>
        </div>
        <div>
          <dt>代理二进制</dt>
          <dd>{appState.proxy.binary_path ?? "未解析"}</dd>
        </div>
      </dl>

      <div className="action-row">
        <button
          className="secondary-action"
          type="button"
          onClick={() => onRunProxyAction("start_proxy")}
          disabled={isProxyBusy || !canStart}
        >
          {proxyAction === "start_proxy" ? "启动中..." : "启动代理"}
        </button>
        <button
          className="secondary-action"
          type="button"
          onClick={() => onRunProxyAction("stop_proxy")}
          disabled={isProxyBusy || !canStop}
        >
          {proxyAction === "stop_proxy" ? "停止中..." : "停止代理"}
        </button>
        <button
          className="secondary-action"
          type="button"
          onClick={() => onRunProxyAction("restart_proxy")}
          disabled={isProxyBusy || appState.proxy.status === "missing_binary"}
        >
          {proxyAction === "restart_proxy" ? "重启中..." : "重启代理"}
        </button>
        <button
          className="secondary-action"
          type="button"
          onClick={() => onRunProxyAction("check_proxy_health")}
          disabled={isProxyBusy || appState.proxy.status === "missing_binary"}
        >
          {proxyAction === "check_proxy_health" ? "检查中..." : "健康检查"}
        </button>
      </div>
    </article>
  );
}