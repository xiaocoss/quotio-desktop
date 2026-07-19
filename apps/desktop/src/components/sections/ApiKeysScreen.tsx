import { useState, useEffect } from "react";
import type { ApiKeyBinding, ApiKeyEntry, AppState } from "../../types";
import { CheckIcon, CopyIcon, KeyIcon, PencilIcon, PlusIcon, TrashIcon } from "../icons";
import { useT } from "../../i18n";
import { invoke } from "../../lib/tauri";
import "./api-keys-floral.css";

type CustomProviderOption = { id: string; name: string };

type ApiKeysScreenProps = {
  appState: AppState;
  isManagementBusy: boolean;
  managementAction: string | null;
  onRefreshManagement: () => void;
  onRunManagementStateAction: (command: string, args?: Record<string, unknown>) => void;
};

export function ApiKeysScreen({ appState, isManagementBusy, onRunManagementStateAction }: ApiKeysScreenProps) {
  const t = useT();
  const apiKeys = appState.api_keys;
  const bindings = appState.api_key_bindings ?? [];

  const [showAdd, setShowAdd] = useState(false);
  const [newApiKey, setNewApiKey] = useState("");
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [replacementApiKey, setReplacementApiKey] = useState("");
  const [keyQuery, setKeyQuery] = useState("");
  const [copiedConnection, setCopiedConnection] = useState<string | null>(null);

  const builtinProviders: CustomProviderOption[] = appState.providers.map((p) => ({ id: p.id, name: p.display_name }));
  const [customProviders, setCustomProviders] = useState<CustomProviderOption[]>([]);
  useEffect(() => {
    void invoke<{ id: string; name: string }[]>("list_custom_providers")
      .then((list) => setCustomProviders(list.map((p) => ({ id: p.id, name: p.name }))))
      .catch((err) => console.warn("[ApiKeysScreen] list_custom_providers:", err));
  }, []);
  const allProviders: CustomProviderOption[] = [...builtinProviders, ...customProviders];

  const [keyRouterAvailable, setKeyRouterAvailable] = useState<boolean | null>(null);
  useEffect(() => {
    void invoke<boolean>("key_router_available").then(setKeyRouterAvailable).catch((err) => console.warn("[ApiKeysScreen] key_router_available:", err));
  }, []);
  const hasBoundKeys = bindings.some((b) => Boolean(b.provider_id));
  const boundCount = apiKeys.filter((entry) => Boolean(bindingFor(entry.value))).length;
  const globalCount = Math.max(0, apiKeys.length - boundCount);
  const proxyEndpoint = appState.proxy.endpoint || `http://${appState.settings.proxy_host}:${appState.settings.proxy_port}`;
  const visibleApiKeys = apiKeys.filter((entry) => {
    const query = keyQuery.trim().toLowerCase();
    if (!query) return true;
    const providerName = allProviders.find((provider) => provider.id === bindingFor(entry.value))?.name ?? "";
    return `${entry.masked_value} ${providerName}`.toLowerCase().includes(query);
  });

  async function copyConnection(label: string, value: string) {
    try {
      await navigator.clipboard.writeText(value);
      setCopiedConnection(label);
      window.setTimeout(() => setCopiedConnection(null), 1200);
    } catch {
      setCopiedConnection(null);
    }
  }

  function addKey() {
    const value = newApiKey.trim();
    if (!value) return;
    onRunManagementStateAction("add_api_key", { key: value });
    setNewApiKey("");
    setShowAdd(false);
  }

  function bindingFor(keyValue: string): string {
    return bindings.find((b) => b.api_key === keyValue)?.provider_id ?? "";
  }

  async function onBindingChange(keyValue: string, providerId: string) {
    try {
      await invoke<ApiKeyBinding[]>("set_api_key_binding", { apiKey: keyValue, providerId });
      onRunManagementStateAction("refresh_management_state");
    } catch { /* ignore */ }
  }

  return (
    <section className="section-page api-keys-page api-keys-redesign">
      <header className="page-topbar" data-tauri-drag-region>
        <div className="apikey-title-block" data-tauri-drag-region="false">
          <div className="rose-apikey-title-line">
            <h1>{t("nav.api_keys")}</h1>
            <span className="rose-apikey-title-badge"><KeyIcon /> 本地代理访问</span>
          </div>
          <p>管理客户端访问凭据与服务商路由分配</p>
        </div>
        <div className="topbar-actions">
          <button
            className="icon-button apikey-generate-button"
            type="button"
            onClick={() => {
              setShowAdd(true);
              setNewApiKey(generateApiKey());
            }}
            disabled={isManagementBusy}
            title="生成密钥"
            aria-label="生成密钥"
          >
            <KeyIcon />
            <span>生成密钥</span>
          </button>
          <button
            className={showAdd ? "icon-button icon-button--active apikey-create-button" : "icon-button apikey-create-button"}
            type="button"
            onClick={() => setShowAdd((value) => !value)}
            disabled={isManagementBusy}
            title="新增密钥"
            aria-label="新增密钥"
          >
            <PlusIcon />
            <span>新增密钥</span>
          </button>
          <span className="rose-apikey-header-avatar" aria-hidden="true"><img src="/rose/character-avatar.png" alt="" /></span>
        </div>
      </header>

      <section className="rose-apikey-metrics" aria-label="API 密钥概览">
        <article><span className="rose-apikey-metric-icon rose-apikey-metric-icon--key"><KeyIcon /></span><div><small>密钥总数</small><strong>{apiKeys.length}<em> 个</em></strong><p>当前可用于客户端认证</p></div></article>
        <article><span className="rose-apikey-metric-icon rose-apikey-metric-icon--bound"><RouteNodesIcon /></span><div><small>已绑定路由</small><strong>{boundCount}<em> 个</em></strong><p>请求仅进入指定服务商</p></div></article>
        <article><span className="rose-apikey-metric-icon rose-apikey-metric-icon--global"><GlobeIcon /></span><div><small>全局路由</small><strong>{globalCount}<em> 个</em></strong><p>跟随代理的全局策略</p></div></article>
        <article><span className="rose-apikey-metric-icon rose-apikey-metric-icon--router"><WarningTriangleIcon /></span><div><small>路由服务</small><strong className={keyRouterAvailable === false ? "is-warning" : "is-healthy"}>{keyRouterAvailable === false ? "需处理" : keyRouterAvailable === null ? "检测中" : "已就绪"}</strong><p>{keyRouterAvailable === false ? "插件缺失，绑定暂不生效" : "按密钥分配服务商"}</p></div></article>
      </section>

      <div className="apikey-content-grid">
      <article className="panel api-keys-panel">
        <div className="panel-label apikey-panel-heading">
          <div>
            <span className="eyebrow apikey-panel-label-default">{t("nav.api_keys")}</span>
            <span className="eyebrow apikey-panel-label-rose">CLIENT CREDENTIALS</span>
            <h2>客户端密钥</h2>
            <p>用于连接 Quotio 本地代理服务</p>
          </div>
          <span className="count-pill">{apiKeys.length} 个</span>
        </div>

        <div className="rose-apikey-toolbar">
          <label><span>⌕</span><input value={keyQuery} onChange={(event) => setKeyQuery(event.target.value)} placeholder="搜索密钥或已绑定服务商" /></label>
          <div><span className="rose-apikey-legend-dot rose-apikey-legend-dot--bound" /> 已绑定 <strong>{boundCount}</strong><span className="rose-apikey-legend-dot" /> 全局 <strong>{globalCount}</strong></div>
        </div>

        {hasBoundKeys && keyRouterAvailable === false ? (
          <div className="apikey-router-warning">
            ⚠ 你给密钥绑定了服务商,但当前运行环境<strong>缺少「按 key 路由」插件</strong>(quotio-key-router)——
            绑定<strong>不会生效</strong>,代理仍按全局轮询命中所有可用池,请求可能落到你没想绑的服务商。
            请用随包内置该插件的版本(0.4.x 安装包),或确认代理目录 <code>plugins/</code> 里有该插件。
          </div>
        ) : null}

        {showAdd ? (
          <div className="apikey-add">
            <input
              type="text"
              value={newApiKey}
              onChange={(event) => setNewApiKey(event.target.value)}
              placeholder="sk-..."
              autoFocus
            />
            <button className="ghost-action" type="button" onClick={() => setNewApiKey(generateApiKey())} disabled={isManagementBusy}>
              生成
            </button>
            <button className="secondary-action" type="button" onClick={addKey} disabled={isManagementBusy || newApiKey.trim().length === 0}>
              保存
            </button>
          </div>
        ) : null}

        {apiKeys.length === 0 ? (
          <>
            <p className="empty-copy apikey-empty-default">暂无密钥。点击右上角生成或新增一个客户端密钥。</p>
            <div className="apikey-empty-state">
              <span><KeyIcon /></span>
              <strong>还没有客户端密钥</strong>
              <p>点击右上角生成一把安全密钥，开始连接代理服务。</p>
            </div>
          </>
        ) : visibleApiKeys.length === 0 ? (
          <div className="rose-apikey-no-results">没有找到匹配的密钥</div>
        ) : (
          <div className="apikey-list">
            {visibleApiKeys.map((entry) => {
              const index = apiKeys.indexOf(entry);
              return (
              <ApiKeyRow
                key={`${index}-${entry.masked_value}`}
                index={index}
                entry={entry}
                rawValue={entry.value}
                isEditing={editingIndex === index}
                replacementValue={replacementApiKey}
                isBusy={isManagementBusy}
                boundProviderId={bindingFor(entry.value)}
                providers={allProviders}
                onReplacementChange={setReplacementApiKey}
                onBindingChange={(providerId) => void onBindingChange(entry.value, providerId)}
                onEdit={() => {
                  setEditingIndex(index);
                  setReplacementApiKey("");
                }}
                onCancel={() => {
                  setEditingIndex(null);
                  setReplacementApiKey("");
                }}
                onSave={() => {
                  const replacement = replacementApiKey.trim();
                  if (!replacement) return;
                  onRunManagementStateAction("update_api_key", { key: entry.value, replacement });
                  setEditingIndex(null);
                  setReplacementApiKey("");
                }}
                onDelete={() => onRunManagementStateAction("remove_api_key", { key: entry.value })}
              />
              );
            })}
          </div>
        )}

        <p className="empty-copy apikey-foot">{t("apikeys.foot")}</p>
      </article>

      <aside className="rose-apikey-side-stack" aria-label="连接与路由信息">
        <section className="rose-apikey-side-panel rose-apikey-connect-panel">
          <header><span><KeyIcon /></span><div><h2>连接配置</h2><p>在客户端中填写以下信息</p></div></header>
          <div className="rose-apikey-config-field"><label>代理端点</label><div><code>{proxyEndpoint}</code><button type="button" onClick={() => void copyConnection("endpoint", proxyEndpoint)} aria-label="复制代理端点">{copiedConnection === "endpoint" ? <CheckIcon /> : <CopyIcon />}</button></div></div>
          <div className="rose-apikey-config-field"><label>认证请求头</label><div><code>Authorization: Bearer sk-...</code><button type="button" onClick={() => void copyConnection("header", "Authorization: Bearer YOUR_API_KEY")} aria-label="复制认证请求头">{copiedConnection === "header" ? <CheckIcon /> : <CopyIcon />}</button></div></div>
          <div className="rose-apikey-env">
            <span><strong>环境变量（可选）</strong><small>将以下变量配置到你的客户端环境中</small></span>
            <div><code>OPENAI_BASE_URL</code><button type="button" onClick={() => void copyConnection("base-env", `OPENAI_BASE_URL=${proxyEndpoint}`)} aria-label="复制 OPENAI_BASE_URL">{copiedConnection === "base-env" ? <CheckIcon /> : <CopyIcon />}</button></div>
            <div><code>OPENAI_API_KEY</code><button type="button" onClick={() => void copyConnection("key-env", "OPENAI_API_KEY=YOUR_API_KEY")} aria-label="复制 OPENAI_API_KEY">{copiedConnection === "key-env" ? <CheckIcon /> : <CopyIcon />}</button></div>
          </div>
        </section>

        <section className="rose-apikey-side-panel rose-apikey-route-panel">
          <header><span>↗</span><div><h2>路由健康</h2><p>密钥与服务商的分配状态</p></div></header>
          <div className="rose-apikey-health-row"><span className={keyRouterAvailable === false ? "warning" : "healthy"}>{keyRouterAvailable === false ? "!" : "✓"}</span><div><strong>按 Key 路由</strong><small>{keyRouterAvailable === false ? "插件缺失，需安装 quotio-key-router" : keyRouterAvailable === null ? "正在检测运行环境" : "路由插件工作正常"}</small></div><em>{keyRouterAvailable === false ? "需处理" : "正常"}</em><b>›</b></div>
          <div className="rose-apikey-health-row"><span className="healthy">✓</span><div><strong>密钥分配</strong><small>{boundCount} 个指定路由，{globalCount} 个使用全局路由</small></div><em>已同步</em><b>›</b></div>
          <div className="rose-apikey-health-row"><span className="healthy">✓</span><div><strong>代理端点</strong><small>{appState.proxy.status === "running" ? "本地代理正在运行" : "代理当前未运行"}</small></div><em>{appState.proxy.status === "running" ? "在线" : "离线"}</em><b>›</b></div>
        </section>
      </aside>
      </div>

      <section className="rose-apikey-guide" aria-label="客户端接入步骤">
        <header><div><h2>客户端接入</h2><p>三步完成 Quotio 代理认证</p></div><span>凭据仅应填写在受信任的客户端中</span></header>
        <div>
          <article><b>01</b><span><KeyIcon /></span><div><strong>创建访问密钥</strong><p>生成新密钥，或录入已有的客户端凭据。</p></div></article>
          <article><b>02</b><span><RouteNodesIcon /></span><div><strong>选择服务商路由</strong><p>按需指定服务商；留空则跟随全局路由策略。</p></div></article>
          <article><b>03</b><span><PaperPlaneIcon /></span><div><strong>配置并开始调用</strong><p>填入代理端点和 API Key，即可通过 Quotio 发起请求。</p></div></article>
        </div>
      </section>
    </section>
  );
}

