import { useEffect, useState } from "react";
import { useT } from "../i18n";
import { loadSavedMfaRecords, getMfaOtpToken, getMfaTimeRemaining, type MfaRecord } from "../lib/mfaVault";
import { maskEmail } from "../lib/format";

// 「添加账号」弹窗内的 2FA 验证码就近取用面板。OAuth 登录时浏览器要 2FA,但弹窗是
// 全屏遮罩、且 OAuth 轮询状态在弹窗里(切走会断流程),所以把 2FA 搬进来:展开即可
// 看到已保存条目的实时验证码并一键复制,复用 2FA 金库(同一份 localStorage + 算码)。
export function Mfa2faQuickPanel() {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [records, setRecords] = useState<MfaRecord[]>([]);
  const [, setTick] = useState(0); // 每秒触发重渲染:刷新验证码 + 倒计时
  const [copiedId, setCopiedId] = useState<string | null>(null);

  // 展开时才加载 + 起秒表,收起就停,不白跑。
  useEffect(() => {
    if (!open) return;
    setRecords(loadSavedMfaRecords());
    const timer = window.setInterval(() => setTick((t) => t + 1), 1000);
    return () => window.clearInterval(timer);
  }, [open]);

  async function copyCode(record: MfaRecord) {
    const code = getMfaOtpToken(record.secret);
    if (!code) return;
    try {
      await navigator.clipboard.writeText(code);
      setCopiedId(record.id);
      window.setTimeout(() => setCopiedId((id) => (id === record.id ? null : id)), 1500);
    } catch {
      /* clipboard 不可用时静默 */
    }
  }

  const remaining = getMfaTimeRemaining();

  return (
    <div className="aam-mfa">
      <button className="aam-mfa-toggle" type="button" onClick={() => setOpen((v) => !v)}>
        <span>{t("hardcoded.w063")}</span>
        <span className="aam-mfa-hint">{t("hardcoded.w064")}</span>
        <span className="aam-mfa-caret">{open ? "▲" : "▼"}</span>
      </button>
      {open ? (
        records.length === 0 ? (
          <p className="aam-mfa-empty">{t("hardcoded.w065")}</p>
        ) : (
          <ul className="aam-mfa-list">
            {records.map((record) => {
              const code = getMfaOtpToken(record.secret);
              return (
                <li key={record.id} className="aam-mfa-item">
                  <span className="aam-mfa-name" title={record.accountName}>
                    {maskEmail(record.accountName || t("hardcoded.w066"))}
                  </span>
                  <code className="aam-mfa-code">{code || "------"}</code>
                  <span className="aam-mfa-countdown" title={t("hardcoded.w067")}>{remaining}s</span>
                  <button className="aam-mfa-copy" type="button" onClick={() => void copyCode(record)}>
                    {copiedId === record.id ? t("twoFactor.copied") : t("common.copy")}
                  </button>
                </li>
              );
            })}
          </ul>
        )
      ) : null}
    </div>
  );
}
