import { type ClipboardEvent, useEffect, useRef, useState } from "react";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
import jsQR from "jsqr";
import { CheckIcon, CopyIcon, PencilIcon, RefreshIcon, TrashIcon } from "../icons";
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
import "./twofa.css";

type ListTab = "saved" | "history";

const MAX_HISTORY = 50;

// TOTP 周期(秒)。getMfaTimeRemaining() 返回 1..30,倒计时圆环据此计算弧长。
const TOTP_PERIOD = 30;
// 圆环几何(对齐 /twofa/countdown-ring.svg):r=43 → 周长 2πr。
const RING_RADIUS = 43;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

// 内联的 SVG 精灵图标(素材见 public/twofa/twofa-icons.svg)。
function Icon({ id }: { id: string }) {
  return (
    <svg className="tf-icon" aria-hidden="true">
      <use href={`/twofa/twofa-icons.svg#${id}`} />
    </svg>
  );
}

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

/** Display-only grouping of an all-digit code into two halves (e.g. `428916` →
 *  `428 916`), matching the design. Copy/verify always use the raw token. */
function formatCode(code: string): string {
  if (!code || !/^\d+$/.test(code)) return code;
  const mid = Math.ceil(code.length / 2);
  return `${code.slice(0, mid)} ${code.slice(mid)}`;
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
  const [searchQuery, setSearchQuery] = useState("");
  const uploadInputRef = useRef<HTMLInputElement | null>(null);
  const secretInputRef = useRef<HTMLInputElement | null>(null);
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
  // 仅用于列表展示的客户端过滤(不落盘、不改数据层)。
  const search = searchQuery.trim().toLowerCase();
  const filteredRecords = search
    ? visibleRecords.filter(
        (record) => (record.accountName || "").toLowerCase().includes(search) || record.secret.toLowerCase().includes(search),
      )
    : visibleRecords;
  // 倒计时圆环弧长:剩余时间越少,弧越短(30s 满环,0s 空环)。
  const ringOffset = RING_CIRCUMFERENCE * (1 - timeRemaining / TOTP_PERIOD);

  function focusSecretInput() {
    const el = secretInputRef.current;
    if (!el) return;
    el.scrollIntoView({ behavior: "smooth", block: "center" });
    el.focus();
  }

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
    <section className="section-page twofa-redesign">
      <header className="page-topbar" data-tauri-drag-region>
        <div className="tf-topline" data-tauri-drag-region="false">
          <h1>{t("title.two_factor")}</h1>
          <span className="tf-status-pill">
            <Icon id="icon-shield" />
            {t("twoFactor.statusPill", "本地 TOTP")}
          </span>
        </div>
      </header>

      <article className="panel tf-notice">
        <Icon id="icon-2fa" />
        <span>{t("twoFactor.desc")}</span>
      </article>

      <section className="tf-grid">
        <article className="panel tf-generator">
          <h2>{t("twoFactor.generateTitle", "生成 TOTP 验证码")}</h2>

          <div className="tf-input">
            <Icon id="icon-search" />
            <input value={nameValue} onChange={(event) => setNameValue(event.target.value)} placeholder={t("twoFactor.namePlaceholder")} />
          </div>

          <div className="tf-input-row">
            <div className="tf-input">
              <Icon id="icon-key" />
              <input
                ref={secretInputRef}
                value={inputValue}
                onChange={(event) => setInputValue(event.target.value)}
                onPaste={handlePasteImage}
                placeholder={t("twoFactor.inputPlaceholder")}
              />
            </div>
            <button className="button" type="button" onClick={() => parseAndQuery(inputValue)}>
              {t("twoFactor.query")}
            </button>
            <button className="button primary" type="button" onClick={saveCurrentInput}>
              {t("twoFactor.save")}
            </button>
            <button className="button" type="button" onClick={() => uploadInputRef.current?.click()} disabled={recognizingImage}>
              <Icon id="icon-qr" />
              {recognizingImage ? "…" : "QR"}
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

          <div className="tf-code-panel">
            <div className="tf-code-main">
              <div className="tf-code-label">
                {t("twoFactor.currentCode")}
                <button className="tf-code-copy" type="button" disabled={!activeToken} onClick={() => void copyText("active", activeToken)} aria-label={t("twoFactor.copy")}>
                  {copiedId === "active" ? <CheckIcon /> : <CopyIcon />}
                </button>
              </div>
              {activeToken ? (
                <div className="tf-code">{formatCode(activeToken)}</div>
              ) : (
                <div className="tf-code-empty">{t("twoFactor.noCode")}</div>
              )}
              {inputError ? <p className="tf-error">{inputError}</p> : null}
            </div>
            {/* 倒计时的彩弧和秒数只在有活跃验证码时显示;没输密钥时环保持静态、显示「--」,
                不再空转倒数。 */}
            <svg
              className={activeToken ? "tf-countdown" : "tf-countdown tf-countdown--idle"}
              viewBox="0 0 112 112"
              fill="none"
              role="img"
              aria-label={activeToken ? `${timeRemaining}s` : t("twoFactor.noCode")}
            >
              <circle className="tf-countdown-track" cx="56" cy="56" r={RING_RADIUS} strokeWidth="12" />
              {activeToken ? (
                <circle
                  className="tf-countdown-arc"
                  cx="56"
                  cy="56"
                  r={RING_RADIUS}
                  strokeWidth="12"
                  strokeLinecap="round"
                  transform="rotate(-90 56 56)"
                  strokeDasharray={RING_CIRCUMFERENCE}
                  strokeDashoffset={ringOffset}
                />
              ) : null}
              <circle cx="56" cy="56" r="32" fill="#FFF8EC" />
              <text className="tf-countdown-text" x="56" y="63" textAnchor="middle">
                {activeToken ? `${timeRemaining}s` : "--"}
              </text>
            </svg>
          </div>

          <div className="tf-hint">
            <Icon id="icon-shield" />
            {t("twoFactor.codeHint", "验证码每 30 秒更新一次，请及时使用。")}
          </div>
        </article>

        <aside className="panel tf-vault">
          <div className="tf-vault-head">
            <h2>{t("twoFactor.vaultTitle", "本地保险箱")}</h2>
            <span className="tf-vault-sub">{t("twoFactor.vaultSubtitle", "完全本地 · 安全隐私")}</span>
          </div>
          <div className="tf-secure-list">
            <div className="tf-secure-row">
              <div className="tf-secure-icon">
                <Icon id="icon-2fa" />
              </div>
              <div className="tf-secure-text">
                <strong>{t("twoFactor.vaultLocalTitle", "本地保存")}</strong>
                <span>{t("twoFactor.vaultLocalDesc", "所有密钥与数据仅存储在本机，不会上传到云端。")}</span>
              </div>
              <svg className="tf-icon tf-secure-flag" aria-hidden="true">
                <use href="/twofa/twofa-icons.svg#icon-shield" />
              </svg>
            </div>
            <div className="tf-secure-row">
              <div className="tf-secure-icon purple">
                <Icon id="icon-shield" />
              </div>
              <div className="tf-secure-text">
                <strong>{t("twoFactor.vaultEncryptTitle", "加密保险箱")}</strong>
                <span>{t("twoFactor.vaultEncryptDesc", "密钥使用本地加密存储，应用重启后仍安全可用。")}</span>
              </div>
              <svg className="tf-icon tf-secure-flag" aria-hidden="true">
                <use href="/twofa/twofa-icons.svg#icon-shield" />
              </svg>
            </div>
            <div className="tf-secure-row">
              <div className="tf-secure-icon blue">
                <Icon id="icon-download" />
              </div>
              <div className="tf-secure-text">
                <strong>{t("twoFactor.vaultBackupTitle", "定期备份")}</strong>
                <span>{t("twoFactor.vaultBackupDesc", "导出备份文件，防止误删或重装导致数据丢失。")}</span>
              </div>
              <span className="tf-secure-chev">›</span>
            </div>
          </div>
          <div className="tf-vault-actions">
            <button className="button" type="button" onClick={() => void importRecords()}>
              <Icon id="icon-upload" />
              {t("twoFactor.importKeys", "导入密钥")}
            </button>
            <button className="button" type="button" onClick={() => void exportRecords()} disabled={records.length === 0}>
              <Icon id="icon-download" />
              {t("twoFactor.exportBackup", "导出备份")}
            </button>
          </div>
        </aside>
      </section>

      <section className="panel tf-saved">
        <div className="tf-saved-top">
          <div className="tf-tabs">
            <button className={activeTab === "saved" ? "tf-tab active" : "tf-tab"} type="button" onClick={() => setActiveTab("saved")}>
              {t("twoFactor.saved")}
            </button>
            <button className={activeTab === "history" ? "tf-tab active" : "tf-tab"} type="button" onClick={() => setActiveTab("history")}>
              {t("twoFactor.history")}
            </button>
          </div>
          <div className="tf-saved-actions">
            <button className="button" type="button" onClick={() => void importRecords()}>
              <Icon id="icon-upload" />
              {t("twoFactor.import")}
            </button>
            <button className="button" type="button" onClick={() => void exportRecords()} disabled={records.length === 0}>
              <Icon id="icon-download" />
              {t("twoFactor.export")}
            </button>
            {activeTab === "history" ? (
              <button className="button" type="button" onClick={() => setHistoryRecords([])}>
                {t("twoFactor.clearHistory")}
              </button>
            ) : null}
          </div>
        </div>
        <div className="tf-saved-body">
          <div className="tf-input tf-search">
            <Icon id="icon-search" />
            <input value={searchQuery} onChange={(event) => setSearchQuery(event.target.value)} placeholder={t("twoFactor.searchPlaceholder", "搜索名称或邮箱")} />
          </div>

          {visibleRecords.length === 0 ? (
            activeTab === "history" ? (
              <div className="tf-nomatch">{t("twoFactor.historyEmpty", "暂无历史记录")}</div>
            ) : (
              <div className="tf-empty">
                <div>
                  <img src="/twofa/vault-empty.svg" alt="" />
                  <strong>{t("twoFactor.emptyTitle", "尚未保存任何密钥")}</strong>
                  <p>{t("twoFactor.emptyDesc", "通过上方输入区保存第一个 TOTP 密钥，或导入已有备份。")}</p>
                  <div className="tf-empty-buttons">
                    <button className="button primary" type="button" onClick={focusSecretInput}>
                      {t("twoFactor.emptyCreate", "新建并保存密钥")}
                    </button>
                    <button className="button" type="button" onClick={() => void importRecords()}>
                      {t("twoFactor.emptyImport", "导入备份文件")}
                    </button>
                  </div>
                </div>
              </div>
            )
          ) : filteredRecords.length === 0 ? (
            <div className="tf-nomatch">{t("twoFactor.noMatch", "未找到匹配的密钥")}</div>
          ) : (
            <div className="tf-records">
              {filteredRecords.map((record) => {
                const token = getMfaOtpToken(record.secret);
                return (
                  <div className="tf-record" key={record.id}>
                    <div className="tf-record-main">
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
                    <div className="tf-record-code">
                      <span>{token ? formatCode(token) : "------"}</span>
                      <small>{timeRemaining}s</small>
                    </div>
                    <div className="tf-record-actions">
                      <button className="tf-icon-btn" type="button" onClick={() => void copyText(record.id, token)} disabled={!token} aria-label={t("twoFactor.copy")}>
                        {copiedId === record.id ? <CheckIcon /> : <CopyIcon />}
                      </button>
                      {activeTab === "saved" ? (
                        <button className="tf-icon-btn" type="button" onClick={() => startEdit(record)} aria-label={t("twoFactor.editName")}>
                          <PencilIcon />
                        </button>
                      ) : (
                        <button
                          className="tf-icon-btn"
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
                      <button className="tf-icon-btn tf-icon-btn--danger" type="button" onClick={() => void deleteRecord(record, activeTab)} aria-label={t("twoFactor.delete")}>
                        <TrashIcon />
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </section>
    </section>
  );
}
