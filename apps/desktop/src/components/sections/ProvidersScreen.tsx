import { useState, useEffect, useRef, type ChangeEvent } from "react";
import type { AppState, AuthFile, OAuthStatusResponse, OAuthUrlResponse, ProviderSummary } from "../../types";
import { maskEmail } from "../../lib/format";
import { PlusIcon, RefreshIcon, TrashIcon } from "../icons";
import { useT } from "../../i18n";
import { invoke } from "../../lib/tauri";
import { Select } from "../Select";

type CustomProvider = { id: string; name: string; base_url: string; api_key: string; kind: string; prefix?: string };

type ProvidersScreenProps = {
  appState: AppState;
  isManagementBusy: boolean;
  managementAction: string | null;
  onRefreshManagement: () => void;
  onRunManagementStateAction: (command: string, args?: Record<string, unknown>) => void;
  onStartOAuth: (endpoint: string, projectId: string | null, isWebui?: boolean) => Promise<OAuthUrlResponse | null>;
  onPollOAuth: (token: string) => Promise<OAuthStatusResponse | null>;
};

type OAuthSession = {
  providerName: string;
  url: string | null;
  state: string | null;
  status: string;
  error: string | null;
};

type AccountGroupData = {
  id: string;
  label: string;
  colorHex: string;
  accounts: AuthFile[];
};

