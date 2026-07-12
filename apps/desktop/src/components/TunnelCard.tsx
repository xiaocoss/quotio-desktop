import { useEffect, useState, type ChangeEvent } from "react";
import { invoke } from "../lib/tauri";
import { useT } from "../i18n";
import { Switch } from "./Switch";

type TunnelStatus = {
  running: boolean;
  public_url: string | null;
  has_binary: boolean;
};

/// Self-contained cloudflared "quick tunnel" control: download the binary,
/// start/stop the tunnel, and surface the public https://*.trycloudflare.com URL.
/// Only renders inside the Tauri app (the browser mock has no tunnel backend).
export function TunnelCard() {
  const t = useT();
  const isTauri = "__TAURI_INTERNALS__" in window;
  const [status, setStatus] = useState<TunnelStatus | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!isTauri) return;
    void invoke<TunnelStatus>("tunnel_status").then(setStatus).catch(() => {});
    let unlistenProgress: (() => void) | undefined;
    let unlistenUrl: (() => void) | undefined;
    void import("@tauri-apps/api/event").then(({ listen }) => {
      void listen<number>("cloudflared-download-progress", (event) => {
        setProgress(event.payload >= 100 ? null : event.payload);
      }).then((fn) => {
        unlistenProgress = fn;
      });
      void listen<string>("tunnel-url", (event) => {
        setStatus((prev) =>
          prev
            ? { ...prev, public_url: event.payload, running: true }
            : { running: true, public_url: event.payload, has_binary: true },
        );
      }).then((fn) => {
        unlistenUrl = fn;
      });
    });
    return () => {
      unlistenProgress?.();
      unlistenUrl?.();
    };
  }, [isTauri]);

  async function run(command: string) {
    setBusy(true);
    if (command === "download_cloudflared") setProgress(0);
    try {
      setStatus(await invoke<TunnelStatus>(command));
    } catch {
      /* surfaced elsewhere; keep the card responsive */
    } finally {
      setBusy(false);
      if (command === "download_cloudflared") setProgress(null);
    }
  }

  async function copyUrl() {
    if (!status?.public_url) return;
    try {
      await navigator.clipboard.writeText(status.public_url);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch {
      setCopied(false);
    }
  }

  if (!isTauri) return null;

  return (
    <article className="sr-tool-card">
      <span className="sr-tool-icon" aria-hidden="true">
        <svg className="sr-icon">
          <use href="/settings/settings-icons.svg#cloud" />
        </svg>
      </span>
      <div className="sr-tool-text">
        <strong>{t("tunnel.title")}</strong>
        <small className={status?.running && status.public_url ? "tunnel-url" : undefined} title={status?.running && status.public_url ? status.public_url : undefined}>
          {status?.running ? status.public_url ?? t("tunnel.detecting") : t("tunnel.desc")}
        </small>
      </div>
      <div className="sr-tool-actions">
        {status?.running && status.public_url ? (
          <button className="sr-mini-btn" type="button" onClick={() => void copyUrl()}>
            {copied ? t("tunnel.copied") : t("tunnel.copy")}
          </button>
        ) : null}
        {status && !status.has_binary ? (
          <button className="sr-mini-btn" type="button" onClick={() => void run("download_cloudflared")} disabled={busy}>
            {busy && progress != null ? `${t("tunnel.downloading")} ${progress}%` : t("tunnel.download")}
          </button>
        ) : status?.running ? (
          <button className="sr-mini-btn sr-mini-btn--danger" type="button" onClick={() => void run("stop_tunnel")} disabled={busy}>
            {t("tunnel.stop")}
          </button>
        ) : (
          <button className="sr-mini-btn sr-mini-btn--primary" type="button" onClick={() => void run("start_tunnel")} disabled={busy}>
            {t("tunnel.start")}
          </button>
        )}
      </div>
    </article>
  );
}

/// Minimal account warmup control: sends a 1-token request through each
/// Antigravity account (via the proxy management `api-call`) to keep it active.
export function WarmupCard() {
  const t = useT();
  const isTauri = "__TAURI_INTERNALS__" in window;
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);

  if (!isTauri) return null;

  async function warmup() {
    setBusy(true);
    setResult(null);
    try {
      const count = await invoke<number>("warmup_accounts");
      setResult(t("warmup.done").replace("{n}", String(count)));
    } catch (error) {
      setResult(String(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <article className="sr-tool-card sr-tool-card--warm">
      <span className="sr-tool-icon" aria-hidden="true">
        <svg className="sr-icon">
          <use href="/settings/settings-icons.svg#rocket" />
        </svg>
      </span>
      <div className="sr-tool-text">
        <strong>{t("warmup.antigravity")}</strong>
        <small title={result ?? undefined}>{result ?? t("warmup.desc")}</small>
      </div>
      <div className="sr-tool-actions">
        <button className="sr-mini-btn sr-mini-btn--primary" type="button" onClick={() => void warmup()} disabled={busy}>
          {busy ? t("warmup.running") : t("warmup.button")}
        </button>
      </div>
    </article>
  );
}

/// Launch-at-login toggle backed by the autostart plugin.
export function AutostartCard() {
  const t = useT();
  const isTauri = "__TAURI_INTERNALS__" in window;
  const [enabled, setEnabled] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!isTauri) return;
    void invoke<boolean>("get_autostart").then(setEnabled).catch(() => {});
  }, [isTauri]);

  if (!isTauri) return null;

  async function toggle() {
    setBusy(true);
    try {
      setEnabled(await invoke<boolean>("set_autostart", { enabled: !enabled }));
    } catch {
      /* ignore */
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="settings-group">
      <h2 className="settings-group-title">{t("autostart.title")}</h2>
      <div className="settings-card">
        <div className="settings-row">
          <div className="settings-row-text">
            <strong>{t("autostart.label")}</strong>
            <small>{t("autostart.desc")}</small>
          </div>
          <Switch on={enabled} disabled={busy} onChange={() => void toggle()} label="autostart" />
        </div>
      </div>
    </div>
  );
}

/// Import CLIProxyAPI account JSON files into the auth directory.
export function ImportAuthCard() {
  const t = useT();
  const isTauri = "__TAURI_INTERNALS__" in window;
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);

  if (!isTauri) return null;

  async function onFiles(event: ChangeEvent<HTMLInputElement>) {
    const files = Array.from(event.target.files ?? []);
    event.target.value = "";
    if (files.length === 0) return;
    setBusy(true);
    setResult(null);
    let imported = 0;
    for (const file of files) {
      try {
        const content = await file.text();
        await invoke("import_auth_file", { filename: file.name, content });
        imported += 1;
      } catch {
        /* skip invalid files */
      }
    }
    setBusy(false);
    setResult(t("import.done").replace("{n}", String(imported)));
  }

  return (
    <div className="settings-group">
      <h2 className="settings-group-title">{t("import.title")}</h2>
      <div className="settings-card">
        <div className="settings-row">
          <div className="settings-row-text">
            <strong>{t("import.label")}</strong>
            <small>{result ?? t("import.desc")}</small>
          </div>
          <label className={busy ? "secondary-action import-auth-btn import-auth-btn--busy" : "secondary-action import-auth-btn"}>
            {busy ? t("import.importing") : t("import.button")}
            <input type="file" accept=".json,application/json" multiple hidden disabled={busy} onChange={onFiles} />
          </label>
        </div>
      </div>
    </div>
  );
}
