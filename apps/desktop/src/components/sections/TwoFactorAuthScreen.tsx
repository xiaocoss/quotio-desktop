import { type ClipboardEvent, type CSSProperties, useEffect, useRef, useState } from "react";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
import jsQR from "jsqr";
import { CheckIcon, CopyIcon, EyeIcon, EyeOffIcon, PencilIcon, PlusIcon, RefreshIcon, TrashIcon } from "../icons";
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
import "./twofa-rose.css";

type ListTab = "saved" | "history";

const MAX_HISTORY = 50;
const MFA_STORAGE_KEY_LAST_BACKUP = "quotio.mfa.last-backup-at.v1";

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

function describeMfaRecord(record: MfaRecord): { provider: string; account: string } {
  const value = record.accountName.trim();
  if (!value) return { provider: "TOTP", account: "" };

  const colon = value.indexOf(":");
  if (colon > 0 && colon < value.length - 1) {
    return { provider: value.slice(0, colon).trim(), account: value.slice(colon + 1).trim() };
  }

  const at = value.lastIndexOf("@");
  if (at > 0 && at < value.length - 1) {
    const domain = value.slice(at + 1).split(".")[0] || "TOTP";
    return { provider: `${domain.slice(0, 1).toUpperCase()}${domain.slice(1)}`, account: value };
  }

  return { provider: value, account: value };
}

function getRecordTone(record: MfaRecord): number {
  return Array.from(record.accountName || record.secret).reduce((total, char) => total + char.charCodeAt(0), 0) % 5;
}

function RoseProviderMark({ provider, tone }: { provider: string; tone: number }) {
  const key = provider.trim().toLowerCase();

  if (key.includes("github")) {
    return (
      <span className="rose-tf-record-mark rose-tf-brand-mark" data-brand="github">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M12 .7A11.5 11.5 0 0 0 8.36 23.1c.58.11.79-.25.79-.56v-2.24c-3.22.7-3.9-1.37-3.9-1.37-.53-1.34-1.29-1.7-1.29-1.7-1.05-.72.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.76 2.69 1.25 3.35.96.1-.75.4-1.25.73-1.54-2.57-.29-5.27-1.28-5.27-5.68 0-1.26.45-2.28 1.19-3.09-.12-.29-.52-1.47.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77-.01c2.2-1.49 3.17-1.18 3.17-1.18.63 1.58.23 2.76.11 3.05.74.81 1.19 1.83 1.19 3.09 0 4.41-2.71 5.38-5.29 5.67.42.36.79 1.07.79 2.16v3.2c0 .31.21.68.8.56A11.5 11.5 0 0 0 12 .7Z" />
        </svg>
      </span>
    );
  }

  if (key.includes("google")) {
    return <span className="rose-tf-record-mark rose-tf-brand-mark" data-brand="google" aria-hidden="true">G</span>;
  }

  if (key.includes("microsoft")) {
    return (
      <span className="rose-tf-record-mark rose-tf-brand-mark" data-brand="microsoft">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path fill="#f25022" d="M2 2h9v9H2z" />
          <path fill="#7fba00" d="M13 2h9v9h-9z" />
          <path fill="#00a4ef" d="M2 13h9v9H2z" />
          <path fill="#ffb900" d="M13 13h9v9h-9z" />
        </svg>
      </span>
    );
  }

  if (key.includes("binance")) {
    return (
      <span className="rose-tf-record-mark rose-tf-brand-mark" data-brand="binance">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="m12 2 3.1 3.1L12 8.2 8.9 5.1 12 2Zm-5.1 5.1L10 10.2 6.9 13.3 3.8 10.2l3.1-3.1Zm10.2 0 3.1 3.1-3.1 3.1-3.1-3.1 3.1-3.1ZM12 9.1l2.9 2.9-2.9 2.9L9.1 12 12 9.1Zm-5.1 5.6 3.1 3.1-3.1 3.1-3.1-3.1 3.1-3.1Zm10.2 0 3.1 3.1-3.1 3.1-3.1-3.1 3.1-3.1ZM12 15.8l3.1 3.1L12 22l-3.1-3.1 3.1-3.1Z" />
        </svg>
      </span>
    );
  }

  if (key.includes("dropbox")) {
    return (
      <span className="rose-tf-record-mark rose-tf-brand-mark" data-brand="dropbox">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="m6.3 3 5.7 3.5-5.7 3.6L.6 6.5 6.3 3Zm11.4 0 5.7 3.5-5.7 3.6L12 6.5 17.7 3ZM6.3 10.8l5.7 3.6-5.7 3.5-5.7-3.5 5.7-3.6Zm11.4 0 5.7 3.6-5.7 3.5-5.7-3.5 5.7-3.6ZM12 15.4l5.7 3.5L12 22.4l-5.7-3.5 5.7-3.5Z" />
        </svg>
      </span>
    );
  }

  return <span className="rose-tf-record-mark" data-tone={tone}>{provider.slice(0, 2).toUpperCase()}</span>;
}

