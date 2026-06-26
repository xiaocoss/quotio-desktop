import { type ClipboardEvent, useEffect, useRef, useState } from "react";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
import jsQR from "jsqr";
import { CheckIcon, CopyIcon, KeyIcon, PencilIcon, RefreshIcon, TrashIcon } from "../icons";
import { useT } from "../../i18n";
import {
  MFA_STORAGE_KEY_HISTORY,
  MFA_STORAGE_KEY_SAVED,
  createMfaRecordId,
  dedupeMfaRecordsBySecret,
  getMfaOtpToken,
  getMfaTimeRemaining,
  loadMfaHistoryRecords,
  loadSavedMfaRecords,
  normalizeMfaRecord,
  parseMfaCredentialInput,
  toMfaSecretIdentity,
  type MfaRecord,
  type ParsedMfaCredential,
} from "../../lib/mfaVault";
import "./TwoFactorAuthScreen.css";

type ListTab = "saved" | "history";

const MAX_HISTORY = 50;

/** Keep `head` + `tail` chars, replace the middle with a fixed `****` (fixed so the
 *  hidden length isn't leaked). Display-only — the full value is still used to
 *  generate codes. */
function maskMiddle(value: string, head: number, tail: number): string {
  const v = value.trim();
  if (v.length <= head + tail) return v.length <= 1 ? v : `${v.slice(0, 1)}****`;
  return `${v.slice(0, head)}****${v.slice(-tail)}`;
}

/** Mask an account name; if it's an email, mask only the local part and keep the
 *  domain (e.g. `jel****5b@icloud.com`). */
function maskAccountName(value: string): string {
  const v = value.trim();
  const at = v.indexOf("@");
  if (at > 0) return `${maskMiddle(v.slice(0, at), 3, 2)}${v.slice(at)}`;
  return maskMiddle(v, 3, 2);
}

async function decodeQrTextFromImage(file: Blob): Promise<string | null> {
  const imageUrl = URL.createObjectURL(file);
  try {
    const image = await new Promise<HTMLImageElement>((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve(img);
      img.onerror = reject;
      img.src = imageUrl;
    });

    const maxSide = 2200;
    const scale = Math.min(1, maxSide / Math.max(image.naturalWidth, image.naturalHeight));
    const width = Math.max(1, Math.round(image.naturalWidth * scale));
    const height = Math.max(1, Math.round(image.naturalHeight * scale));
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    if (!context) return null;
    context.drawImage(image, 0, 0, width, height);
    const imageData = context.getImageData(0, 0, width, height);
    const result = jsQR(imageData.data, imageData.width, imageData.height, { inversionAttempts: "attemptBoth" });
    return result?.data?.trim() || null;
  } finally {
    URL.revokeObjectURL(imageUrl);
  }
}