export function ProvidersScreen({
  appState,
  isManagementBusy,
  managementAction,
  onRefreshManagement,
  onRunManagementStateAction,
  onStartOAuth,
  onPollOAuth,
}: ProvidersScreenProps) {
  const t = useT();
  const proxyAuthFiles = appState.management.auth_files ?? [];
  // Fall back to the local auth dir so existing accounts show even when the
  // proxy (and its /auth-files) isn't connected.
  const [localAccounts, setLocalAccounts] = useState<AuthFile[]>([]);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    void invoke<AuthFile[]>("list_local_accounts").then(setLocalAccounts).catch(() => {});
  }, [appState.management.auth_files]);
  const authFiles = proxyAuthFiles.length > 0 ? proxyAuthFiles : localAccounts;
  const groups = groupAccounts(authFiles, appState.providers);
  const oauthProviders = appState.providers.filter((provider) => provider.oauth_endpoint);
  const vertexProvider = appState.providers.find((provider) => provider.id === "vertex");

  const [showAdd, setShowAdd] = useState(false);
  const [projectId, setProjectId] = useState("");
  const [oauthSession, setOAuthSession] = useState<OAuthSession | null>(null);
  const [vertexJson, setVertexJson] = useState("");
  const [vertexError, setVertexError] = useState<string | null>(null);
  const [showAddCustom, setShowAddCustom] = useState(false);
  const [editingCustomId, setEditingCustomId] = useState<string | null>(null);
  const [customForm, setCustomForm] = useState({ name: "", base_url: "", api_key: "", kind: "openai", prefix: "" });
  const [customProviders, setCustomProviders] = useState<CustomProvider[]>([]);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    void invoke<CustomProvider[]>("list_custom_providers").then(setCustomProviders).catch(() => {});
  }, []);

  // Tracks the OAuth `state` currently being auto-polled, so starting a new
  // authorization (or unmounting) cancels the previous polling loop.
  const pollRef = useRef<string | null>(null);
  useEffect(
    () => () => {
      pollRef.current = null;
    },
    [],
  );

  // Open the authorization URL in the system browser (Tauri opener, falling back
  // to window.open), mirroring the macOS reference app's auto-open behavior.
  async function openAuthUrl(url: string) {
    try {
      if ("__TAURI_INTERNALS__" in window) {
        const { openUrl } = await import("@tauri-apps/plugin-opener");
        await openUrl(url);
        return;
      }
    } catch {
      /* fall back to window.open below */
    }
    window.open(url, "_blank", "noreferrer");
  }

  // Auto-poll the OAuth status (mirrors the macOS reference app's pollOAuthStatus):
  // every 2s, up to ~2 min, until the proxy reports "ok" (success) or "error".
  // onPollOAuth refreshes the management snapshot on success, so the new account
  // appears without the user clicking "poll" manually.
  async function autoPollOAuth(state: string) {
    pollRef.current = state;
    for (let attempt = 0; attempt < 60; attempt += 1) {
      await new Promise((resolve) => window.setTimeout(resolve, 2000));
      if (pollRef.current !== state) return; // superseded by a new auth, or unmounted
      const response = await onPollOAuth(state);
      if (pollRef.current !== state) return;
      if (!response) continue;
      setOAuthSession((current) =>
        current && current.state === state ? { ...current, status: response.status, error: response.error } : current,
      );
      if (["ok", "success", "completed"].includes(response.status)) {
        pollRef.current = null;
        return;
      }
      if (response.status === "error") {
        pollRef.current = null;
        return;
      }
    }
    setOAuthSession((current) =>
      current && current.state === state ? { ...current, status: "error", error: "OAuth 授权超时，请重试。" } : current,
    );
    pollRef.current = null;
  }

  function resetCustomForm() {
    setCustomForm({ name: "", base_url: "", api_key: "", kind: "openai", prefix: "" });
    setEditingCustomId(null);
    setShowAddCustom(false);
  }

  function startEditCustom(provider: CustomProvider) {
    setCustomForm({
      name: provider.name,
      base_url: provider.base_url,
      api_key: provider.api_key,
      kind: provider.kind || "openai",
      prefix: provider.prefix ?? "",
    });
    setEditingCustomId(provider.id);
    setShowAddCustom(true);
  }

  async function submitCustomProvider() {
    if (!customForm.name.trim() || !customForm.base_url.trim()) return;
    try {
      const command = editingCustomId ? "update_custom_provider" : "add_custom_provider";
      const args = editingCustomId ? { id: editingCustomId, ...customForm } : customForm;
      setCustomProviders(await invoke<CustomProvider[]>(command, args));
      resetCustomForm();
    } catch {
      /* surfaced elsewhere */
    }
  }

  async function removeCustomProvider(id: string) {
    try {
      setCustomProviders(await invoke<CustomProvider[]>("delete_custom_provider", { id }));
      if (editingCustomId === id) resetCustomForm();
    } catch {
      /* ignore */
    }
  }

  async function onImportFiles(event: ChangeEvent<HTMLInputElement>) {
    const files = Array.from(event.target.files ?? []);
    event.target.value = "";
    if (files.length === 0) return;
    for (const file of files) {
      try {
        const content = await file.text();
        await invoke("import_auth_file", { filename: file.name, content });
      } catch {
        /* skip invalid files */
      }
    }
    onRefreshManagement();
    if ("__TAURI_INTERNALS__" in window) {
      try {
        setLocalAccounts(await invoke<AuthFile[]>("list_local_accounts"));
      } catch {
        /* ignore */
      }
    }
  }

  return (
    <section className="section-page providers-page">
      <header className="page-topbar" data-tauri-drag-region>
        <h1>{t("nav.providers")}</h1>
        <div className="topbar-actions">
          <button
            className="icon-button"
            type="button"
            onClick={onRefreshManagement}
            disabled={isManagementBusy}
            title="刷新账号"
            aria-label="刷新账号"
          >
            <RefreshIcon />
          </button>
        </div>
      </header>

      {showAdd ? (
        <section className="section-grid providers-add-grid">
          <OAuthPanel
            providers={oauthProviders}
            projectId={projectId}
            oauthSession={oauthSession}
            isBusy={isManagementBusy}
            managementAction={managementAction}
            onProjectIdChange={setProjectId}
            onStartOAuth={async (provider) => {
              const response = await onStartOAuth(provider.oauth_endpoint ?? "", projectId, true);
              if (!response) return;
              setOAuthSession({
                providerName: provider.display_name,
                url: response.url,
                state: response.state,
                status: response.status,
                error: response.error,
              });
              // Auto-open the browser and auto-poll like the reference app, so a
              // successful authorization adds the account without extra clicks.
              if (response.url) void openAuthUrl(response.url);
              if (response.state && !response.error) void autoPollOAuth(response.state);
            }}
            onPollOAuth={async () => {
              if (!oauthSession?.state) return;
              const response = await onPollOAuth(oauthSession.state);
              if (!response) return;
              setOAuthSession({ ...oauthSession, status: response.status, error: response.error });
            }}
          />

          <VertexImportPanel
            provider={vertexProvider}
            value={vertexJson}
            error={vertexError}
            isBusy={isManagementBusy}
            onChange={(value) => {
              setVertexJson(value);
              setVertexError(null);
            }}
            onImport={() => {
              const value = vertexJson.trim();
              if (!value) return;
              try {
                JSON.parse(value);
              } catch {
                setVertexError("JSON 格式不合法。");
                return;
              }
              onRunManagementStateAction("import_management_vertex_service_account", { json: value });
              setVertexJson("");
            }}
          />
        </section>
      ) : null}

      <article className="panel accounts-panel">
        <div className="panel-label">
          <span className="eyebrow">{t("providers.yourAccounts")}</span>
          <span className="accounts-panel-right">
            <button
              className={showAdd ? "ghost-action ghost-action--active" : "ghost-action"}
              type="button"
              onClick={() => setShowAdd((value) => !value)}
            >
              {showAdd ? t("providers.closeOAuth") : t("providers.openOAuth")}
            </button>
            <label className="ghost-action import-pick" title={t("import.desc")}>
              {t("import.button")}
              <input
                type="file"
                accept=".json,application/json"
                multiple
                hidden
                disabled={isManagementBusy}
                onChange={onImportFiles}
              />
            </label>
            <span className="count-pill">{authFiles.length}</span>
          </span>
        </div>

        {groups.length === 0 ? (
          <p className="empty-copy">暂无账号快照。点击右上角 + 通过 OAuth 授权或导入 Service Account 添加账号。</p>
        ) : (
          <div className="account-groups">
            {groups.map((group) => (
              <AccountGroup
                key={group.id}
                group={group}
                isBusy={isManagementBusy}
                onDelete={(account) => onRunManagementStateAction("delete_management_auth_file", { name: account.name })}
              />
            ))}
          </div>
        )}
      </article>

      <article className="panel">
        <div className="panel-label">
          <span className="eyebrow">{t("providers.customProviders")}</span>
          <button
            className={showAddCustom ? "icon-button icon-button--active" : "icon-button"}
            type="button"
            onClick={() => (showAddCustom ? resetCustomForm() : setShowAddCustom(true))}
            title={t("providers.addCustom")}
            aria-label={t("providers.addCustom")}
          >
            <PlusIcon />
          </button>
        </div>

        {showAddCustom ? (
          <div className="stacked-form custom-provider-form">
            <label>
              {t("providers.cpName")}
              <input
                value={customForm.name}
                onChange={(event) => setCustomForm({ ...customForm, name: event.target.value })}
                placeholder="My Provider"
              />
            </label>
            <label>
              {t("providers.cpBaseUrl")}
              <input
                value={customForm.base_url}
                onChange={(event) => setCustomForm({ ...customForm, base_url: event.target.value })}
                placeholder="https://api.example.com/v1"
              />
            </label>
            <label>
              {t("providers.cpApiKey")}
              <input
                type="password"
                value={customForm.api_key}
                onChange={(event) => setCustomForm({ ...customForm, api_key: event.target.value })}
                placeholder="sk-..."
              />
            </label>
            <label>
              {t("providers.cpKind")}
              <Select
                value={customForm.kind}
                options={[
                  { value: "openai", label: "OpenAI" },
                  { value: "gemini", label: "Gemini" },
                ]}
                onChange={(value) => setCustomForm({ ...customForm, kind: value })}
              />
            </label>
            <label>
              {t("providers.cpPrefix")}
              <input
                value={customForm.prefix}
                onChange={(event) => setCustomForm({ ...customForm, prefix: event.target.value })}
                placeholder="myprovider"
              />
            </label>
            <button
              className="primary-action"
              type="button"
              onClick={() => void submitCustomProvider()}
              disabled={!customForm.name.trim() || !customForm.base_url.trim()}
            >
              {editingCustomId ? t("providers.cpSave") : t("providers.cpAdd")}
            </button>
          </div>
        ) : null}

        {customProviders.length === 0 ? (
          <p className="empty-copy">导入自定义 OpenAI / Gemini 兼容端点。点击右上角 + 添加。</p>
        ) : (
          <div className="custom-provider-list">
            {customProviders.map((provider) => (
              <div className="custom-provider-row" key={provider.id}>
                <div className="custom-provider-info">
                  <strong>{provider.name}</strong>
                  <small>{provider.base_url}</small>
                </div>
                <span className="custom-provider-kind">{provider.kind}</span>
                <button
                  className="row-icon-btn"
                  type="button"
                  onClick={() => startEditCustom(provider)}
                  title={t("providers.cpEdit")}
                  aria-label={t("providers.cpEdit")}
                >
                  ✎
                </button>
                <button
                  className="row-icon-btn row-icon-btn--danger"
                  type="button"
                  onClick={() => void removeCustomProvider(provider.id)}
                  title="删除"
                  aria-label="删除"
                >
                  <TrashIcon />
                </button>
              </div>
            ))}
          </div>
        )}
      </article>
    </section>
  );
}

