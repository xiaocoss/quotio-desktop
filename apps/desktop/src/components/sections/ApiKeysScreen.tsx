import { useState } from "react";
import type { ApiKeyEntry, AppState } from "../../types";
import { CheckIcon, CopyIcon, KeyIcon, PencilIcon, PlusIcon, TrashIcon } from "../icons";
import { useT } from "../../i18n";

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

  const [showAdd, setShowAdd] = useState(false);
  const [newApiKey, setNewApiKey] = useState("");
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [replacementApiKey, setReplacementApiKey] = useState("");

  function addKey() {
    const value = newApiKey.trim();
    if (!value) return;
    onRunManagementStateAction("add_api_key", { key: value });
    setNewApiKey("");
    setShowAdd(false);
  }

  return (
    <section className="section-page api-keys-page">
      <header className="page-topbar" data-tauri-drag-region>
        <h1>{t("nav.api_keys")}</h1>
        <div className="topbar-actions">
          <button
            className="icon-button"
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
          </button>
          <button
            className={showAdd ? "icon-button icon-button--active" : "icon-button"}
            type="button"
            onClick={() => setShowAdd((value) => !value)}
            disabled={isManagementBusy}
            title="新增密钥"
            aria-label="新增密钥"
          >
            <PlusIcon />
          </button>
        </div>
      </header>

      <article className="panel api-keys-panel">
        <div className="panel-label">
          <span className="eyebrow">{t("nav.api_keys")}</span>
          <span className="count-pill">{apiKeys.length}</span>
        </div>

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
          <p className="empty-copy">暂无密钥。点击右上角生成或新增一个客户端密钥。</p>
        ) : (
          <div className="apikey-list">
            {apiKeys.map((entry, index) => (
              <ApiKeyRow
                key={`${index}-${entry.masked_value}`}
                entry={entry}
                rawValue={entry.value}
                isEditing={editingIndex === index}
                replacementValue={replacementApiKey}
                isBusy={isManagementBusy}
                onReplacementChange={setReplacementApiKey}
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
            ))}
          </div>
        )}

        <p className="empty-copy apikey-foot">{t("apikeys.foot")}</p>
      </article>
    </section>
  );
}

function ApiKeyRow({
  entry,
  rawValue,
  isEditing,
  replacementValue,
  isBusy,
  onReplacementChange,
  onEdit,
  onCancel,
  onSave,
  onDelete,
}: {
  entry: ApiKeyEntry;
  rawValue: string;
  isEditing: boolean;
  replacementValue: string;
  isBusy: boolean;
  onReplacementChange: (value: string) => void;
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

  return (
    <div className="apikey-row">
      <code className="apikey-value">{entry.masked_value}</code>
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

function generateApiKey() {
  const alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
  const randomPart = Array.from({ length: 32 }, () => alphabet[Math.floor(Math.random() * alphabet.length)]).join("");
  return `sk-${randomPart}`;
}
