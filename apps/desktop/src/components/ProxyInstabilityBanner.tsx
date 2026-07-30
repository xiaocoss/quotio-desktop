import { useEffect, useState } from "react";
import type { AppState } from "../types";
import { detectProxyInstability } from "../lib/proxyInstability";
import { useT } from "../i18n";

// A dismissible warning shown when several recent requests failed at the
// upstream-proxy layer (connection reset / 5xx) while an upstream proxy is
// configured — the classic "your 科学上网 node is unstable, switch it" case.
// The raw failure surfaces in the LLM client as a Go `wsarecv: ... forcibly
// closed` 500; this turns the pattern into one actionable Chinese hint.
export function ProxyInstabilityBanner({ appState }: { appState: AppState | null }) {
  const t = useT();
  const instability = detectProxyInstability(appState);
  // Hide after the user dismisses, until a *new* failure pushes the count higher
  // (or it recovers and a fresh burst starts, which resets the marker to 0).
  const [dismissedAt, setDismissedAt] = useState(0);

  // Reset the dismissal once the upstream recovers (no instability), so a later
  // burst re-surfaces the banner. Keyed on a stable boolean, not the per-render
  // instability object, so the effect only fires when the state actually flips.
  const active = instability !== null;
  useEffect(() => {
    if (!active) setDismissedAt(0);
  }, [active]);

  if (!instability || instability.failureCount <= dismissedAt) return null;

  const body = t("banner.proxyUnstable.body")
    .replace("{count}", String(instability.failureCount))
    .replace("{proxy}", instability.proxyUrl);

  return (
    <div className="proxy-instability-banner" role="status">
      <div className="proxy-instability-text">
        <strong>{t("banner.proxyUnstable.title")}</strong>
        <p>{body}</p>
      </div>
      <button
        type="button"
        className="proxy-instability-dismiss"
        onClick={() => setDismissedAt(instability.failureCount)}
        aria-label={t("common.close")}
        title={t("common.close")}
      >
        ✕
      </button>
    </div>
  );
}
