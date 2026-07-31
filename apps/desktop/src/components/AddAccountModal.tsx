import { useState, useEffect, useRef, type ChangeEvent } from "react";
import { useT } from "../i18n";
import { Mfa2faQuickPanel } from "./Mfa2faQuickPanel";
import type { NativeOAuthCompleteResponse, NativeOAuthStartResponse, OAuthStatusResponse, OAuthUrlResponse, ProviderSummary } from "../types";
import { CheckIcon, CopyIcon, KeyIcon, PlusIcon, RefreshIcon } from "./icons";
import { invoke } from "../lib/tauri";
import { maskEmail } from "../lib/format";

function GlobeIcon() {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="8" cy="8" r="6.5" />
      <path d="M2 8h12M8 1.5c1.8 2 2.8 4 2.8 6.5S9.8 12.5 8 14.5M8 1.5C6.2 3.5 5.2 5.5 5.2 8s1 4.5 2.8 6.5" />
    </svg>
  );
}

function FileIcon() {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M9 1.5H4.5a1.5 1.5 0 0 0-1.5 1.5v10a1.5 1.5 0 0 0 1.5 1.5h7a1.5 1.5 0 0 0 1.5-1.5V5.5L9 1.5z" />
      <path d="M9 1.5v4h4" />
    </svg>
  );
}

function XIcon() {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
      <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" />
    </svg>
  );
}

function SpinnerIcon() {
  return (
    <svg className="aam-spinner" viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
      <path d="M8 1.5A6.5 6.5 0 1 1 1.5 8" />
    </svg>
  );
}

type Tab = "oauth" | "token" | "import" | "org";

/** kiro_idc_start 返回(camelCase,见 quotio-core::kiro_idc::KiroIdcStartResponse)。 */
type KiroIdcStart = {
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
  loginOption: string;
};
/** kiro_idc_poll 返回。status: "pending" | "success" | "error"。 */
type KiroIdcPoll = { status: string; error: string | null };

type AddAccountModalProps = {
  provider: ProviderSummary;
  projectId: string;
  /** 非空 = 这是「重新授权」某个已存在账号:弹窗会显示该账号供用户对照 / 复制。 */
  reauthAccountLabel?: string | null;
  onClose: () => void;
  onStartOAuth: (endpoint: string, projectId: string | null, isWebui?: boolean) => Promise<OAuthUrlResponse | null>;
  onPollOAuth: (token: string) => Promise<OAuthStatusResponse | null>;
  onRefreshQuotas: () => void;
  onImportFile: (e: ChangeEvent<HTMLInputElement>) => void;
};

