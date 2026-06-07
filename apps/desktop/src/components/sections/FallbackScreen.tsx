import { useMemo, useState } from "react";
import type { AppState, AvailableModel, FallbackConfigAction, FallbackEntry, ProviderSummary, VirtualModel } from "../../types";
import { Switch } from "../Switch";
import { CheckIcon, InfoIcon, MinusIcon, PlusIcon, TrashIcon } from "../icons";
import { Select } from "../Select";
import { useT } from "../../i18n";

type FallbackScreenProps = {
  appState: AppState;
  isBusy: boolean;
  fallbackAction: string | null;
  availableModels: AvailableModel[];
  onUpdateFallback: (action: FallbackConfigAction) => void;
  onDiscoverModels: () => Promise<AvailableModel[]>;
  onRefreshRouteState: () => void;
};

type EntryDraft = { providerId: string; modelId: string };

export function FallbackScreen({
  appState,
  isBusy,
  fallbackAction,
  availableModels,
  onUpdateFallback,
  onDiscoverModels,
}: FallbackScreenProps) {
  const t = useT();
  const fallback = appState.fallback;
  const runtime = appState.fallback_runtime;
  const discoveredModels = availableModels.length > 0 ? availableModels : runtime.available_models;
  const availableProviders = useMemo(
    () => appState.providers.filter((provider) => provider.role === "provider" && provider.enabled),
    [appState.providers],
  );
  // When fallback runs, the ProxyBridge listens on proxy_port + 100.
  const fallbackEndpoint = useMemo(() => {
    try {
      const url = new URL(appState.proxy.endpoint);
      const port = Number(url.port || "28317");
      url.port = String(port < 65435 ? port + 100 : port - 100);
      return url.toString().replace(/\/$/, "");
    } catch {
      return "";
    }
  }, [appState.proxy.endpoint]);

  const [showAddModel, setShowAddModel] = useState(false);
  const [newModelName, setNewModelName] = useState("");
  const [entryDrafts, setEntryDrafts] = useState<Record<string, EntryDraft>>({});

  function addModel() {
    const name = newModelName.trim();
    if (!name) return;
    onUpdateFallback({ add_virtual_model: { name } });
    setNewModelName("");
    setShowAddModel(false);
  }

  return (
    <section className="section-page fallback-page">
      <header className="page-topbar" data-tauri-drag-region>
        <h1>{t("title.fallback")}</h1>
        <button
          className={showAddModel ? "icon-button icon-button--active" : "icon-button"}
          type="button"
          onClick={() => setShowAddModel((value) => !value)}
          disabled={!fallback.is_enabled}
          title="新增虚拟模型"
          aria-label="新增虚拟模型"
        >
          <PlusIcon />
        </button>
      </header>

      <article className="panel fallback-settings">
        <div className="panel-label">
          <span className="eyebrow">{t("fallback.settings")}</span>
        </div>
        <div className="switch-row">
          <div className="switch-row-text">
            <div className="switch-row-title">
              <strong>{t("fallback.enable")}</strong>
              <span className="nav-badge">{t("common.experimental")}</span>
            </div>
            <small>开启后，虚拟模型会在额度耗尽时自动回退到其它 provider。</small>
          </div>
          <Switch
            on={fallback.is_enabled}
            disabled={isBusy}
            onChange={() => onUpdateFallback({ set_enabled: { enabled: !fallback.is_enabled } })}
            label="Enable Fallback"
          />
        </div>
        <div className="switch-row">
          <div className="switch-row-text">
            <strong>{t("fallback.routeCaching")}</strong>
            <small>成功回退后的路由会被缓存复用；关闭会清理运行态缓存。</small>
          </div>
          <Switch
            on={fallback.is_route_caching_enabled}
            disabled={isBusy || !fallback.is_enabled}
            onChange={() => onUpdateFallback({ set_route_caching: { enabled: !fallback.is_route_caching_enabled } })}
            label="Route caching"
          />
        </div>
        {fallback.is_enabled && fallbackEndpoint ? (
          <div className="fallback-endpoint">
            <span className="eyebrow">{t("fallback.endpoint")}</span>
            <code>{fallbackEndpoint}</code>
            <small>{t("fallback.endpointHint")}</small>
          </div>
        ) : null}
      </article>

      <article className="panel">
        <div className="panel-label">
          <span className="eyebrow">{t("fallback.virtualModels")}</span>
          <span className="vm-info" title="虚拟模型映射到真实 provider 模型" aria-hidden="true">
            <InfoIcon />
          </span>
        </div>

        {showAddModel ? (
          <div className="fallback-add-model">
            <input
              value={newModelName}
              onChange={(event) => setNewModelName(event.target.value)}
              placeholder="quotio-opus-4-6-thinking"
              autoFocus
            />
            <button className="secondary-action" type="button" onClick={addModel} disabled={isBusy || newModelName.trim().length === 0}>
              添加
            </button>
          </div>
        ) : null}

        {fallback.virtual_models.length === 0 ? (
          <p className="empty-copy">暂无虚拟模型。开启 Fallback 后点击右上角 + 创建第一个。</p>
        ) : (
          <div className="virtual-model-list">
            {fallback.virtual_models.map((model) => (
              <VirtualModelItem
                key={model.id}
                model={model}
                providers={availableProviders}
                availableModels={discoveredModels}
                fallbackEnabled={fallback.is_enabled}
                isBusy={isBusy}
                discovering={fallbackAction === "discover_available_models"}
                draft={entryDrafts[model.id] ?? { providerId: availableProviders[0]?.id ?? "", modelId: discoveredModels[0]?.id ?? "" }}
                onDraftChange={(draft) => setEntryDrafts((current) => ({ ...current, [model.id]: draft }))}
                onAction={onUpdateFallback}
                onDiscoverModels={onDiscoverModels}
              />
            ))}
          </div>
        )}

        <p className="empty-copy fallback-foot">虚拟模型映射到真实 provider 模型，额度耗尽时自动按优先级回退。</p>
      </article>
    </section>
  );
}