function ApiKeyRow({
  index,
  entry,
  rawValue,
  isEditing,
  replacementValue,
  isBusy,
  boundProviderId,
  providers,
  onReplacementChange,
  onBindingChange,
  onEdit,
  onCancel,
  onSave,
  onDelete,
}: {
  index: number;
  entry: ApiKeyEntry;
  rawValue: string;
  isEditing: boolean;
  replacementValue: string;
  isBusy: boolean;
  boundProviderId: string;
  providers: CustomProviderOption[];
  onReplacementChange: (value: string) => void;
  onBindingChange: (providerId: string) => void;
  onEdit: () => void;
  onCancel: () => void;
  onSave: () => void;
  onDelete: () => void;
}) {
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(rawValue);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch {
      setCopied(false);
    }
  }

  const boundName = providers.find((p) => p.id === boundProviderId)?.name;

  return (
    <div className="apikey-row">
      <div className="rose-apikey-row-identity">
        <span>{index + 1}</span>
        <div><strong>客户端密钥 {String(index + 1).padStart(2, "0")}</strong><small className={boundName ? "is-bound" : "is-global"}>{boundName ? `已绑定 ${boundName}` : "全局路由"}</small></div>
      </div>
      <div className="apikey-row-main">
        <code className="apikey-value">{entry.masked_value}</code>
        {providers.length > 0 ? (
          <select
            className="apikey-binding-select"
            value={boundProviderId}
            onChange={(e) => onBindingChange(e.target.value)}
          >
            <option value="">全部服务商</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
        ) : boundName ? (
          <span className="apikey-binding-badge">{boundName}</span>
        ) : null}
      </div>
      {isEditing ? (
        <div className="apikey-edit">
          <input
            type="text"
            value={replacementValue}
            onChange={(event) => onReplacementChange(event.target.value)}
            placeholder="新的 API key"
            autoFocus
          />
          <button className="secondary-action" type="button" onClick={onSave} disabled={isBusy || replacementValue.trim().length === 0}>
            保存
          </button>
          <button className="ghost-action" type="button" onClick={onCancel} disabled={isBusy}>
            取消
          </button>
        </div>
      ) : (
        <div className="apikey-actions">
          <button className="row-icon-btn" type="button" onClick={copy} title="复制密钥" aria-label="复制密钥">
            {copied ? <CheckIcon /> : <CopyIcon />}
          </button>
          <button className="row-icon-btn" type="button" onClick={onEdit} disabled={isBusy} title="替换密钥" aria-label="替换密钥">
            <PencilIcon />
          </button>
          <button className="row-icon-btn row-icon-btn--danger" type="button" onClick={onDelete} disabled={isBusy} title="删除密钥" aria-label="删除密钥">
            <TrashIcon />
          </button>
        </div>
      )}
    </div>
  );
}

function RouteNodesIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <circle cx="6" cy="12" r="2.5" stroke="currentColor" strokeWidth="1.8" />
      <circle cx="18" cy="6" r="2.5" stroke="currentColor" strokeWidth="1.8" />
      <circle cx="18" cy="18" r="2.5" stroke="currentColor" strokeWidth="1.8" />
      <path d="M8.3 10.8 15.7 7.2M8.3 13.2l7.4 3.6" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

function GlobeIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <circle cx="12" cy="12" r="8.5" stroke="currentColor" strokeWidth="1.8" />
      <path d="M3.8 12h16.4M12 3.5c2.2 2.3 3.3 5.1 3.3 8.5S14.2 18.2 12 20.5C9.8 18.2 8.7 15.4 8.7 12S9.8 5.8 12 3.5Z" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
    </svg>
  );
}

function WarningTriangleIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path d="M12 4.2 21 19.5H3L12 4.2Z" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" />
      <path d="M12 9v5.2M12 17.4v.2" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

function PaperPlaneIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path d="m3.5 10.2 16.8-6.1-6.1 16.8-3.1-7.8-7.6-2.9Z" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round" />
      <path d="m11.1 13.1 5.1-5.1" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
    </svg>
  );
}

function generateApiKey() {
  const alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
  const randomPart = Array.from({ length: 32 }, () => alphabet[Math.floor(Math.random() * alphabet.length)]).join("");
  return `sk-${randomPart}`;
}