function AccountGroup({
  group,
  isBusy,
  onDelete,
}: {
  group: AccountGroupData;
  isBusy: boolean;
  onDelete: (account: AuthFile) => void;
}) {
  const [open, setOpen] = useState(true);
  const initial = group.label.trim().charAt(0).toUpperCase() || "?";

  return (
    <div className="account-group">
      <button className="account-group-head" type="button" onClick={() => setOpen((value) => !value)}>
        <span className={open ? "group-chevron group-chevron--open" : "group-chevron"} aria-hidden="true">
          ›
        </span>
        <span className="account-logo" style={{ color: `#${group.colorHex}`, background: `#${group.colorHex}22` }} aria-hidden="true">
          {initial}
        </span>
        <span className="account-group-name">{group.label}</span>
        <span className="account-group-count">{group.accounts.length}</span>
      </button>

      {open ? (
        <div className="account-rows">
          {group.accounts.map((account) => (
            <AccountRow key={account.id} account={account} colorHex={group.colorHex} isBusy={isBusy} onDelete={() => onDelete(account)} />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function AccountRow({
  account,
  colorHex,
  isBusy,
  onDelete,
}: {
  account: AuthFile;
  colorHex: string;
  isBusy: boolean;
  onDelete: () => void;
}) {
  const t = useT();
  const label = account.email || account.account || account.label || account.name;
  const initial = label.trim().charAt(0).toUpperCase() || "?";

  return (
    <div className="account-row">
      <span className="account-logo account-logo--sm" style={{ color: `#${colorHex}`, background: `#${colorHex}22` }} aria-hidden="true">
        {initial}
      </span>
      <div className="account-row-info">
        <span className="account-row-email">{maskEmail(label)}</span>
        <span className="account-row-status">{account.disabled ? t("providers.statusDisabled") : account.status}</span>
      </div>
      <div className="account-row-actions">
        {account.active_in_ide ? (
          <span className="ide-pill ide-pill--active">{t("providers.activeInIde")}</span>
        ) : (
          <span className="ide-pill ide-pill--use">{t("providers.useInIde")}</span>
        )}
        <button className="row-icon-btn row-icon-btn--danger" type="button" onClick={onDelete} disabled={isBusy} title="删除账号" aria-label="删除账号">
          <TrashIcon />
        </button>
      </div>
    </div>
  );
}

function groupAccounts(authFiles: AuthFile[], providers: ProviderSummary[]): AccountGroupData[] {
  const groups: AccountGroupData[] = [];
  const index = new Map<string, number>();

  for (const account of authFiles) {
    let position = index.get(account.provider);
    if (position === undefined) {
      const provider = providers.find((item) => item.id === account.provider || item.id.includes(account.provider));
      position = groups.length;
      index.set(account.provider, position);
      groups.push({
        id: account.provider,
        label: provider?.display_name ?? account.provider,
        colorHex: provider?.color_hex ?? "8a8a8e",
        accounts: [],
      });
    }
    groups[position].accounts.push(account);
  }

  return groups;
}

function OAuthPanel({
  providers,
  projectId,
  oauthSession,
  isBusy,
  managementAction,
  onProjectIdChange,
  onStartOAuth,
  onPollOAuth,
}: {
  providers: ProviderSummary[];
  projectId: string;
  oauthSession: OAuthSession | null;
  isBusy: boolean;
  managementAction: string | null;
  onProjectIdChange: (value: string) => void;
  onStartOAuth: (provider: ProviderSummary) => void;
  onPollOAuth: () => void;
}) {
  return (
    <article className="panel section-panel">
      <div className="panel-header">
        <div>
          <p className="eyebrow">OAuth</p>
          <h2>授权入口</h2>
        </div>
        <span className="count-pill">{providers.length} providers</span>
      </div>

      <div className="stacked-form">
        <label>
          Project ID，可选
          <input value={projectId} onChange={(event) => onProjectIdChange(event.target.value)} placeholder="Google / Vertex project id" />
        </label>
      </div>

      <div className="provider-chip-grid">
        {providers.map((provider) => (
          <button
            className="provider-chip provider-chip--button"
            key={provider.id}
            type="button"
            onClick={() => onStartOAuth(provider)}
            disabled={isBusy || !provider.oauth_endpoint}
          >
            <span className="provider-dot" style={{ backgroundColor: `#${provider.color_hex}` }} />
            <span>
              <strong>{provider.display_name}</strong>
              <small>{provider.oauth_endpoint}</small>
            </span>
          </button>
        ))}
      </div>

      {oauthSession ? (
        <div className="oauth-session-card">
          <div className="oauth-session-head">
            <strong>{oauthSession.providerName}</strong>
            <span className={`oauth-status oauth-status--${oauthSession.status}`}>{oauthSession.status}</span>
          </div>
          {oauthSession.error ? <p className="inline-error">{oauthSession.error}</p> : null}
          {oauthSession.state ? <code className="oauth-token">{oauthSession.state}</code> : null}
          <div className="oauth-session-actions">
            {oauthSession.url ? (
              <a className="secondary-action" href={oauthSession.url} target="_blank" rel="noreferrer">
                打开授权链接
              </a>
            ) : null}
            <button className="primary-action" type="button" onClick={onPollOAuth} disabled={isBusy || !oauthSession.state}>
              {managementAction === "poll_management_oauth" ? "轮询中..." : "轮询授权状态"}
            </button>
          </div>
        </div>
      ) : null}
    </article>
  );
}

function VertexImportPanel({
  provider,
  value,
  error,
  isBusy,
  onChange,
  onImport,
}: {
  provider: ProviderSummary | undefined;
  value: string;
  error: string | null;
  isBusy: boolean;
  onChange: (value: string) => void;
  onImport: () => void;
}) {
  return (
    <article className="panel section-panel">
      <div className="panel-header">
        <div>
          <p className="eyebrow">Vertex</p>
          <h2>Service Account 导入</h2>
        </div>
        <span className="count-pill">{provider?.display_name ?? "Vertex AI"}</span>
      </div>

      <div className="stacked-form">
        <label>
          Service account JSON
          <textarea
            value={value}
            onChange={(event) => onChange(event.target.value)}
            placeholder='{"type":"service_account",...}'
            rows={9}
          />
        </label>
        {error ? <p className="inline-error">{error}</p> : null}
        <button className="secondary-action" type="button" onClick={onImport} disabled={isBusy || value.trim().length === 0}>
          导入 Vertex JSON
        </button>
      </div>
    </article>
  );
}
