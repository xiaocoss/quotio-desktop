import { useState, useEffect, useRef, type ChangeEvent } from "react";
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

type Tab = "oauth" | "token" | "import";

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
  const hasNativeOAuth = Boolean(provider.native_oauth);
  const hasProxyOAuth = Boolean(provider.oauth_endpoint);
  const hasOAuth = hasNativeOAuth || hasProxyOAuth;
  const isVertex = provider.id === "vertex";
  const [tab, setTab] = useState<Tab>(hasOAuth ? "oauth" : "token");
  const nativeLoginRef = useRef<string | null>(null);
  const [isDeviceFlow, setIsDeviceFlow] = useState(false);
  const [deviceUserCode, setDeviceUserCode] = useState("");

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
      setOauthError("无法获取授权链接，请检查代理状态后重试。");
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
          setOauthError("无法获取授权状态（代理无响应），请重试。");
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
        setOauthError(res.error ?? "授权失败。");
        pollRef.current = null;
        return;
      }
    }
    setOauthStatus("error");
    setOauthError("OAuth 授权超时，请重试。");
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
          setOauthError(res.error ?? "授权失败。");
          nativeLoginRef.current = null;
          return;
        }
      } catch (err) {
        setOauthStatus("error");
        setOauthError(`授权完成失败：${String(err)}`);
        nativeLoginRef.current = null;
        return;
      }
    }
    setOauthStatus("error");
    setOauthError("OAuth 授权超时，请重试。");
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
      setOauthError("令牌交换失败，请重试。");
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
      setImportMessage("导入成功");
      setTokenInput("");
      onRefreshQuotas();
    } catch (err) {
      setImportStatus("error");
      setImportMessage(String(err) || "导入失败");
    }
    setImporting(false);
  }

  function switchTab(t: Tab) {
    pollRef.current = null;
    setTab(t);
    setImportStatus("idle");
    setImportMessage("");
  }

  return (
    <div className="modal-overlay aam-overlay" onClick={onClose}>
      <div className="aam-modal" onClick={(e) => e.stopPropagation()}>
        <div className="aam-header">
          <h2>{reauthAccountLabel ? "重新授权" : "添加账号"}</h2>
          <button className="aam-close" type="button" onClick={onClose}><XIcon /></button>
        </div>

        {reauthAccountLabel ? (
          <div className="aam-reauth-banner">
            <KeyIcon />
            <div className="aam-reauth-text">
              <span className="aam-reauth-caption">正在重新授权账号</span>
              <button
                type="button"
                className="aam-reauth-email"
                title={revealLabel ? reauthAccountLabel : "点击显示完整账号"}
                onClick={() => setRevealLabel((v) => !v)}
              >
                {revealLabel ? reauthAccountLabel : maskEmail(reauthAccountLabel)}
              </button>
            </div>
            <button
              type="button"
              className="aam-reauth-copy"
              onClick={() => void handleCopyLabel()}
              title="复制完整账号"
            >
              {labelCopied ? <CheckIcon /> : <CopyIcon />}
            </button>
          </div>
        ) : null}

        <div className="aam-tabs">
          {hasOAuth ? (
            <button className={`aam-tab${tab === "oauth" ? " aam-tab--active" : ""}`} type="button" onClick={() => switchTab("oauth")}>
              <GlobeIcon /> OAuth 授权
            </button>
          ) : null}
          <button className={`aam-tab${tab === "token" ? " aam-tab--active" : ""}`} type="button" onClick={() => switchTab("token")}>
            <KeyIcon /> Token / JSON
          </button>
          <button className={`aam-tab${tab === "import" ? " aam-tab--active" : ""}`} type="button" onClick={() => switchTab("import")}>
            <FileIcon /> 导入
          </button>
        </div>

        <div className="aam-body">
          {tab === "oauth" && (
            <div className="aam-section">
              <div className="aam-hint-row">
                <GlobeIcon />
                <span>推荐使用浏览器完成 {provider.display_name} 授权</span>
              </div>

              {oauthStatus === "error" && !oauthUrl ? (
                <div className="aam-status aam-status--error">
                  <span>{oauthError}</span>
                  <button className="aam-retry-btn" type="button" onClick={handleRetryOAuth}>
                    <RefreshIcon /> 重新生成授权信息
                  </button>
                </div>
              ) : oauthUrl ? (
                <>
                  {isDeviceFlow && deviceUserCode ? (
                    <div className="aam-device-code">
                      <p className="aam-desc">请在浏览器中打开下方链接，输入以下验证码完成授权：</p>
                      <code className="aam-user-code">{deviceUserCode}</code>
                    </div>
                  ) : null}
                  <div className="aam-url-box">
                    <input type="text" readOnly value={oauthUrl} onFocus={(e) => e.currentTarget.select()} />
                    <button type="button" onClick={handleCopyUrl} title="复制链接">
                      {urlCopied ? <CheckIcon /> : <CopyIcon />}
                    </button>
                  </div>
                  <button className="aam-primary-btn aam-primary-btn--full" type="button" onClick={() => void openAuthUrl(oauthUrl)}>
                    <GlobeIcon /> 在浏览器中打开
                  </button>
                  {oauthStatus === "polling" && (
                    <div className="aam-status aam-status--loading">
                      <SpinnerIcon />
                      <span>等待授权完成，完成后此窗口将自动更新…</span>
                    </div>
                  )}
                  {oauthStatus === "exchanging" && (
                    <div className="aam-status aam-status--loading">
                      <SpinnerIcon />
                      <span>正在交换令牌…</span>
                    </div>
                  )}
                  {oauthStatus === "success" && (
                    <div className="aam-status aam-status--success">
                      <CheckIcon />
                      <span>授权成功！账号已添加。</span>
                    </div>
                  )}
                  {oauthStatus === "error" && (
                    <div className="aam-status aam-status--error">
                      <span>{oauthError}</span>
                      <button className="aam-retry-btn" type="button" onClick={handleRetryOAuth}>
                        <RefreshIcon /> 刷新授权链接
                      </button>
                    </div>
                  )}
                  <label className="aam-label">手动输入回调地址</label>
                  <div className="aam-url-box">
                    <input
                      type="text"
                      placeholder="粘贴完整回调地址，例如: http://localhost:1455/auth/callback?code=...&state=..."
                      value={manualCallback}
                      onChange={(e) => setManualCallback(e.target.value)}
                    />
                    <button className="aam-callback-btn" type="button" onClick={() => void handleManualCallback()} disabled={callbackBusy || !manualCallback.trim()}>
                      {callbackBusy ? <SpinnerIcon /> : <CheckIcon />}
                      <span>提交</span>
                    </button>
                  </div>
                  <p className="aam-hint">完成授权后此窗口将自动更新</p>
                </>
              ) : (
                <div className="aam-oauth-loading">
                  <SpinnerIcon />
                  <span>正在准备授权信息…</span>
                </div>
              )}
            </div>
          )}

          {tab === "token" && (
            <div className="aam-section">
              <p className="aam-desc">
                {isVertex
                  ? "粘贴 Vertex AI Service Account JSON 凭据。"
                  : `粘贴 ${provider.display_name} 的 Token 或 JSON 凭据内容。`}
              </p>
              <textarea
                className="aam-token-input"
                value={tokenInput}
                onChange={(e) => setTokenInput(e.target.value)}
                placeholder={isVertex ? '{"type":"service_account",...}' : "粘贴 Token 或 JSON…"}
                rows={6}
              />
              <button className="aam-primary-btn" type="button" onClick={() => void handleTokenImport()} disabled={importing || !tokenInput.trim()}>
                {importing ? <SpinnerIcon /> : <PlusIcon />}
                导入
              </button>
            </div>
          )}

          {tab === "import" && (
            <div className="aam-section">
              <p className="aam-desc">从本地 JSON 文件导入认证凭据。</p>
              <input ref={fileRef} type="file" accept=".json,application/json" multiple hidden onChange={(e) => { onImportFile(e); setImportStatus("success"); setImportMessage("文件已导入"); }} />
              <button className="aam-primary-btn" type="button" onClick={() => fileRef.current?.click()} disabled={importing}>
                <FileIcon /> 选择 JSON 文件导入
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