export function AddAccountModal({
  provider,
  projectId,
  reauthAccountLabel,
  onClose,
  onStartOAuth,
  onPollOAuth,
  onRefreshQuotas,
  onImportFile,
}: AddAccountModalProps) {
  const t = useT();
  const hasNativeOAuth = Boolean(provider.native_oauth);
  const hasProxyOAuth = Boolean(provider.oauth_endpoint);
  const hasOAuth = hasNativeOAuth || hasProxyOAuth;
  const isVertex = provider.id === "vertex";
  const isKiro = provider.id === "kiro";
  const [tab, setTab] = useState<Tab>(hasOAuth ? "oauth" : "token");
  const nativeLoginRef = useRef<string | null>(null);
  const [isDeviceFlow, setIsDeviceFlow] = useState(false);
  const [deviceUserCode, setDeviceUserCode] = useState("");

  // Kiro 组织(IAM Identity Center / awsidc)/ 个人(Builder ID)AWS SSO 设备流登录
  const [idcMode, setIdcMode] = useState<"builderid" | "awsidc">("builderid");
  const [idcStartUrl, setIdcStartUrl] = useState("");
  const [idcRegion, setIdcRegion] = useState("us-east-1");
  const [idcStatus, setIdcStatus] = useState<"idle" | "starting" | "waiting" | "success" | "error">("idle");
  const [idcUserCode, setIdcUserCode] = useState("");
  const [idcVerifyUri, setIdcVerifyUri] = useState("");
  const [idcError, setIdcError] = useState<string | null>(null);
  const idcActiveRef = useRef(false);

  // OAuth state
  const [oauthUrl, setOauthUrl] = useState<string | null>(null);
  const [oauthStatus, setOauthStatus] = useState<"idle" | "preparing" | "ready" | "polling" | "exchanging" | "success" | "error">("idle");
  const [oauthError, setOauthError] = useState<string | null>(null);
  const [urlCopied, setUrlCopied] = useState(false);
  const pollRef = useRef<string | null>(null);

  // Token/JSON state
  const [tokenInput, setTokenInput] = useState("");
  const [importing, setImporting] = useState(false);
  const [importStatus, setImportStatus] = useState<"idle" | "success" | "error">("idle");
  const [importMessage, setImportMessage] = useState("");

  // Manual callback state
  const [manualCallback, setManualCallback] = useState("");
  const [callbackBusy, setCallbackBusy] = useState(false);

  // 重新授权横幅:展示 / 复制正在重授的账号
  const [labelCopied, setLabelCopied] = useState(false);
  const [revealLabel, setRevealLabel] = useState(false);

  // File input ref
  const fileRef = useRef<HTMLInputElement>(null);

  // Auto-prepare OAuth when tab opens
  useEffect(() => {
    if (tab === "oauth" && hasOAuth && oauthStatus === "idle") {
      void prepareOAuth();
    }
    return () => { pollRef.current = null; nativeLoginRef.current = null; };
  }, [tab]);

  // Auto-close modal on success after 1200ms
  useEffect(() => {
    if (oauthStatus !== "success") return;
    const timer = setTimeout(onClose, 1200);
    return () => clearTimeout(timer);
  }, [oauthStatus, onClose]);

  // Kiro 设备流成功后同样自动关闭
  useEffect(() => {
    if (idcStatus !== "success") return;
    const timer = setTimeout(onClose, 1200);
    return () => clearTimeout(timer);
  }, [idcStatus, onClose]);

  // 卸载时停掉进行中的设备流轮询
  useEffect(() => () => { idcActiveRef.current = false; }, []);

  async function openAuthUrl(url: string) {
    try {
      if ("__TAURI_INTERNALS__" in window) {
        const { openUrl } = await import("@tauri-apps/plugin-opener");
        await openUrl(url);
        return;
      }
    } catch { /* fallback */ }
    window.open(url, "_blank", "noreferrer");
  }

  async function prepareOAuth() {
    setOauthStatus("preparing");
    setOauthError(null);

    if (hasNativeOAuth) {
      try {
        const res = await invoke<NativeOAuthStartResponse>("native_oauth_start", { providerId: provider.id });
        nativeLoginRef.current = res.login_id;
        setOauthUrl(res.auth_url);
        setOauthStatus("ready");
        if (res.flow === "device_code") {
          setIsDeviceFlow(true);
          setDeviceUserCode(res.user_code);
        }
        void startNativePolling(res.login_id);
      } catch (err) {
        setOauthStatus("error");
        setOauthError(String(err));
      }
      return;
    }

    const response = await onStartOAuth(provider.oauth_endpoint ?? "", projectId, true);
    if (!response) {
      setOauthStatus("error");
      setOauthError(t("hardcoded.w002"));
      return;
    }
    if (response.error) {
      setOauthStatus("error");
      setOauthError(response.error);
      return;
    }
    setOauthUrl(response.url);
    setOauthStatus("ready");
    if (response.state) void startPolling(response.state);
  }

  async function startPolling(state: string) {
    pollRef.current = state;
    setOauthStatus("polling");
    let consecutiveFailures = 0;
    for (let attempt = 0; attempt < 60; attempt++) {
      if (attempt > 0) await new Promise((r) => setTimeout(r, 2000));
      if (pollRef.current !== state) return;
      const res = await onPollOAuth(state);
      if (pollRef.current !== state) return;
      if (!res) {
        consecutiveFailures++;
        if (consecutiveFailures >= 3) {
          setOauthStatus("error");
          setOauthError(t("hardcoded.w003"));
          pollRef.current = null;
          return;
        }
        continue;
      }
      consecutiveFailures = 0;
      if (["ok", "success", "completed"].includes(res.status)) {
        setOauthStatus("success");
        pollRef.current = null;
        onRefreshQuotas();
        return;
      }
      if (res.status === "error") {
        setOauthStatus("error");
        setOauthError(res.error ?? t("hardcoded.w004"));
        pollRef.current = null;
        return;
      }
    }
    setOauthStatus("error");
    setOauthError(t("hardcoded.w005"));
    pollRef.current = null;
  }

  async function startNativePolling(loginId: string) {
    nativeLoginRef.current = loginId;
    setOauthStatus("polling");
    for (let attempt = 0; attempt < 150; attempt++) {
      if (attempt > 0) await new Promise((r) => setTimeout(r, 2000));
      if (nativeLoginRef.current !== loginId) return;
      try {
        const res = await invoke<NativeOAuthCompleteResponse>("native_oauth_complete", { loginId });
        if (nativeLoginRef.current !== loginId) return;
        if (res.status === "success") {
          setOauthStatus("success");
          nativeLoginRef.current = null;
          onRefreshQuotas();
          return;
        }
        if (res.status === "error") {
          setOauthStatus("error");
          setOauthError(res.error ?? t("hardcoded.w004"));
          nativeLoginRef.current = null;
          return;
        }
      } catch (err) {
        setOauthStatus("error");
        setOauthError(`${t("hardcoded.w004")} ${String(err)}`);
        nativeLoginRef.current = null;
        return;
      }
    }
    setOauthStatus("error");
    setOauthError(t("hardcoded.w005"));
    nativeLoginRef.current = null;
  }

  function handleRetryOAuth() {
    pollRef.current = null;
    nativeLoginRef.current = null;
    setOauthUrl(null);
    setOauthStatus("idle");
    setOauthError(null);
    setIsDeviceFlow(false);
    setDeviceUserCode("");
    if (hasNativeOAuth) {
      invoke("native_oauth_cancel", { loginId: null }).catch(() => {});
    }
    void prepareOAuth();
  }

  async function handleCopyUrl() {
    if (!oauthUrl) return;
    try {
      await navigator.clipboard.writeText(oauthUrl);
      setUrlCopied(true);
      setTimeout(() => setUrlCopied(false), 1200);
    } catch { /* ignore */ }
  }

  async function handleCopyLabel() {
    if (!reauthAccountLabel) return;
    try {
      await navigator.clipboard.writeText(reauthAccountLabel);
      setLabelCopied(true);
      setTimeout(() => setLabelCopied(false), 1200);
    } catch { /* ignore */ }
  }

  async function handleManualCallback() {
    const url = manualCallback.trim();
    if (!url) return;
    setCallbackBusy(true);
    setOauthStatus("exchanging");
    try {
      if (hasNativeOAuth && nativeLoginRef.current) {
        await invoke("native_oauth_submit_callback", { loginId: nativeLoginRef.current, callbackUrl: url });
      } else {
        await invoke("submit_oauth_callback", { url });
      }
      setManualCallback("");
    } catch {
      setOauthStatus("error");
      setOauthError(t("hardcoded.w006"));
    }
    setCallbackBusy(false);
  }

  async function handleTokenImport() {
    const value = tokenInput.trim();
    if (!value) return;
    setImporting(true);
    setImportStatus("idle");
    try {
      if (isVertex) {
        JSON.parse(value);
      }
      await invoke("import_auth_token", { providerId: provider.id, content: value });
      setImportStatus("success");
      setImportMessage(t("hardcoded.w007"));
      setTokenInput("");
      onRefreshQuotas();
    } catch (err) {
      setImportStatus("error");
      setImportMessage(String(err) || t("hardcoded.w008"));
    }
    setImporting(false);
  }

  // ---- Kiro AWS SSO 设备流 ----
  async function startIdcLogin() {
    setIdcStatus("starting");
    setIdcError(null);
    setIdcUserCode("");
    setIdcVerifyUri("");
    try {
      const res = await invoke<KiroIdcStart>("kiro_idc_start", {
        loginOption: idcMode,
        startUrl: idcMode === "awsidc" ? idcStartUrl.trim() : null,
        region: idcRegion.trim() || null,
      });
      setIdcUserCode(res.userCode);
      setIdcVerifyUri(res.verificationUri);
      setIdcStatus("waiting");
      void openAuthUrl(res.verificationUri);
      idcActiveRef.current = true;
      void pollIdcLogin(Math.max(1, res.interval), res.expiresIn);
    } catch (err) {
      setIdcStatus("error");
      setIdcError(String(err));
    }
  }

  async function pollIdcLogin(intervalSec: number, expiresIn: number) {
    const deadline = Date.now() + Math.max(30, expiresIn) * 1000;
    let transientFailures = 0;
    while (idcActiveRef.current && Date.now() < deadline) {
      await new Promise((r) => setTimeout(r, intervalSec * 1000));
      if (!idcActiveRef.current) return;
      let res: KiroIdcPoll;
      try {
        res = await invoke<KiroIdcPoll>("kiro_idc_poll");
      } catch (err) {
        // 命令层异常(极少):容忍几次再放弃。
        if (++transientFailures >= 3) {
          setIdcStatus("error");
          setIdcError(`${t("hardcoded.w004")} ${String(err)}`);
          idcActiveRef.current = false;
          return;
        }
        continue;
      }
      if (!idcActiveRef.current) return;
      transientFailures = 0;
      if (res.status === "success") {
        setIdcStatus("success");
        idcActiveRef.current = false;
        onRefreshQuotas();
        return;
      }
      if (res.status === "error") {
        setIdcStatus("error");
        setIdcError(res.error ?? t("hardcoded.w004"));
        idcActiveRef.current = false;
        return;
      }
      // "pending" → 继续轮询
    }
    if (idcActiveRef.current) {
      idcActiveRef.current = false;
      setIdcStatus("error");
      setIdcError(t("hardcoded.w009"));
    }
  }

  function cancelIdc() {
    idcActiveRef.current = false;
    invoke("kiro_idc_cancel").catch(() => {});
  }

  function retryIdc() {
    cancelIdc();
    setIdcStatus("idle");
    setIdcError(null);
    setIdcUserCode("");
    setIdcVerifyUri("");
  }

  function switchTab(t: Tab) {
    pollRef.current = null;
    if (t !== "org" && idcActiveRef.current) cancelIdc();
    setTab(t);
    setImportStatus("idle");
    setImportMessage("");
  }

  return (
    <div className="modal-overlay aam-overlay" onClick={onClose}>
      <div className="aam-modal" onClick={(e) => e.stopPropagation()}>
        <div className="aam-header">
          <h2>{reauthAccountLabel ? t("providers.reauth") : t("import.button")}</h2>
          <button className="aam-close" type="button" onClick={onClose}><XIcon /></button>
        </div>

        {reauthAccountLabel ? (
          <div className="aam-reauth-banner">
            <KeyIcon />
            <div className="aam-reauth-text">
              <span className="aam-reauth-caption">{t("hardcoded.w010")}</span>
              <button
                type="button"
                className="aam-reauth-email"
                title={revealLabel ? reauthAccountLabel : t("hardcoded.w011")}
                onClick={() => setRevealLabel((v) => !v)}
              >
                {revealLabel ? reauthAccountLabel : maskEmail(reauthAccountLabel)}
              </button>
            </div>
            <button
              type="button"
              className="aam-reauth-copy"
              onClick={() => void handleCopyLabel()}
              title={t("hardcoded.w012")}
            >
              {labelCopied ? <CheckIcon /> : <CopyIcon />}
            </button>
          </div>
        ) : null}

        <div className="aam-tabs">
          {hasOAuth ? (
            <button className={`aam-tab${tab === "oauth" ? " aam-tab--active" : ""}`} type="button" onClick={() => switchTab("oauth")}>
              <GlobeIcon /> {t("addAccount.oauth")}
            </button>
          ) : null}
          {isKiro ? (
            <button className={`aam-tab${tab === "org" ? " aam-tab--active" : ""}`} type="button" onClick={() => switchTab("org")}>
              <KeyIcon /> {t("addAccount.organization")}
            </button>
          ) : null}
          <button className={`aam-tab${tab === "token" ? " aam-tab--active" : ""}`} type="button" onClick={() => switchTab("token")}>
            <KeyIcon /> Token / JSON
          </button>
          <button className={`aam-tab${tab === "import" ? " aam-tab--active" : ""}`} type="button" onClick={() => switchTab("import")}>
            <FileIcon /> {t("twoFactor.import")}
          </button>
        </div>

        <div className="aam-body">
          {tab === "oauth" && (
            <div className="aam-section">
              <div className="aam-hint-row">
                <GlobeIcon />
                <span>{t("addAccount.browserRecommended").replace("{provider}", provider.display_name)}</span>
              </div>

              {oauthStatus === "error" && !oauthUrl ? (
                <div className="aam-status aam-status--error">
                  <span>{oauthError}</span>
                  <button className="aam-retry-btn" type="button" onClick={handleRetryOAuth}>
                    <RefreshIcon /> {t("addAccount.regenerate")}
                  </button>
                </div>
              ) : oauthUrl ? (
                <>
                  {isDeviceFlow && deviceUserCode ? (
                    <div className="aam-device-code">
                      <p className="aam-desc">{t("hardcoded.w013")}</p>
                      <code className="aam-user-code">{deviceUserCode}</code>
                    </div>
                  ) : null}
                  <div className="aam-url-box">
                    <input type="text" readOnly value={oauthUrl} onFocus={(e) => e.currentTarget.select()} />
                    <button type="button" onClick={handleCopyUrl} title={t("hardcoded.w014")}>
                      {urlCopied ? <CheckIcon /> : <CopyIcon />}
                    </button>
                  </div>
                  <button className="aam-primary-btn aam-primary-btn--full" type="button" onClick={() => void openAuthUrl(oauthUrl)}>
                    <GlobeIcon /> {t("addAccount.openBrowser")}
                  </button>
                  {oauthStatus === "polling" && (
                    <div className="aam-status aam-status--loading">
                      <SpinnerIcon />
                      <span>{t("hardcoded.w015")}</span>
                    </div>
                  )}
                  {oauthStatus === "exchanging" && (
                    <div className="aam-status aam-status--loading">
                      <SpinnerIcon />
                      <span>{t("hardcoded.w016")}</span>
                    </div>
                  )}
                  {oauthStatus === "success" && (
                    <div className="aam-status aam-status--success">
                      <CheckIcon />
                      <span>{t("hardcoded.w017")}</span>
                    </div>
                  )}
                  {oauthStatus === "error" && (
                    <div className="aam-status aam-status--error">
                      <span>{oauthError}</span>
                      <button className="aam-retry-btn" type="button" onClick={handleRetryOAuth}>
                        <RefreshIcon /> {t("addAccount.regenerate")}
                      </button>
                    </div>
                  )}
                  <label className="aam-label">{t("hardcoded.w018")}</label>
                  <div className="aam-url-box">
                    <input
                      type="text"
                      placeholder={t("hardcoded.w019")}
                      value={manualCallback}
                      onChange={(e) => setManualCallback(e.target.value)}
                    />
                    <button className="aam-callback-btn" type="button" onClick={() => void handleManualCallback()} disabled={callbackBusy || !manualCallback.trim()}>
                      {callbackBusy ? <SpinnerIcon /> : <CheckIcon />}
                      <span>{t("hardcoded.w020")}</span>
                    </button>
                  </div>
                  <p className="aam-hint">{t("hardcoded.w021")}</p>
                </>
              ) : (
                <div className="aam-oauth-loading">
                  <SpinnerIcon />
                  <span>{t("hardcoded.w022")}</span>
                </div>
              )}
            </div>
          )}

          {tab === "org" && (
            <div className="aam-section">
              <div className="aam-hint-row">
                <GlobeIcon />
                <span>{t("hardcoded.w023")}</span>
              </div>

              {(idcStatus === "idle" || idcStatus === "starting") && (
                <>
                  <div className="aam-idc-mode">
                    <button
                      type="button"
                      className={`aam-idc-opt${idcMode === "builderid" ? " aam-idc-opt--active" : ""}`}
                      onClick={() => setIdcMode("builderid")}
                    >
                      {t("addAccount.personalBuilder")}
                    </button>
                    <button
                      type="button"
                      className={`aam-idc-opt${idcMode === "awsidc" ? " aam-idc-opt--active" : ""}`}
                      onClick={() => setIdcMode("awsidc")}
                    >
                      {t("addAccount.organizationIam")}
                    </button>
                  </div>

                  {idcMode === "awsidc" && (
                    <div>
                      <label className="aam-label">{t("hardcoded.w024")}</label>
                      <input
                        className="aam-text-input"
                        type="text"
                        placeholder="https://d-xxxxxxxxxx.awsapps.com/start"
                        value={idcStartUrl}
                        onChange={(e) => setIdcStartUrl(e.target.value)}
                      />
                    </div>
                  )}

                  <div>
                    <label className="aam-label">{t("hardcoded.w025")}</label>
                    <input
                      className="aam-text-input"
                      type="text"
                      placeholder="us-east-1"
                      value={idcRegion}
                      onChange={(e) => setIdcRegion(e.target.value)}
                    />
                  </div>

                  <button
                    className="aam-primary-btn"
                    type="button"
                    onClick={() => void startIdcLogin()}
                    disabled={idcStatus === "starting" || (idcMode === "awsidc" && !idcStartUrl.trim())}
                  >
                    {idcStatus === "starting" ? <SpinnerIcon /> : <GlobeIcon />}
                    {t("addAccount.startLogin")}
                  </button>
                  <p className="aam-hint">
                    {t("addAccount.startLoginHint")}
                  </p>
                </>
              )}

              {idcStatus === "waiting" && (
                <>
                  <div className="aam-device-code">
                    <p className="aam-desc">{t("hardcoded.w026")}</p>
                    <code className="aam-user-code">{idcUserCode}</code>
                  </div>
                  <div className="aam-url-box">
                    <input type="text" readOnly value={idcVerifyUri} onFocus={(e) => e.currentTarget.select()} />
                    <button type="button" onClick={() => void openAuthUrl(idcVerifyUri)} title={t("hardcoded.w027")}>
                      <GlobeIcon />
                    </button>
                  </div>
                  <div className="aam-status aam-status--loading">
                    <SpinnerIcon />
                    <span>{t("hardcoded.w028")}</span>
                  </div>
                  <button className="aam-retry-btn" type="button" onClick={retryIdc}>
                    <RefreshIcon /> {t("addAccount.cancelRestart")}
                  </button>
                </>
              )}

              {idcStatus === "success" && (
                <div className="aam-status aam-status--success">
                  <CheckIcon />
                  <span>{t("hardcoded.w029")}</span>
                </div>
              )}

              {idcStatus === "error" && (
                <div className="aam-status aam-status--error">
                  <span>{idcError}</span>
                  <button className="aam-retry-btn" type="button" onClick={retryIdc}>
                    <RefreshIcon /> {t("addAccount.restart")}
                  </button>
                </div>
              )}
            </div>
          )}

          {tab === "token" && (
            <div className="aam-section">
              <p className="aam-desc">
                {isVertex
                  ? t("hardcoded.w030")
                  : `${t("hardcoded.w031")} (${provider.display_name})`}
              </p>
              <textarea
                className="aam-token-input"
                value={tokenInput}
                onChange={(e) => setTokenInput(e.target.value)}
                placeholder={isVertex ? '{"type":"service_account",...}' : t("hardcoded.w031")}
                rows={6}
              />
              <button className="aam-primary-btn" type="button" onClick={() => void handleTokenImport()} disabled={importing || !tokenInput.trim()}>
                {importing ? <SpinnerIcon /> : <PlusIcon />}
                {t("twoFactor.import")}
              </button>
            </div>
          )}

          {tab === "import" && (
            <div className="aam-section">
              <p className="aam-desc">{t("hardcoded.w032")}</p>
              <input ref={fileRef} type="file" accept=".json,application/json" multiple hidden onChange={(e) => { onImportFile(e); setImportStatus("success"); setImportMessage(t("hardcoded.w033")); }} />
              <button className="aam-primary-btn" type="button" onClick={() => fileRef.current?.click()} disabled={importing}>
                <FileIcon /> {t("addAccount.importJson")}
              </button>
            </div>
          )}

          {importStatus !== "idle" ? (
            <div className={`aam-status aam-status--${importStatus}`}>
              {importStatus === "success" ? <CheckIcon /> : null}
              <span>{importMessage}</span>
            </div>
          ) : null}

          <Mfa2faQuickPanel />
        </div>
      </div>
    </div>
  );
}