function VirtualModelItem({
  model,
  providers,
  availableModels,
  fallbackEnabled,
  isBusy,
  discovering,
  draft,
  onDraftChange,
  onAction,
  onDiscoverModels,
}: {
  model: VirtualModel;
  providers: ProviderSummary[];
  availableModels: AvailableModel[];
  fallbackEnabled: boolean;
  isBusy: boolean;
  discovering: boolean;
  draft: EntryDraft;
  onDraftChange: (draft: EntryDraft) => void;
  onAction: (action: FallbackConfigAction) => void;
  onDiscoverModels: () => Promise<AvailableModel[]>;
}) {
  const t = useT();
  const [open, setOpen] = useState(true);
  const [adding, setAdding] = useState(false);
  const entries = [...model.fallback_entries].sort((left, right) => left.priority - right.priority);

  return (
    <div className="virtual-model">
      <div className="virtual-model-head">
        <button className="virtual-model-toggle" type="button" onClick={() => setOpen((value) => !value)}>
          <span className={open ? "group-chevron group-chevron--open" : "group-chevron"} aria-hidden="true">
            ›
          </span>
          <span className="virtual-model-name">{model.name}</span>
          <small>
            {entries.length} {entries.length === 1 ? t("fallback.entry") : t("fallback.entries")}
          </small>
        </button>
        <button
          className={model.is_enabled ? "vm-status vm-status--on" : "vm-status vm-status--off"}
          type="button"
          onClick={() => onAction({ toggle_virtual_model: { id: model.id, enabled: !model.is_enabled } })}
          disabled={isBusy || !fallbackEnabled}
          title={model.is_enabled ? "已启用（点击停用）" : "已停用（点击启用）"}
          aria-label="切换启用状态"
        >
          {model.is_enabled ? <CheckIcon /> : null}
        </button>
        <button
          className="row-icon-btn row-icon-btn--danger"
          type="button"
          onClick={() => onAction({ remove_virtual_model: { id: model.id } })}
          disabled={isBusy}
          title="删除虚拟模型"
          aria-label="删除虚拟模型"
        >
          <TrashIcon />
        </button>
      </div>

      {open ? (
        <div className="virtual-model-body">
          {entries.length === 0 ? (
            <p className="empty-copy">暂无兜底项。</p>
          ) : (
            entries.map((entry) => (
              <EntryRow
                key={entry.id}
                entry={entry}
                provider={providers.find((provider) => provider.id === entry.provider_id) ?? null}
                isBusy={isBusy}
                onRemove={() => onAction({ remove_entry: { virtual_model_id: model.id, entry_id: entry.id } })}
              />
            ))
          )}

          {adding ? (
            <div className="entry-add-form">
              <Select
                value={draft.providerId}
                options={providers.map((provider) => ({ value: provider.id, label: provider.display_name }))}
                disabled={isBusy || providers.length === 0}
                onChange={(value) => onDraftChange({ ...draft, providerId: value })}
              />
              {availableModels.length > 0 ? (
                <Select
                  value={draft.modelId}
                  options={[{ value: "", label: "选择模型" }, ...availableModels.map((available) => ({ value: available.id, label: available.name || available.id }))]}
                  disabled={isBusy}
                  onChange={(value) => onDraftChange({ ...draft, modelId: value })}
                />
              ) : (
                <input
                  value={draft.modelId}
                  onChange={(event) => onDraftChange({ ...draft, modelId: event.target.value })}
                  placeholder="model id"
                  disabled={isBusy}
                />
              )}
              <button
                className="secondary-action"
                type="button"
                onClick={() => {
                  const providerId = draft.providerId || providers[0]?.id || "";
                  const modelId = draft.modelId.trim();
                  if (!providerId || !modelId) return;
                  onAction({ add_entry: { virtual_model_id: model.id, provider_id: providerId, model_id: modelId } });
                  onDraftChange({ providerId, modelId: "" });
                  setAdding(false);
                }}
                disabled={isBusy || !fallbackEnabled || draft.modelId.trim().length === 0}
              >
                添加
              </button>
              <button className="ghost-action" type="button" onClick={() => setAdding(false)} disabled={isBusy}>
                取消
              </button>
            </div>
          ) : (
            <button
              className="add-entry-btn"
              type="button"
              onClick={() => {
                setAdding(true);
                if (availableModels.length === 0) void onDiscoverModels();
              }}
              disabled={isBusy || !fallbackEnabled}
            >
              <PlusIcon />
              <span>{t("fallback.addEntry")}{discovering ? t("fallback.discovering") : ""}</span>
            </button>
          )}
        </div>
      ) : null}
    </div>
  );
}

function EntryRow({
  entry,
  provider,
  isBusy,
  onRemove,
}: {
  entry: FallbackEntry;
  provider: ProviderSummary | null;
  isBusy: boolean;
  onRemove: () => void;
}) {
  const colorHex = provider?.color_hex ?? "8a8a8e";
  const initial = (provider?.display_name ?? entry.provider_id).trim().charAt(0).toUpperCase() || "?";

  return (
    <div className="entry-row">
      <span className="entry-priority">{entry.priority}</span>
      <span className="account-logo account-logo--sm" style={{ color: `#${colorHex}`, background: `#${colorHex}22` }} aria-hidden="true">
        {initial}
      </span>
      <div className="entry-info">
        <strong>{provider?.display_name ?? entry.provider_id}</strong>
        <small>{entry.model_id}</small>
      </div>
      <button
        className="row-icon-btn row-icon-btn--danger entry-remove"
        type="button"
        onClick={onRemove}
        disabled={isBusy}
        title="移除兜底项"
        aria-label="移除兜底项"
      >
        <MinusIcon />
      </button>
    </div>
  );
}