export function TwoFactorAuthScreen() {
  const t = useT();
  const [records, setRecords] = useState<MfaRecord[]>(() => loadSavedMfaRecords());
  const [historyRecords, setHistoryRecords] = useState<MfaRecord[]>(() => loadMfaHistoryRecords());
  const [inputValue, setInputValue] = useState("");
  const [nameValue, setNameValue] = useState("");
  const [inputError, setInputError] = useState("");
  const [activeQuery, setActiveQuery] = useState<ParsedMfaCredential | null>(null);
  const [activeTab, setActiveTab] = useState<ListTab>("saved");
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [recognizingImage, setRecognizingImage] = useState(false);
  const [timeRemaining, setTimeRemaining] = useState(() => getMfaTimeRemaining());
  const uploadInputRef = useRef<HTMLInputElement | null>(null);
  // 跳过首挂载的回写:records/historyRecords 初值来自 load*(已去重/规范化),首挂载
  // 就写回会用规范化结果静默覆盖磁盘上的原始数据;只在用户真正改动后才持久化。
  const savedHydrated = useRef(false);
  const historyHydrated = useRef(false);

  useEffect(() => {
    if (!savedHydrated.current) {
      savedHydrated.current = true;
      return;
    }
    localStorage.setItem(MFA_STORAGE_KEY_SAVED, JSON.stringify(records));
  }, [records]);

  useEffect(() => {
    if (!historyHydrated.current) {
      historyHydrated.current = true;
      return;
    }
    localStorage.setItem(MFA_STORAGE_KEY_HISTORY, JSON.stringify(historyRecords));
  }, [historyRecords]);

  useEffect(() => {
    const timer = window.setInterval(() => setTimeRemaining(getMfaTimeRemaining()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const activeToken = activeQuery ? getMfaOtpToken(activeQuery.secret) : "";
  const visibleRecords = activeTab === "saved" ? records : historyRecords;

  function applyQueryResult(parsed: ParsedMfaCredential) {
    setActiveQuery(parsed);
    setInputError("");
    setHistoryRecords((prev) => {
      const next: MfaRecord = {
        id: createMfaRecordId(),
        accountName: parsed.accountName,
        secret: parsed.secret,
        remark: "",
        time: Date.now(),
      };
      const identity = toMfaSecretIdentity(next.secret);
      return [next, ...prev.filter((record) => toMfaSecretIdentity(record.secret) !== identity)].slice(0, MAX_HISTORY);
    });
  }

  function parseAndQuery(rawInput: string, invalidMessage = t("twoFactor.invalidInput")) {
    const parsed = parseMfaCredentialInput(rawInput);
    if (!parsed) {
      setInputError(invalidMessage);
      return null;
    }
    applyQueryResult(parsed);
    return parsed;
  }

  function saveCurrentInput() {
    const parsed = parseMfaCredentialInput(inputValue);
    if (!parsed) {
      setInputError(t("twoFactor.invalidInput"));
      return;
    }

    // A manually-typed name/email wins over the otpauth-derived one.
    const accountName = nameValue.trim() || parsed.accountName || activeQuery?.accountName || "";
    setRecords((prev) => {
      const identity = toMfaSecretIdentity(parsed.secret);
      const existingIndex = prev.findIndex((record) => toMfaSecretIdentity(record.secret) === identity);
      if (existingIndex >= 0) {
        return prev.map((record, index) => (index === existingIndex ? { ...record, accountName: accountName || record.accountName } : record));
      }
      return [{ id: createMfaRecordId(), accountName, secret: parsed.secret, remark: "", time: Date.now() }, ...prev];
    });
    setInputError("");
    setNameValue("");
    setInputValue("");
  }

  async function copyText(id: string, text: string) {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      setCopiedId(id);
      window.setTimeout(() => setCopiedId(null), 1200);
    } catch {
      // Clipboard permission is best-effort.
    }
  }

  async function exportRecords() {
    if (records.length === 0) return;
    const data = JSON.stringify(records.map(({ accountName, secret, time }) => ({ accountName, secret, time })), null, 2);
    const defaultPath = `quotio_2fa_${new Date().toISOString().slice(0, 10)}.json`;
    try {
      const filePath = await save({ defaultPath, filters: [{ name: "JSON", extensions: ["json"] }] });
      if (filePath) await writeTextFile(filePath, data);
    } catch {
      const blob = new Blob([data], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = defaultPath;
      link.click();
      URL.revokeObjectURL(url);
    }
  }

  async function importRecords() {
    try {
      let text = "";
      try {
        const selected = await open({ multiple: false, filters: [{ name: "JSON", extensions: ["json"] }] });
        if (!selected) return;
        text = await readTextFile(Array.isArray(selected) ? selected[0] : selected);
      } catch {
        const input = document.createElement("input");
        input.type = "file";
        input.accept = "application/json,.json";
        text = await new Promise<string>((resolve, reject) => {
          input.onchange = () => {
            const file = input.files?.[0];
            if (!file) return resolve("");
            const reader = new FileReader();
            reader.onload = () => resolve(String(reader.result || ""));
            reader.onerror = reject;
            reader.readAsText(file);
          };
          input.click();
        });
      }
      if (!text) return;
      const parsed = JSON.parse(text);
      if (!Array.isArray(parsed)) throw new Error("Invalid import payload");
      const incoming = parsed.map(normalizeMfaRecord).filter((item): item is MfaRecord => Boolean(item));
      setRecords((prev) => dedupeMfaRecordsBySecret([...incoming, ...prev]));
    } catch {
      setInputError(t("twoFactor.importFailed"));
    }
  }

  async function decodeAndQueryImage(file: Blob) {
    setRecognizingImage(true);
    setInputError("");
    try {
      const text = await decodeQrTextFromImage(file);
      if (!text) {
        setInputError(t("twoFactor.qrDecodeFailed"));
        return;
      }
      setInputValue(text);
      parseAndQuery(text, t("twoFactor.qrNotOtpAuth"));
    } finally {
      setRecognizingImage(false);
    }
  }

  function handlePasteImage(event: ClipboardEvent<HTMLInputElement>) {
    const imageItem = Array.from(event.clipboardData.items).find((item) => item.type.startsWith("image/"));
    if (!imageItem) return;
    event.preventDefault();
    const file = imageItem.getAsFile();
    if (file) void decodeAndQueryImage(file);
  }

  function startEdit(record: MfaRecord) {
    setEditingId(record.id);
    setEditingName(record.accountName);
  }

  function saveEdit() {
    if (!editingId) return;
    setRecords((prev) => prev.map((record) => (record.id === editingId ? { ...record, accountName: editingName.trim() } : record)));
    setEditingId(null);
    setEditingName("");
  }

  async function deleteRecord(record: MfaRecord, tab: ListTab) {
    const ok = await confirm(t("twoFactor.delete"), { title: t("twoFactor.delete"), kind: "warning" }).catch(() => window.confirm(t("twoFactor.delete")));
    if (!ok) return;
    if (tab === "saved") setRecords((prev) => prev.filter((item) => item.id !== record.id));
    else setHistoryRecords((prev) => prev.filter((item) => item.id !== record.id));
  }

  return (
    <section className="section-page two-factor-page">
      <header className="page-topbar" data-tauri-drag-region>
        <h1>{t("title.two_factor")}</h1>
      </header>

      <article className="panel two-factor-intro">
        <KeyIcon />
        <span>{t("twoFactor.desc")}</span>
      </article>

      <article className="panel two-factor-query-panel">
        <div className="two-factor-input-row">
          <input
            className="two-factor-name-input"
            value={nameValue}
            onChange={(event) => setNameValue(event.target.value)}
            placeholder={t("twoFactor.namePlaceholder")}
          />
          <input value={inputValue} onChange={(event) => setInputValue(event.target.value)} onPaste={handlePasteImage} placeholder={t("twoFactor.inputPlaceholder")} />
          <button className="secondary-action" type="button" onClick={() => parseAndQuery(inputValue)}>
            {t("twoFactor.query")}
          </button>
          <button className="secondary-action" type="button" onClick={saveCurrentInput}>
            {t("twoFactor.save")}
          </button>
          <button className="ghost-action" type="button" onClick={() => uploadInputRef.current?.click()} disabled={recognizingImage}>
            {recognizingImage ? "..." : "QR"}
          </button>
          <input
            ref={uploadInputRef}
            type="file"
            accept="image/*"
            hidden
            onChange={(event) => {
              const file = event.currentTarget.files?.[0];
              event.currentTarget.value = "";
              if (file) void decodeAndQueryImage(file);
            }}
          />
        </div>
        {inputError ? <p className="two-factor-error">{inputError}</p> : null}
        <div className="two-factor-current-code">
          <span>{t("twoFactor.currentCode")}</span>
          <strong className={activeToken ? "two-factor-code-value" : "two-factor-code-placeholder"}>{activeToken || t("twoFactor.noCode")}</strong>
          <small>{timeRemaining}s</small>
          <button className="row-icon-btn" type="button" disabled={!activeToken} onClick={() => void copyText("active", activeToken)}>
            {copiedId === "active" ? <CheckIcon /> : <CopyIcon />}
          </button>
        </div>
      </article>

      <article className="panel two-factor-list-panel">
        <div className="two-factor-list-head">
          <div className="two-factor-tabs">
            <button className={activeTab === "saved" ? "active" : ""} type="button" onClick={() => setActiveTab("saved")}>
              {t("twoFactor.saved")}
            </button>
            <button className={activeTab === "history" ? "active" : ""} type="button" onClick={() => setActiveTab("history")}>
              {t("twoFactor.history")}
            </button>
          </div>
          <div className="two-factor-actions">
            <button className="ghost-action" type="button" onClick={() => void importRecords()}>
              {t("twoFactor.import")}
            </button>
            <button className="ghost-action" type="button" onClick={() => void exportRecords()} disabled={records.length === 0}>
              {t("twoFactor.export")}
            </button>
            {activeTab === "history" ? (
              <button className="ghost-action" type="button" onClick={() => setHistoryRecords([])}>
                {t("twoFactor.clearHistory")}
              </button>
            ) : null}
          </div>
        </div>
        <div className="two-factor-records">
          {visibleRecords.length === 0 ? <p className="empty-copy">{t("twoFactor.noCode")}</p> : null}
          {visibleRecords.map((record) => {
            const token = getMfaOtpToken(record.secret);
            return (
              <div className="two-factor-record" key={record.id}>
                <div className="two-factor-record-main">
                  {editingId === record.id ? (
                    <input
                      value={editingName}
                      onChange={(event) => setEditingName(event.target.value)}
                      onBlur={saveEdit}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") saveEdit();
                      }}
                      autoFocus
                    />
                  ) : (
                    <strong>{record.accountName ? maskAccountName(record.accountName) : t("twoFactor.unnamed")}</strong>
                  )}
                  <code>{maskMiddle(record.secret, 4, 4)}</code>
                </div>
                <div className="two-factor-record-code">
                  <span>{token || "------"}</span>
                  <small>{timeRemaining}s</small>
                </div>
                <button className="row-icon-btn" type="button" onClick={() => void copyText(record.id, token)} disabled={!token}>
                  {copiedId === record.id ? <CheckIcon /> : <CopyIcon />}
                </button>
                {activeTab === "saved" ? (
                  <button className="row-icon-btn" type="button" onClick={() => startEdit(record)} aria-label={t("twoFactor.editName")}>
                    <PencilIcon />
                  </button>
                ) : (
                  <button
                    className="row-icon-btn"
                    type="button"
                    onClick={() => {
                      setInputValue(record.secret);
                      setActiveQuery({ accountName: record.accountName, secret: record.secret });
                    }}
                    aria-label={t("common.refresh")}
                  >
                    <RefreshIcon />
                  </button>
                )}
                <button className="row-icon-btn row-icon-btn--danger" type="button" onClick={() => void deleteRecord(record, activeTab)} aria-label={t("twoFactor.delete")}>
                  <TrashIcon />
                </button>
              </div>
            );
          })}
        </div>
      </article>
    </section>
  );
}
