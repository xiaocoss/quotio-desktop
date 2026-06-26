import { useState, useEffect, useRef, useMemo, memo, type ChangeEvent } from "react";
import type { AccountAuthHealth, AppState, AuthFile, OAuthStatusResponse, OAuthUrlResponse, ProviderSummary } from "../../types";
import { maskEmail, matchAuthFile } from "../../lib/format";
import { EyeIcon, EyeOffIcon, PlusIcon, RefreshIcon, TrashIcon } from "../icons";
import { useT } from "../../i18n";
import { invoke } from "../../lib/tauri";
import { Select } from "../Select";
import { AddAccountModal } from "../AddAccountModal";

type ProviderKey = { id: string; label: string; api_key: string; enabled: boolean; weight: number };
type CustomProvider = { id: string; name: string; base_url: string; api_key: string; kind: string; prefix?: string; keys: ProviderKey[]; default_model?: string; models?: string[]; proxy_mode?: string };

type ProvidersScreenProps = {
  appState: AppState;
  isManagementBusy: boolean;
  managementAction: string | null;
  onRefreshManagement: () => void;
  onRefreshQuotas: () => void;
  onRunManagementStateAction: (command: string, args?: Record<string, unknown>) => void;
  onStartOAuth: (endpoint: string, projectId: string | null, isWebui?: boolean) => Promise<OAuthUrlResponse | null>;
  onPollOAuth: (token: string) => Promise<OAuthStatusResponse | null>;
};

type AccountGroupData = {
  id: string;
  label: string;
  colorHex: string;
  accounts: AuthFile[];
};