function formatRecordTime(timestamp: number): string {
  if (!Number.isFinite(timestamp)) return "—";
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return "—";
  const pad = (value: number) => String(value).padStart(2, "0");
  return [
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`,
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`,
  ].join(" ");
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

export function TwoFactorAuthScreen({ roseMode = false }: { roseMode?: boolean }) {
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
  const [selectedRecordId, setSelectedRecordId] = useState<string | null>(null);
  const [composerOpen, setComposerOpen] = useState(false);
  const [secretRevealed, setSecretRevealed] = useState(false);
  const [lastBackupAt, setLastBackupAt] = useState<number | null>(() => {
    const stored = Number(localStorage.getItem(MFA_STORAGE_KEY_LAST_BACKUP));
    return Number.isFinite(stored) && stored > 0 ? stored : null;
  });
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

  useEffect(() => {
    if (!roseMode) return;
    const candidates = activeTab === "saved" ? records : historyRecords;
    if (candidates.length === 0) {
      setSelectedRecordId(null);
      return;
    }
    if (!selectedRecordId || !candidates.some((record) => record.id === selectedRecordId)) {
      setSelectedRecordId(candidates[0].id);
    }
  }, [activeTab, historyRecords, records, roseMode, selectedRecordId]);

  const activeToken = activeQuery ? getMfaOtpToken(activeQuery.secret) : "";
  const visibleRecords = activeTab === "saved" ? records : historyRecords;
  // 仅用于列表展示的客户端过滤(不落盘、不改数据层)。
  const search = searchQuery.trim().toLowerCase();
  const filteredRecords = search
    ? visibleRecords.filter(
        (record) => (record.accountName || "").toLowerCase().includes(search) || record.secret.toLowerCase().includes(search),
      )
    : visibleRecords;
  const selectedRecord = visibleRecords.find((record) => record.id === selectedRecordId) ?? visibleRecords[0] ?? null;
  const selectedToken = selectedRecord ? getMfaOtpToken(selectedRecord.secret) : "";
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
    if (!roseMode) {
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
      return;
    }

    const identity = toMfaSecretIdentity(parsed.secret);
    const existingRecord = records.find((record) => toMfaSecretIdentity(record.secret) === identity);
    const savedRecordId = existingRecord?.id ?? createMfaRecordId();
    setRecords((prev) => {
      const existingIndex = prev.findIndex((record) => toMfaSecretIdentity(record.secret) === identity);
      if (existingIndex >= 0) {
        return prev.map((record, index) => (index === existingIndex ? { ...record, accountName: accountName || record.accountName } : record));
      }
      return [{ id: savedRecordId, accountName, secret: parsed.secret, remark: "", time: Date.now() }, ...prev];
    });
    setInputError("");
    setNameValue("");
    setInputValue("");
    setActiveTab("saved");
    setSelectedRecordId(savedRecordId);
    setComposerOpen(false);
    setSecretRevealed(false);
    setActiveQuery({ accountName, secret: parsed.secret });
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
    const markBackupCreated = () => {
      const timestamp = Date.now();
      setLastBackupAt(timestamp);
      localStorage.setItem(MFA_STORAGE_KEY_LAST_BACKUP, String(timestamp));
    };
    try {
      const filePath = await save({ defaultPath, filters: [{ name: "JSON", extensions: ["json"] }] });
      if (filePath) {
        await writeTextFile(filePath, data);
        markBackupCreated();
      }
    } catch {
      const blob = new Blob([data], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = defaultPath;
      link.click();
      URL.revokeObjectURL(url);
      markBackupCreated();
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

  function openRoseComposer() {
    setComposerOpen(true);
    setActiveQuery(null);
    setInputError("");
    setNameValue("");
    setInputValue("");
    setSecretRevealed(false);
  }

  function selectRoseRecord(record: MfaRecord) {
    setSelectedRecordId(record.id);
    setComposerOpen(false);
    setSecretRevealed(false);
    setActiveQuery({ accountName: record.accountName, secret: record.secret });
  }

  if (roseMode) {
    const selectedDescription = selectedRecord ? describeMfaRecord(selectedRecord) : null;
    const listTitle = activeTab === "saved" ? t("twoFactor.saved", "已保存") : t("twoFactor.history", "历史");
    const progressStyle = { "--rose-tf-progress": `${Math.max(0, Math.min(30, timeRemaining)) * 12}deg` } as CSSProperties;

    return (
      <section className="section-page twofa-redesign rose-twofa">
        <header className="page-topbar rose-tf-topbar" data-tauri-drag-region>
          <div className="tf-topline" data-tauri-drag-region="false">
            <h1>{t("twoFactor.roseTitle", "2FA 验证器保险库")}</h1>
            <span className="tf-status-pill">
              <Icon id="icon-shield" />
              {t("twoFactor.statusPill", "本地 TOTP")}
            </span>
          </div>
          <div className="rose-tf-account-anchor" aria-hidden="true" data-tauri-drag-region="false">
            <span className="rose-tf-sun">
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <circle cx="12" cy="12" r="3.5" />
                <path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M18.7 5.3l-1.4 1.4M6.7 17.3l-1.4 1.4" />
              </svg>
            </span>
            <span className="rose-tf-user-card">
              <img src="/rose/character-avatar.png" alt="" />
              <strong>{t("twoFactor.roseUserLabel", "优雅的玫瑰")}</strong>
              <span>
                <svg viewBox="0 0 16 16" aria-hidden="true">
                  <path d="m4 6 4 4 4-4" />
                </svg>
              </span>
            </span>
          </div>
        </header>

        <article className="rose-tf-notice">
          <Icon id="icon-2fa" />
          <span>{t("twoFactor.roseNotice", "生成并管理本地 TOTP 动态验证码。密钥仅保存在本机；请确保已备份并妥善保管。")}</span>
        </article>

        <section className="rose-tf-workspace">
          <article className="panel rose-tf-list-panel">
            <div className="rose-tf-list-head">
              <div>
                <div className="rose-tf-list-title-row">
                  <h2>{t("twoFactor.roseAccountList", "账号列表")}</h2>
                </div>
                {historyRecords.length > 0 ? (
                  <div className="rose-tf-list-tabs" role="tablist" aria-label={t("twoFactor.roseListSource", "账号来源")}>
                    <button className={activeTab === "saved" ? "is-active" : ""} type="button" role="tab" aria-selected={activeTab === "saved"} onClick={() => setActiveTab("saved")}>
                      {t("twoFactor.saved", "已保存")} {records.length}
                    </button>
                    <button className={activeTab === "history" ? "is-active" : ""} type="button" role="tab" aria-selected={activeTab === "history"} onClick={() => setActiveTab("history")}>
                      {t("twoFactor.history", "历史")} {historyRecords.length}
                    </button>
                  </div>
                ) : null}
              </div>
              <div className="rose-tf-list-actions">
                <button
                  className="rose-tf-soft-button"
                  type="button"
                  onClick={() => {
                    openRoseComposer();
                    uploadInputRef.current?.click();
                  }}
                  disabled={recognizingImage}
                >
                  <Icon id="icon-qr" />
                  {recognizingImage ? "…" : t("twoFactor.roseImportQr", "导入二维码")}
                </button>
                <button className="rose-tf-soft-button" type="button" onClick={() => void importRecords()}>
                  <Icon id="icon-upload" />
                  {t("twoFactor.roseImportBackup", "导入备份文件")}
                </button>
                <button className="rose-tf-add-button" type="button" onClick={openRoseComposer}>
                  <PlusIcon />
                  {t("twoFactor.roseAddAccount", "添加账号")}
                </button>
              </div>
            </div>

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

            <div className="rose-tf-search">
              <Icon id="icon-search" />
              <input value={searchQuery} onChange={(event) => setSearchQuery(event.target.value)} placeholder={t("twoFactor.searchPlaceholder", "搜索账号、标签、邮箱或备注")} />
              {activeTab === "history" && historyRecords.length > 0 ? (
                <button type="button" onClick={() => setHistoryRecords([])}>{t("twoFactor.clearHistory", "清空历史")}</button>
              ) : null}
            </div>

            <div className="rose-tf-record-list" aria-label={listTitle}>
              {visibleRecords.length === 0 ? (
                <div className="rose-tf-list-empty">
                  <img src="/twofa/vault-empty.svg" alt="" />
                  <strong>{activeTab === "saved" ? t("twoFactor.emptyTitle", "尚未保存任何密钥") : t("twoFactor.historyEmpty", "暂无历史记录")}</strong>
                  <p>{activeTab === "saved" ? t("twoFactor.emptyDesc", "添加第一个 TOTP 账号，或导入已有备份。") : t("twoFactor.roseHistoryHint", "查询过的有效密钥会显示在这里。")}</p>
                  {activeTab === "saved" ? (
                    <button className="rose-tf-add-button" type="button" onClick={openRoseComposer}>
                      <PlusIcon />
                      {t("twoFactor.roseAddAccount", "添加账号")}
                    </button>
                  ) : null}
                </div>
              ) : filteredRecords.length === 0 ? (
                <div className="rose-tf-list-empty rose-tf-list-empty--compact">{t("twoFactor.noMatch", "未找到匹配的密钥")}</div>
              ) : (
                filteredRecords.map((record) => {
                  const description = describeMfaRecord(record);
                  const token = getMfaOtpToken(record.secret);
                  const selected = selectedRecord?.id === record.id && !composerOpen;
                  return (
                    <div
                      className={selected ? "rose-tf-record is-selected" : "rose-tf-record"}
                      key={record.id}
                      role="button"
                      tabIndex={0}
                      onClick={() => selectRoseRecord(record)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          selectRoseRecord(record);
                        }
                      }}
                    >
                      <RoseProviderMark provider={description.provider} tone={getRecordTone(record)} />
                      <span className="rose-tf-record-copy">
                        {editingId === record.id ? (
                          <input
                            value={editingName}
                            onChange={(event) => setEditingName(event.target.value)}
                            onClick={(event) => event.stopPropagation()}
                            onBlur={saveEdit}
                            onKeyDown={(event) => {
                              event.stopPropagation();
                              if (event.key === "Enter") saveEdit();
                            }}
                            autoFocus
                          />
                        ) : (
                          <strong>{description.provider}</strong>
                        )}
                        <small>{description.account || t("twoFactor.unnamed", "未命名密钥")}</small>
                      </span>
                      <span className="rose-tf-record-code">{token ? formatCode(token) : "--- ---"}</span>
                      <span className="rose-tf-mini-timer" style={progressStyle}>{timeRemaining}s</span>
                      <span className="rose-tf-row-actions">
                        {activeTab === "saved" ? (
                          <button type="button" onClick={(event) => { event.stopPropagation(); startEdit(record); }} aria-label={t("twoFactor.editName")}>
                            <PencilIcon />
                          </button>
                        ) : (
                          <button type="button" onClick={(event) => { event.stopPropagation(); setInputValue(record.secret); applyQueryResult({ accountName: record.accountName, secret: record.secret }); }} aria-label={t("common.refresh")}>
                            <RefreshIcon />
                          </button>
                        )}
                        <button type="button" onClick={(event) => { event.stopPropagation(); void deleteRecord(record, activeTab); }} aria-label={t("twoFactor.delete")}>
                          <TrashIcon />
                        </button>
                      </span>
                    </div>
                  );
                })
              )}
            </div>
            <section className="rose-tf-vault-summary">
              <h3>{t("twoFactor.roseVaultOverview", "保险库概览")}</h3>
              <div className="rose-tf-summary-grid">
                <div>
                  <span className="rose-tf-summary-icon">
                    <svg className="rose-tf-summary-svg" viewBox="0 0 24 24" aria-hidden="true">
                      <circle cx="9" cy="8" r="3" />
                      <circle cx="17" cy="9.5" r="2.4" />
                      <path d="M3.5 19c.3-4 2.2-6 5.5-6s5.2 2 5.5 6M14.5 14c3.7-.6 5.7 1.1 6 4.5" />
                    </svg>
                  </span>
                  <strong>{records.length}</strong>
                  <small>
                    {t("twoFactor.roseAccounts", "个账号")}
                    <span>{t("twoFactor.roseAccountsHint", "已保存的 2FA 账号总数")}</span>
                  </small>
                </div>
                <div>
                  <span className="rose-tf-summary-icon"><Icon id="icon-2fa" /></span>
                  <strong>{t("twoFactor.roseLocalStorage", "本地加密")}</strong>
                  <small>
                    {t("twoFactor.roseLocalStorageHint", "密钥仅保存在本机")}
                    <span>{t("twoFactor.roseLocalStorageSafe", "安全可靠")}</span>
                  </small>
                </div>
                <div>
                  <span className="rose-tf-summary-icon">{lastBackupAt ? <CheckIcon /> : <Icon id="icon-download" />}</span>
                  <strong>{lastBackupAt ? t("twoFactor.roseBackupCreated", "已创建备份") : t("twoFactor.roseBackupReady", "可导出备份")}</strong>
                  <small>
                    {t("twoFactor.roseBackupReadyHint", "最后备份时间")}
                    <span>{lastBackupAt ? formatRecordTime(lastBackupAt) : t("twoFactor.roseBackupMissing", "尚未创建")}</span>
                  </small>
                </div>
              </div>
              <button className="rose-tf-export-button" type="button" onClick={() => void exportRecords()} disabled={records.length === 0}>
                <Icon id="icon-upload" />
                {t("twoFactor.roseExportSummary", "导出备份")}
              </button>
            </section>
          </article>
          <aside className="panel rose-tf-detail-panel">
            {composerOpen || !selectedRecord || !selectedDescription ? (
              <div className="rose-tf-composer">
                <div className="rose-tf-detail-head">
                  <div>
                    <span>{t("twoFactor.roseNewTotp", "新建 TOTP")}</span>
                    <h2>{t("twoFactor.roseAddAccount", "添加账号")}</h2>
                  </div>
                  {selectedRecord ? (
                    <button className="rose-tf-soft-button" type="button" onClick={() => setComposerOpen(false)}>
                      {t("common.cancel", "取消")}
                    </button>
                  ) : null}
                </div>

                <div className="rose-tf-composer-card">
                  <label>
                    <span>{t("twoFactor.roseAccountName", "账号名称")}</span>
                    <div className="rose-tf-field">
                      <Icon id="icon-provider" />
                      <input value={nameValue} onChange={(event) => setNameValue(event.target.value)} placeholder={t("twoFactor.namePlaceholder")} />
                    </div>
                  </label>
                  <label>
                    <span>Secret / otpauth URI</span>
                    <div className="rose-tf-field">
                      <Icon id="icon-key" />
                      <input ref={secretInputRef} value={inputValue} onChange={(event) => setInputValue(event.target.value)} onPaste={handlePasteImage} placeholder={t("twoFactor.inputPlaceholder")} />
                    </div>
                  </label>
                  {inputError ? <p className="rose-tf-error">{inputError}</p> : null}
                  <div className="rose-tf-composer-actions">
                    <button className="rose-tf-soft-button" type="button" onClick={() => parseAndQuery(inputValue)}>{t("twoFactor.query")}</button>
                    <button className="rose-tf-soft-button" type="button" onClick={() => uploadInputRef.current?.click()} disabled={recognizingImage}>
                      <Icon id="icon-qr" />
                      {recognizingImage ? "…" : "QR"}
                    </button>
                    <button className="rose-tf-add-button" type="button" onClick={saveCurrentInput}>{t("twoFactor.save")}</button>
                  </div>
                </div>

                <div className={activeToken ? "rose-tf-query-preview has-code" : "rose-tf-query-preview"}>
                  <span>{t("twoFactor.currentCode")}</span>
                  <strong>{activeToken ? formatCode(activeToken) : "--- ---"}</strong>
                  <small>{activeToken ? t("twoFactor.codeHint", "验证码每 30 秒更新一次，请及时使用。") : t("twoFactor.noCode")}</small>
                </div>

                <div className="rose-tf-local-note">
                  <Icon id="icon-shield" />
                  <span>{t("twoFactor.roseLocalOnly", "密钥数据保存在此设备的应用数据中，请定期手动导出备份。")}</span>
                </div>
              </div>
            ) : (
              <div className="rose-tf-account-detail">
                <div className="rose-tf-detail-head">
                  <h2>{activeTab === "saved" ? t("twoFactor.roseAccountDetail", "账号详情") : t("twoFactor.history", "历史")}</h2>
                  <button className="rose-tf-delete-button" type="button" onClick={() => void deleteRecord(selectedRecord, activeTab)}>
                    <TrashIcon />
                    {t("twoFactor.roseDeleteAccount", "删除账号")}
                  </button>
                </div>

                <section className="rose-tf-code-card">
                  <div>
                    <span className="rose-tf-card-label">{t("twoFactor.roseCodeLabel", "验证码")}</span>
                    <button className="rose-tf-inline-copy" type="button" onClick={() => void copyText(`rose-code-${selectedRecord.id}`, selectedToken)} disabled={!selectedToken} aria-label={t("twoFactor.copy")}>
                      {copiedId === `rose-code-${selectedRecord.id}` ? <CheckIcon /> : <CopyIcon />}
                    </button>
                    <strong>{selectedToken ? formatCode(selectedToken) : "--- ---"}</strong>
                    <small>{t("twoFactor.roseRemaining", "剩余时间：每 30 秒自动更新")}</small>
                  </div>
                  <span className="rose-tf-large-timer" style={progressStyle}>{timeRemaining}s</span>
                </section>

                <section className="rose-tf-info-card">
                  <div className="rose-tf-section-head">
                    <h3>{t("twoFactor.roseAccountInfo", "账号信息")}</h3>
                    {activeTab === "saved" ? (
                      <button type="button" onClick={() => startEdit(selectedRecord)}>
                        <PencilIcon />
                        {t("twoFactor.roseEditInfo", "编辑信息")}
                      </button>
                    ) : null}
                  </div>
                  {editingId === selectedRecord.id ? (
                    <div className="rose-tf-detail-edit">
                      <input
                        value={editingName}
                        onChange={(event) => setEditingName(event.target.value)}
                        onBlur={saveEdit}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") saveEdit();
                        }}
                        autoFocus
                      />
                    </div>
                  ) : null}
                  <dl>
                    <div><dt>{t("twoFactor.roseProvider", "服务商")}</dt><dd>{selectedDescription.provider}</dd></div>
                    <div><dt>{t("twoFactor.roseAccount", "账号")}</dt><dd>{selectedDescription.account || t("twoFactor.unnamed")}</dd></div>
                    <div><dt>{t("twoFactor.roseSecretType", "密钥类型")}</dt><dd>TOTP</dd></div>
                    <div><dt>{t("twoFactor.roseAlgorithm", "算法")}</dt><dd>SHA-1</dd></div>
                    <div><dt>{t("twoFactor.roseDigits", "位数")}</dt><dd>{selectedToken.length || 6} {t("twoFactor.roseDigitsUnit", "位")}</dd></div>
                    <div><dt>{t("twoFactor.roseInterval", "间隔")}</dt><dd>30 {t("twoFactor.roseSeconds", "秒")}</dd></div>
                    <div><dt>{t("twoFactor.roseCreatedAt", "创建时间")}</dt><dd>{formatRecordTime(selectedRecord.time)}</dd></div>
                    <div><dt>{t("twoFactor.roseUpdatedAt", "最后更新")}</dt><dd>{formatRecordTime(selectedRecord.time)}</dd></div>
                  </dl>
                </section>
                <section className="rose-tf-secret-card">
                  <div className="rose-tf-section-head"><h3>{t("twoFactor.roseSecret", "密钥")}</h3></div>
                  <div className="rose-tf-secret-row">
                    <code>{secretRevealed ? selectedRecord.secret : "••••••••••••••••••••••••••••••••"}</code>
                    <button type="button" onClick={() => setSecretRevealed((value) => !value)} aria-label={secretRevealed ? t("twoFactor.roseHideSecret", "隐藏密钥") : t("twoFactor.roseRevealSecret", "显示密钥")}>
                      {secretRevealed ? <EyeOffIcon /> : <EyeIcon />}
                    </button>
                    <button type="button" onClick={() => void copyText(`rose-secret-${selectedRecord.id}`, selectedRecord.secret)} aria-label={t("twoFactor.roseCopySecret", "复制密钥")}>
                      {copiedId === `rose-secret-${selectedRecord.id}` ? <CheckIcon /> : <CopyIcon />}
                    </button>
                  </div>
                  <p>
                    <Icon id="icon-shield" />
                    {t("twoFactor.roseLocalOnlyShort", "密钥仅存储在本地，不会上传到任何服务器。")}
                  </p>
                </section>
                <section className="rose-tf-backup-card">
                  <h3>{t("twoFactor.roseBackupRecovery", "备份与恢复")}</h3>
                  <button type="button" onClick={() => void exportRecords()} disabled={records.length === 0}>
                    <span><Icon id="icon-download" /></span>
                    <span>
                      <strong>{t("twoFactor.roseExportBackup", "导出备份文件")}</strong>
                      <small>{t("twoFactor.roseExportBackupHint", "导出所有已保存账号，用于备份或迁移")}</small>
                    </span>
                    <span><Icon id="icon-download" /></span>
                  </button>
                  <button type="button" onClick={() => void importRecords()}>
                    <span><Icon id="icon-upload" /></span>
                    <span>
                      <strong>{t("twoFactor.roseImportBackup", "导入备份文件")}</strong>
                      <small>{t("twoFactor.roseImportBackupHint", "从 JSON 文件恢复账号与密钥")}</small>
                    </span>
                    <span><Icon id="icon-upload" /></span>
                  </button>
                </section>
              </div>
            )}
          </aside>
        </section>
      </section>
    );
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