// 公共「⋯」菜单:只用来给「还没连接」(没卡片)的服务商加第一个账号。
// 已连接的有自己的卡片(+ 添加 / 导出账号 / 删除所有账号),文件导入也在添加弹窗里——
// 所以这里不再放 导入 / 导出所有 / 清空所有。没有可加的服务商时整个菜单不显示。
function GlobalActionsMenu({
  oauthProviders, onSelectProvider,
}: {
  oauthProviders: ProviderSummary[];
  onSelectProvider: (provider: ProviderSummary) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function close(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  if (oauthProviders.length === 0) return null;

  return (
    <div className="pv-card-menu-anchor" ref={ref}>
      <button className="pv-card-more pv-global-more" type="button" onClick={() => setOpen((v) => !v)} aria-label="Add unconnected provider">
        <svg viewBox="0 0 16 16" width="15" height="15" fill="currentColor"><circle cx="3" cy="8" r="1.3"/><circle cx="8" cy="8" r="1.3"/><circle cx="13" cy="8" r="1.3"/></svg>
      </button>
      {open ? (
        <div className="pv-card-dropdown pv-global-dropdown">
          {oauthProviders.map((p) => (
            <button key={p.id} type="button" onClick={() => { onSelectProvider(p); setOpen(false); }}>
              Add {p.display_name} Account
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function CustomProviderCard({
  provider,
  boundKeys,
  onEdit,
  onDelete,
  onAddKey,
  onRemoveKey,
  onToggleKey,
}: {
  provider: CustomProvider;
  boundKeys: { masked: string }[];
  onEdit: () => void;
  onDelete: () => void;
  onAddKey: (label: string, apiKey: string) => void;
  onRemoveKey: (keyId: string) => void;
  onToggleKey: (keyId: string) => void;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [addingKey, setAddingKey] = useState(false);
  const [newKeyLabel, setNewKeyLabel] = useState("");
  const [newKeyValue, setNewKeyValue] = useState("");
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    function close(e: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false);
        setConfirmDelete(false);
      }
    }
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menuOpen]);

  const kindColor = provider.kind === "gemini" ? "#4285F4" : provider.kind === "claude" ? "#D97757" : "#10a37f";
  const enabledCount = provider.keys.filter((k) => k.enabled).length;

  return (
    <div className="pv-card cp-card">
      <div className="pv-card-header">
        <span className="cp-icon" style={{ background: kindColor + "22", color: kindColor }}>
          {provider.name.charAt(0).toUpperCase()}
        </span>
        <span className="pv-card-title">{provider.name}</span>
        <span className="cp-kind-badge">{provider.kind}</span>
        {provider.proxy_mode === "direct" ? (
          <span className="cp-kind-badge" title="This provider bypasses the global proxy" style={{ color: "var(--accent, #10a37f)", borderColor: "var(--accent, #10a37f)" }}>Direct</span>
        ) : null}
        <button className="pv-card-more" type="button" onClick={() => setAddingKey(true)} aria-label="Add key" title="Add key">
          <PlusIcon />
        </button>
        <div className="pv-card-menu-anchor" ref={menuRef}>
          <button className="pv-card-more" type="button" onClick={() => setMenuOpen((v) => !v)} aria-label="More">
            <svg viewBox="0 0 16 16" width="14" height="14" fill="currentColor"><circle cx="8" cy="3" r="1.3"/><circle cx="8" cy="8" r="1.3"/><circle cx="8" cy="13" r="1.3"/></svg>
          </button>
          {menuOpen ? (
            <div className="pv-card-dropdown">
              <button type="button" onClick={() => { onEdit(); setMenuOpen(false); }}>Edit Provider</button>
              <button
                type="button"
                className={confirmDelete ? "pv-dropdown-danger" : ""}
                onClick={() => {
                  if (confirmDelete) { onDelete(); setMenuOpen(false); }
                  else { setConfirmDelete(true); window.setTimeout(() => setConfirmDelete(false), 3000); }
                }}
              >
                {confirmDelete ? "Confirm delete?" : "Delete Provider"}
              </button>
            </div>
          ) : null}
        </div>
      </div>

      <div className="cp-card-meta">
        <span className="cp-url-text" title={provider.base_url}>{provider.base_url}</span>
        {provider.models && provider.models.length > 0 ? (
          <>
            {provider.models.slice(0, 4).map((m) => <code key={m} className="cp-model-pill">{m}</code>)}
            {provider.models.length > 4 ? <code className="cp-model-pill">+{provider.models.length - 4}</code> : null}
          </>
        ) : (
          <code className="cp-model-pill" style={{ color: "var(--danger, #d9534f)", borderColor: "var(--danger, #d9534f)" }} title="No models configured, proxy cannot route to this provider (click edit to add models)">
            ⚠ Unconfigured Models
          </code>
        )}
      </div>

      <div className="cp-key-pool">
        <div className="cp-key-pool-header">
          <span>Key Pool</span>
          <span className="pv-count-badge">{enabledCount}/{provider.keys.length} Enabled</span>
        </div>
        {provider.keys.length === 0 ? (
          <p className="cp-key-empty">No keys. Click + to add.</p>
        ) : (
          <div className="cp-key-list">
            {provider.keys.map((k) => (
              <div className={`cp-key-row${k.enabled ? "" : " cp-key-disabled"}`} key={k.id}>
                <button className="cp-key-toggle" type="button" onClick={() => onToggleKey(k.id)} title={k.enabled ? "Disable" : "Enable"}>
                  <span className={`cp-key-dot${k.enabled ? " cp-key-dot--on" : ""}`} />
                </button>
                <span className="cp-key-label">{k.label || "Unnamed"}</span>
                <span className="cp-key-masked">{maskKey(k.api_key)}</span>
                <button className="row-icon-btn row-icon-btn--danger cp-key-del" type="button" onClick={() => onRemoveKey(k.id)} title="Delete key" aria-label="Delete key">
                  <TrashIcon />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {boundKeys.length > 0 ? (
        <div className="cp-bound-keys">
          <span className="cp-bound-keys-label">Bound Client Keys</span>
          {boundKeys.map((bk, i) => (
            <code className="cp-bound-key-tag" key={i}>{bk.masked}</code>
          ))}
        </div>
      ) : null}

      {addingKey ? (
        <div className="cp-add-key-form">
          <input placeholder="Label (optional)" value={newKeyLabel} onChange={(e) => setNewKeyLabel(e.target.value)} />
          <input placeholder="API Key" type="password" value={newKeyValue} onChange={(e) => setNewKeyValue(e.target.value)} />
          <div className="cp-add-key-actions">
            <button type="button" className="primary-action" disabled={!newKeyValue.trim()} onClick={() => { onAddKey(newKeyLabel, newKeyValue); setNewKeyLabel(""); setNewKeyValue(""); setAddingKey(false); }}>
              Add
            </button>
            <button type="button" onClick={() => { setAddingKey(false); setNewKeyLabel(""); setNewKeyValue(""); }}>Cancel</button>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function maskKey(key: string): string {
  if (key.length <= 8) return "****";
  return key.slice(0, 4) + "****" + key.slice(-4);
}

export function ProvidersScreen({
  appState,
  isManagementBusy,
  managementAction: _managementAction,
  onRefreshManagement,
  onRefreshQuotas,
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
    void invoke<AuthFile[]>("list_local_accounts").then(setLocalAccounts).catch((err) => console.warn("[ProvidersScreen] list_local_accounts:", err));
  }, [appState.management.auth_files]);
  const authFiles = useMemo(() => {
    if (proxyAuthFiles.length === 0) return localAccounts;
    if (localAccounts.length === 0) return proxyAuthFiles;
    // Match case-INSENSITIVELY: the proxy's /auth-files lowercases filenames,
    // but the local auth dir keeps the original (mixed) case. A case-sensitive
    // check re-adds every mixed-case local file (e.g. codex-MartilloOlivia…) as
    // a phantom duplicate while all-lowercase ones dedupe fine.
    const proxyNames = new Set(proxyAuthFiles.map((f) => f.name.toLowerCase()));
    const extra = localAccounts.filter((f) => !proxyNames.has(f.name.toLowerCase()));
    return extra.length > 0 ? [...proxyAuthFiles, ...extra] : proxyAuthFiles;
  }, [proxyAuthFiles, localAccounts]);
  const groups = useMemo(() => groupAccounts(authFiles, appState.providers), [authFiles, appState.providers]);
  // Accounts whose quota fetch hit an unrecoverable 401 are flagged "auth_failed"
  // by the backend. Unlike the proxy's recent-request health (which resets on
  // restart), this re-detects every refresh, so it's a durable "re-auth" signal.
  const authFailedNames = useMemo(() => {
    const names = new Set<string>();
    for (const quota of appState.quotas) {
      if (quota.status_message === "auth_failed") {
        const file = matchAuthFile(quota, authFiles);
        if (file) names.add(file.name);
      }
    }
    return names;
  }, [appState.quotas, authFiles]);
  // Per-account health from the persisted usage store, classified by REAL HTTP
  // status code (401/403 = auth, 429 = rate-limit, 5xx = transient). Lets the
  // badge tell a genuine auth failure apart from throttling, so "re-authorize"
  // only fires on actual auth problems — not on a blanket recent-failure count.
  const [authHealth, setAuthHealth] = useState<Map<string, AccountAuthHealth>>(new Map());
  useEffect(() => {
    // `invoke` routes to the dev mock in a plain browser, so this also populates
    // during UI iteration; no __TAURI_INTERNALS__ gate needed.
    void invoke<AccountAuthHealth[]>("query_account_auth_health")
      .then((list) => {
        const map = new Map<string, AccountAuthHealth>();
        for (const item of list) map.set(item.account.trim().toLowerCase(), item);
        setAuthHealth(map);
      })
      .catch((err) => console.warn("[ProvidersScreen] query_account_auth_health:", err));
  }, [appState.management.auth_files, appState.quotas]);
  const oauthProviders = appState.providers.filter((provider) => provider.native_oauth || provider.oauth_endpoint || provider.supports_manual_auth);

  const [addAccountProvider, setAddAccountProvider] = useState<ProviderSummary | null>(null);
  const [projectId] = useState("");
  const [showAddCustom, setShowAddCustom] = useState(false);
  const [editingCustomId, setEditingCustomId] = useState<string | null>(null);
  const [customForm, setCustomForm] = useState({ name: "", base_url: "", api_key: "", kind: "openai", prefix: "", models: "", proxy_mode: "inherit" });
  const [formKeys, setFormKeys] = useState<{ label: string; api_key: string }[]>([{ label: "", api_key: "" }]);
  const [customFormError, setCustomFormError] = useState<string | null>(null);
  const [customProviders, setCustomProviders] = useState<CustomProvider[]>([]);
  useEffect(() => {
    void invoke<CustomProvider[]>("list_custom_providers").then(setCustomProviders).catch((err) => console.warn("[ProvidersScreen] list_custom_providers:", err));
  }, []);

  function reauthAccount(account: AuthFile) {
    const provider = appState.providers.find(
      (item) => item.id === account.provider || item.id.includes(account.provider) || account.provider.includes(item.id),
    );
    if (provider) setAddAccountProvider(provider);
  }

  function resetCustomForm() {
    setCustomForm({ name: "", base_url: "", api_key: "", kind: "openai", prefix: "", models: "", proxy_mode: "inherit" });
    setFormKeys([{ label: "", api_key: "" }]);
    setEditingCustomId(null);
    setShowAddCustom(false);
    setCustomFormError(null);
  }

  function startEditCustom(provider: CustomProvider) {
    setCustomForm({
      name: provider.name,
      base_url: provider.base_url,
      api_key: provider.api_key,
      kind: provider.kind || "openai",
      prefix: provider.prefix ?? "",
      models: (provider.models ?? []).join("\n"),
      proxy_mode: provider.proxy_mode === "direct" ? "direct" : "inherit",
    });
    setEditingCustomId(provider.id);
    setShowAddCustom(true);
  }

  async function submitCustomProvider() {
    if (!customForm.name.trim() || !customForm.base_url.trim()) return;
    setCustomFormError(null);
    // Tauri individual-arg commands expect camelCase keys (baseUrl/apiKey) — the
    // snake_case fields on customForm don't map, so spreading it silently fails.
    const base = { name: customForm.name, baseUrl: customForm.base_url, kind: customForm.kind, prefix: customForm.prefix, models: customForm.models, proxyMode: customForm.proxy_mode };
    try {
      if (editingCustomId) {
        setCustomProviders(await invoke<CustomProvider[]>("update_custom_provider", { id: editingCustomId, ...base, apiKey: customForm.api_key }));
      } else {
        const validKeys = formKeys.filter((k) => k.api_key.trim());
        const firstKey = validKeys[0]?.api_key ?? "";
        let result = await invoke<CustomProvider[]>("add_custom_provider", { ...base, apiKey: firstKey });
        if (validKeys.length > 1) {
          const newProvider = result.find((p) => p.name === customForm.name.trim());
          if (newProvider) {
            for (const k of validKeys.slice(1)) {
              result = await invoke<CustomProvider[]>("add_provider_key", { providerId: newProvider.id, label: k.label, apiKey: k.api_key });
            }
          }
        }
        setCustomProviders(result);
      }
      resetCustomForm();
    } catch (error) {
      setCustomFormError(typeof error === "string" ? error : "Add failed, please check Name/Base URL/Key and try again.");
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

  async function addKeyToProvider(providerId: string, label: string, apiKey: string) {
    try {
      setCustomProviders(await invoke<CustomProvider[]>("add_provider_key", { providerId, label, apiKey }));
    } catch { /* ignore */ }
  }

  async function removeKeyFromProvider(providerId: string, keyId: string) {
    try {
      setCustomProviders(await invoke<CustomProvider[]>("remove_provider_key", { providerId, keyId }));
    } catch { /* ignore */ }
  }

  async function toggleKeyInProvider(providerId: string, keyId: string) {
    try {
      setCustomProviders(await invoke<CustomProvider[]>("toggle_provider_key", { providerId, keyId }));
    } catch { /* ignore */ }
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

  // 按服务商导出:只把该服务商的账号文件打包成 zip(把账号名传给后端做过滤)。
  // 反馈靠系统保存对话框 + 导出后在文件管理器里高亮该 zip,不再用应用内状态。
  async function onExportProvider(group: AccountGroupData) {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const accounts = group.accounts;
    if (accounts.length === 0) return;
    const names = accounts.map((account) => account.name);
    const emails = accounts.map((account) => account.email).filter((email): email is string => Boolean(email));
    const now = new Date();
    const pad = (value: number) => String(value).padStart(2, "0");
    const stamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
    const base =
      emails.length === 1
        ? emails[0]
        : emails.length > 1
          ? `${emails[0]}+${emails.length - 1}`
          : `${accounts.length}accounts`;
    const safe = (value: string) => value.replace(/[<>:"\\|?*\x00-\x1f/]/g, "-");
    const defaultName = `quotio_${safe(group.id)}_${safe(base)}_${stamp}.zip`;
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const target = await save({
        defaultPath: defaultName,
        filters: [{ name: "Zip", extensions: ["zip"] }],
      });
      if (!target) return; // user cancelled the dialog
      const path = await invoke<string>("export_auth_files", { path: target, names });
      try {
        const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
        await revealItemInDir(path);
      } catch {
        /* revealing the folder is best-effort */
      }
    } catch {
      /* save 取消 / 导出失败 —— best-effort,不弹 toast */
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
            onClick={onRefreshQuotas}
            disabled={isManagementBusy}
            title="Refresh accounts"
            aria-label="Refresh accounts"
          >
            <RefreshIcon />
          </button>
        </div>
      </header>

      {addAccountProvider ? (
        <AddAccountModal
          provider={addAccountProvider}
          projectId={projectId}
          onClose={() => setAddAccountProvider(null)}
          onStartOAuth={onStartOAuth}
          onPollOAuth={onPollOAuth}
          onRefreshQuotas={() => { onRefreshQuotas(); onRefreshManagement(); }}
          onImportFile={onImportFiles}
        />
      ) : null}

      {/* ── Connected providers: card grid ── */}
      <div className="pv-section-header">
        <h2 className="pv-section-title">{t("providers.connected", "Connected Providers")}</h2>
        <span className="pv-section-actions">
          <span className="pv-count-badge">{t("providers.count", `Total ${groups.length} providers`)}</span>
          <GlobalActionsMenu
            oauthProviders={oauthProviders.filter(
              // 只列「还没连接」(没卡片)的服务商——加首个账号的唯一入口;已连接的卡片上有 +。
              (p) => !groups.some((g) => g.id === p.id || p.id.includes(g.id) || g.id.includes(p.id)),
            )}
            onSelectProvider={setAddAccountProvider}
          />
        </span>
      </div>

      {groups.length === 0 ? (
        <p className="empty-copy" style={{ padding: "24px 0" }}>{t("providers.empty", "No accounts yet. Click + on a card to authorize, or import via the ⋯ menu.")}</p>
      ) : (
        <div className="pv-card-grid">
          {groups.map((group) => (
            <ProviderCard
              key={group.id}
              group={group}
              isBusy={isManagementBusy}
              authFailedNames={authFailedNames}
              authHealth={authHealth}
              onDelete={(account) => onRunManagementStateAction("delete_management_auth_file", { name: account.name })}
              onReauth={reauthAccount}
              onAddAccount={() => {
                const provider = appState.providers.find((p) => p.id === group.id || p.id.includes(group.id) || group.id.includes(p.id));
                if (provider) setAddAccountProvider(provider);
              }}
              onExport={() => void onExportProvider(group)}
              onDeleteAll={() => {
                for (const account of group.accounts) {
                  onRunManagementStateAction("delete_management_auth_file", { name: account.name });
                }
              }}
              onToggleDisableAll={() => {
                const allDisabled = group.accounts.every((a) => a.disabled);
                for (const account of group.accounts) {
                  if (allDisabled && account.disabled) {
                    onRunManagementStateAction("set_management_auth_file_disabled", { name: account.name, disabled: false });
                  } else if (!allDisabled && !account.disabled) {
                    onRunManagementStateAction("set_management_auth_file_disabled", { name: account.name, disabled: true });
                  }
                }
              }}
            />
          ))}
        </div>
      )}

      {/* ── Custom API management: table ── */}
      <div className="pv-section-header" style={{ marginTop: 28 }}>
        <h2 className="pv-section-title">{t("providers.customApi", "Custom API Management")}</h2>
        <button className="pv-add-btn" type="button" onClick={() => (showAddCustom ? resetCustomForm() : setShowAddCustom(true))}>
          <PlusIcon /> {t("providers.addApi", "Add API")}
        </button>
      </div>

      {showAddCustom ? (
        <article className="panel" style={{ marginBottom: 12 }}>
          <div className="stacked-form custom-provider-form">
            <label>
              {t("providers.cpName")}
              <input value={customForm.name} onChange={(e) => setCustomForm({ ...customForm, name: e.target.value })} placeholder="My Provider" />
            </label>
            <label>
              {t("providers.cpBaseUrl")}
              <input value={customForm.base_url} onChange={(e) => setCustomForm({ ...customForm, base_url: e.target.value })} placeholder="https://api.example.com/v1" />
            </label>
            {editingCustomId ? (
              <p className="cp-form-hint">Keys are managed on the card. Keys are hidden in edit mode.</p>
            ) : (
              <div className="cp-form-keys">
                <div className="cp-form-keys-header">
                  <span>API Key</span>
                  <button type="button" className="cp-form-keys-add" onClick={() => setFormKeys([...formKeys, { label: "", api_key: "" }])}>
                    <PlusIcon /> Add Key
                  </button>
                </div>
                {formKeys.map((fk, i) => (
                  <div className="cp-form-key-row" key={i}>
                    <input
                      placeholder="Label (optional)"
                      value={fk.label}
                      onChange={(e) => { const next = [...formKeys]; next[i] = { ...fk, label: e.target.value }; setFormKeys(next); }}
                    />
                    <input
                      type="password"
                      placeholder="sk-..."
                      value={fk.api_key}
                      onChange={(e) => { const next = [...formKeys]; next[i] = { ...fk, api_key: e.target.value }; setFormKeys(next); }}
                    />
                    {formKeys.length > 1 ? (
                      <button type="button" className="row-icon-btn row-icon-btn--danger" onClick={() => setFormKeys(formKeys.filter((_, j) => j !== i))} title="Remove" aria-label="Remove">
                        <TrashIcon />
                      </button>
                    ) : null}
                  </div>
                ))}
              </div>
            )}
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
              <label>
                {t("providers.cpKind")}
                <Select
                  value={customForm.kind}
                  options={[
                    { value: "openai", label: "OpenAI" },
                    { value: "gemini", label: "Gemini" },
                    { value: "claude", label: "Claude" },
                  ]}
                  onChange={(value) => setCustomForm({ ...customForm, kind: value })}
                />
              </label>
              <label>
                {t("providers.cpPrefix")}
                <input value={customForm.prefix} onChange={(e) => setCustomForm({ ...customForm, prefix: e.target.value })} placeholder="myprovider" />
              </label>
            </div>
            <label>
              Model List (one per line, required for routing)
              <textarea
                value={customForm.models}
                onChange={(e) => setCustomForm({ ...customForm, models: e.target.value })}
                placeholder={"gpt-5.5\nclaude-sonnet-4-5-20250929"}
                rows={3}
                spellCheck={false}
                style={{ fontFamily: "var(--font-mono, monospace)", resize: "vertical" }}
              />
            </label>
            <p className="cp-form-hint">
              Required: The actual models provided by this API. Otherwise the proxy cannot route to it (will return quota/routing error). Separate with comma, space or newline.
            </p>
            <label>
              Connection Mode
              <Select
                value={customForm.proxy_mode}
                options={[
                  { value: "inherit", label: "Use Proxy (Follow global setting)" },
                  { value: "direct", label: "Direct (Bypass proxy)" },
                ]}
                onChange={(value) => setCustomForm({ ...customForm, proxy_mode: value })}
              />
            </label>
            <p className="cp-form-hint">
              "Direct" allows this provider to bypass the global proxy; "Use Proxy" uses the global proxy defined in Settings.
            </p>
            {customFormError ? (
              <p className="cp-form-hint" style={{ color: "var(--danger, #d9534f)" }}>{customFormError}</p>
            ) : null}
            <button className="primary-action" type="button" onClick={() => void submitCustomProvider()} disabled={!customForm.name.trim() || !customForm.base_url.trim()}>
              {editingCustomId ? t("providers.cpSave") : t("providers.cpAdd")}
            </button>
          </div>
        </article>
      ) : null}

      {customProviders.length === 0 ? (
        <p className="empty-copy" style={{ padding: "16px 0" }}>{t("providers.noCustomApi", "No custom APIs yet. Click \"Add API\" to import OpenAI / Gemini / Claude compatible endpoints.")}</p>
      ) : (
        <div className="pv-card-grid">
          {customProviders.map((cp) => {
            const bindings = appState.api_key_bindings ?? [];
            const apiKeys = appState.api_keys;
            const boundKeys = bindings
              .filter((b) => b.provider_id === cp.id)
              .map((b) => {
                const entry = apiKeys.find((k) => k.value === b.api_key);
                return { masked: entry?.masked_value ?? maskKey(b.api_key) };
              });
            return (
              <CustomProviderCard
                key={cp.id}
                provider={cp}
                boundKeys={boundKeys}
                onEdit={() => startEditCustom(cp)}
                onDelete={() => void removeCustomProvider(cp.id)}
                onAddKey={(label, apiKey) => void addKeyToProvider(cp.id, label, apiKey)}
                onRemoveKey={(keyId) => void removeKeyFromProvider(cp.id, keyId)}
                onToggleKey={(keyId) => void toggleKeyInProvider(cp.id, keyId)}
              />
            );
          })}
        </div>
      )}
    </section>
  );
}

// Per-account state for the Providers badge, derived from the proxy's flags +
// the recent-request health (no live `status` string guessing). `needsReauth`
// surfaces the re-auth button and floats the account to the top of its group.
type AccountStateInfo = { tone: "good" | "warn" | "bad" | "muted"; key: string; fallback: string; needsReauth: boolean };

// Look up an account's usage-store health by its email/account label (the usage
// event `source`); the filename (`name`) is never the source, so it's skipped.
function healthFor(
  account: AuthFile,
  authHealth: Map<string, AccountAuthHealth>,
): AccountAuthHealth | undefined {
  for (const candidate of [account.email, account.account, account.label]) {
    if (candidate) {
      const found = authHealth.get(candidate.trim().toLowerCase());
      if (found) return found;
    }
  }
  return undefined;
}

function accountState(
  account: AuthFile,
  authFailed: boolean,
  health: AccountAuthHealth | undefined,
): AccountStateInfo {
  // Re-auth is suggested ONLY on genuine auth failures:
  //   1. the quota probe's unrecoverable 401 (durable, survives restarts), or
  //   2. recent requests dominated by real 401/403 with no success (from the
  //      persisted status codes — how cpa-manager judges a "real 401").
  // A blanket recent-failure count or the proxy's vague "error" status no longer
  // triggers re-auth, since 500/429 failures are rate-limit/transient, not auth.
  if (authFailed) return { tone: "bad", key: "providers.stateNeedsReauth", fallback: "Needs Reauth", needsReauth: true };
  if (health?.recommend_reauth) return { tone: "bad", key: "providers.stateNeedsReauth", fallback: "Needs Reauth", needsReauth: true };
  if (account.disabled && account.quotio_scheduler_standby)
    return { tone: "muted", key: "providers.stateStandby", fallback: "Standby", needsReauth: false };
  if (account.disabled && account.quotio_bound_login_only)
    return { tone: "muted", key: "providers.stateBoundLogin", fallback: "Bound Login", needsReauth: false };
  if (account.disabled) return { tone: "muted", key: "providers.statusDisabled", fallback: "Disabled", needsReauth: false };
  if (account.unavailable) return { tone: "bad", key: "providers.stateUnavailable", fallback: "Unavailable", needsReauth: true };
  const status = (account.status ?? "").trim().toLowerCase();
  if (status === "cooling") return { tone: "warn", key: "providers.stateCooling", fallback: "Cooling", needsReauth: false };

  // Classify by REAL status codes when usage history exists (preferred).
  if (health && health.recent_total > 0) {
    const failures = health.auth_failures + health.rate_limited + health.server_errors;
    if (failures === 0) return { tone: "good", key: "providers.stateActive", fallback: "Active", needsReauth: false };
    if (health.rate_limited > 0 && health.rate_limited >= health.server_errors && health.rate_limited >= health.auth_failures)
      return { tone: "warn", key: "providers.stateRateLimited", fallback: "Rate Limited", needsReauth: false };
    // 5xx dominate the failures → upstream proxy / server congestion (the
    // "wsarecv: forcibly closed" resets), NOT a problem with this account. Flag it
    // as upstream-unstable (warn) rather than the alarming "失败偏多 / 异常".
    if (health.server_errors > 0 && health.server_errors >= health.auth_failures && health.server_errors >= health.rate_limited)
      return { tone: "warn", key: "providers.stateUpstream", fallback: "Upstream Unstable (5xx)", needsReauth: false };
    if (failures >= health.successes)
      return { tone: "bad", key: "providers.stateFailing", fallback: "Failing (High Error Rate)", needsReauth: false };
    return { tone: "warn", key: "providers.stateDegraded", fallback: "Degraded", needsReauth: false };
  }

  // Fallback to the proxy's recent-request buckets when there's no usage history
  // yet (e.g. right after a fresh start) — still without claiming an auth issue.
  const recent = account.recent_requests ?? [];
  const ok = recent.reduce((sum, bucket) => sum + bucket.success, 0);
  const fail = recent.reduce((sum, bucket) => sum + bucket.failed, 0);
  if (fail >= 3 && fail >= ok) return { tone: "bad", key: "providers.stateFailing", fallback: "Failing (High Error Rate)", needsReauth: false };
  if (fail > 0) return { tone: "warn", key: "providers.stateDegraded", fallback: "Degraded", needsReauth: false };
  if (status === "error") return { tone: "bad", key: "providers.stateAnomaly", fallback: "Anomaly", needsReauth: false };
  return { tone: "good", key: "providers.stateActive", fallback: "Active", needsReauth: false };
}

function ProviderCard({
  group,
  isBusy,
  authFailedNames,
  authHealth,
  onDelete,
  onReauth,
  onAddAccount,
  onExport,
  onDeleteAll,
  onToggleDisableAll,
}: {
  group: AccountGroupData;
  isBusy: boolean;
  authFailedNames: Set<string>;
  authHealth: Map<string, AccountAuthHealth>;
  onDelete: (account: AuthFile) => void;
  onReauth: (account: AuthFile) => void;
  onAddAccount: () => void;
  onExport: () => void;
  onDeleteAll: () => void;
  onToggleDisableAll: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const initial = group.label.trim().charAt(0).toUpperCase() || "?";
  const accounts = group.accounts
    .map((account) => ({
      account,
      needsReauth: accountState(account, authFailedNames.has(account.name), healthFor(account, authHealth)).needsReauth,
    }))
    .sort((a, b) => Number(b.needsReauth) - Number(a.needsReauth))
    .map((entry) => entry.account);

  const goodCount = accounts.filter((a) => {
    const s = accountState(a, authFailedNames.has(a.name), healthFor(a, authHealth));
    return s.tone === "good";
  }).length;
  const badCount = accounts.filter((a) => {
    const s = accountState(a, authFailedNames.has(a.name), healthFor(a, authHealth));
    return s.tone === "bad" || s.needsReauth;
  }).length;
  const allDisabled = accounts.length > 0 && accounts.every((a) => a.disabled);

  const cardStatus = badCount > 0 ? "warn" : accounts.length === 0 ? "muted" : "good";
  const statusLabel = badCount > 0 ? `${badCount} Error(s)` : allDisabled ? "Disabled" : goodCount === accounts.length ? "Healthy" : "Idle";

  const PREVIEW_COUNT = 3;
  const previewAccounts = expanded ? accounts : accounts.slice(0, PREVIEW_COUNT);

  useEffect(() => {
    if (!menuOpen) return;
    function close(e: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenuOpen(false);
    }
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menuOpen]);

  return (
    <div className="pv-card">
      <div className="pv-card-head">
        <span className="pv-card-logo" style={{ color: `#${group.colorHex}`, background: `#${group.colorHex}18` }}>
          {initial}
        </span>
        <div className="pv-card-title-area">
          <strong className="pv-card-name">{group.label}</strong>
          <span className={`pv-card-status pv-card-status--${cardStatus}`}>{statusLabel}</span>
        </div>
        <button className="pv-card-add" type="button" onClick={onAddAccount} disabled={isBusy} title="Add Account" aria-label="Add Account">
          <PlusIcon />
        </button>
        <div className="pv-card-menu-anchor" ref={menuRef}>
          <button className="pv-card-more" type="button" onClick={() => setMenuOpen((v) => !v)} aria-label="More Actions">
            <svg viewBox="0 0 16 16" width="14" height="14" fill="currentColor"><circle cx="8" cy="3" r="1.3"/><circle cx="8" cy="8" r="1.3"/><circle cx="8" cy="13" r="1.3"/></svg>
          </button>
          {menuOpen ? (
            <div className="pv-card-dropdown">
              <button type="button" onClick={() => { onAddAccount(); setMenuOpen(false); }}>
                <PlusIcon /> Add Account
              </button>
              {accounts.length > 0 ? (
                <button type="button" onClick={() => { onToggleDisableAll(); setMenuOpen(false); }}>
                  {allDisabled ? "✦ Enable All" : "⏸ Disable All"}
                </button>
              ) : null}
              {accounts.length > 0 ? (
                <button type="button" onClick={() => { onExport(); setMenuOpen(false); }}>
                  ⬇ Export Accounts
                </button>
              ) : null}
              {accounts.length > 0 ? (
                <button
                  type="button"
                  className={confirmDelete ? "pv-dropdown-danger" : ""}
                  onClick={() => {
                    if (confirmDelete) { onDeleteAll(); setMenuOpen(false); setConfirmDelete(false); }
                    else { setConfirmDelete(true); window.setTimeout(() => setConfirmDelete(false), 3000); }
                  }}
                >
                  <TrashIcon /> {confirmDelete ? `Confirm delete ${accounts.length}?` : "Delete All Accounts"}
                </button>
              ) : null}
            </div>
          ) : null}
        </div>
      </div>

      <span className="pv-card-meta">{group.accounts.length} {group.accounts.length === 1 ? "account" : "accounts"}</span>

      <div className="pv-card-accounts">
        {previewAccounts.map((account) => (
          <AccountRow
            key={account.id}
            account={account}
            colorHex={group.colorHex}
            isBusy={isBusy}
            authFailed={authFailedNames.has(account.name)}
            health={healthFor(account, authHealth)}
            onDelete={() => onDelete(account)}
            onReauth={() => onReauth(account)}
          />
        ))}
      </div>

      {accounts.length > PREVIEW_COUNT ? (
        <button className="pv-card-toggle" type="button" onClick={() => setExpanded((v) => !v)}>
          {expanded ? "Collapse" : `View all ${accounts.length}`}{" "}
            <svg viewBox="0 0 12 12" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" style={{ verticalAlign: "middle", transition: "transform 0.2s", transform: expanded ? "rotate(180deg)" : "rotate(0)" }}>
              <path d="M2.5 4.5 6 8l3.5-3.5" />
            </svg>
        </button>
      ) : null}
    </div>
  );
}

type AccountRowProps = {
  account: AuthFile;
  colorHex: string;
  isBusy: boolean;
  authFailed: boolean;
  health: AccountAuthHealth | undefined;
  onDelete: () => void;
  onReauth: () => void;
};

// Signature of the bits of account health that affect the rendered badge.
function healthSignature(health: AccountAuthHealth | undefined): string {
  if (!health) return "none";
  return `${health.recommend_reauth ? 1 : 0}|${health.auth_failures}|${health.rate_limited}|${health.server_errors}|${health.successes}`;
}

// Sum of the recent success/failed buckets — the only part of `recent_requests`
// that affects the rendered status badge.
function recentRequestsSignature(recent: AuthFile["recent_requests"]): string {
  if (!recent || recent.length === 0) return "0";
  let ok = 0;
  let fail = 0;
  for (const bucket of recent) {
    ok += bucket.success;
    fail += bucket.failed;
  }
  return `${ok}/${fail}`;
}

// Skip re-rendering a row when its rendered data is unchanged, even though the
// account object + handler closures are new on every poll tick. The function
// props (onDelete/onReauth) are intentionally ignored — they only ever act on
// the account's stable name/provider.
function areAccountRowPropsEqual(a: AccountRowProps, b: AccountRowProps): boolean {
  if (a.colorHex !== b.colorHex || a.isBusy !== b.isBusy || a.authFailed !== b.authFailed) {
    return false;
  }
  if (healthSignature(a.health) !== healthSignature(b.health)) {
    return false;
  }
  const x = a.account;
  const y = b.account;
  return (
    x.name === y.name &&
    x.email === y.email &&
    x.account === y.account &&
    x.label === y.label &&
    x.disabled === y.disabled &&
    x.unavailable === y.unavailable &&
    x.status === y.status &&
    recentRequestsSignature(x.recent_requests) === recentRequestsSignature(y.recent_requests)
  );
}

const AccountRow = memo(function AccountRow({
  account,
  colorHex,
  isBusy,
  authFailed,
  health,
  onDelete,
  onReauth,
}: AccountRowProps) {
  const t = useT();
  const label = account.email || account.account || account.label || account.name;
  const initial = label.trim().charAt(0).toUpperCase() || "?";
  const state = accountState(account, authFailed, health);
  const [revealed, setRevealed] = useState(false);

  return (
    <div className="account-row">
      <span className="account-logo account-logo--sm" style={{ color: `#${colorHex}`, background: `#${colorHex}22` }} aria-hidden="true">
        {initial}
      </span>
      <div className="account-row-info">
        <span className={`account-row-email${revealed ? " account-row-email--revealed" : ""}`} title={label}>
          {revealed ? label : maskEmail(label)}
        </span>
        <span className={`account-row-status account-row-status--${state.tone}`}>{t(state.key, state.fallback)}</span>
      </div>
      <div className="account-row-actions">
        {state.needsReauth ? (
          <button className="account-reauth-btn" type="button" onClick={onReauth} disabled={isBusy}>
            {t("providers.reauth", "重新授权")}
          </button>
        ) : null}
        <button
          className="row-icon-btn"
          type="button"
          onClick={() => setRevealed((v) => !v)}
          title={revealed ? "Hide email" : "Show full email"}
          aria-label={revealed ? "Hide email" : "Show full email"}
        >
          {revealed ? <EyeOffIcon /> : <EyeIcon />}
        </button>
        <button className="row-icon-btn row-icon-btn--danger" type="button" onClick={onDelete} disabled={isBusy} title="Delete Account" aria-label="Delete Account">
          <TrashIcon />
        </button>
      </div>
    </div>
  );
}, areAccountRowPropsEqual);

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

