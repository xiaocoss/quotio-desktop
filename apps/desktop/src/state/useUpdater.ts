import { useCallback, useEffect, useRef, useState } from "react";
import type { Update } from "@tauri-apps/plugin-updater";
import { checkForUpdate, installUpdate } from "../lib/updater";

export type UpdaterStatus =
  | "idle" // nothing to show
  | "checking" // a check is in flight
  | "available" // a newer version was found
  | "downloading" // installing the update
  | "uptodate" // manual check found nothing (show a brief confirmation)
  | "error"; // manual check failed

// Drives the auto-update flow: one silent check on startup, plus a manual
// "check for updates" path that surfaces "up to date" / errors. Lives in the
// main window only (mounted from AppShell), so the menu-bar panel never checks.
export function useUpdater() {
  const [status, setStatus] = useState<UpdaterStatus>("idle");
  const [version, setVersion] = useState<string | null>(null);
  const [notes, setNotes] = useState<string | null>(null);
  const [percent, setPercent] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const updateRef = useRef<Update | null>(null);

  const check = useCallback(async (manual = false) => {
    setStatus("checking");
    setError(null);
    try {
      const update = await checkForUpdate();
      if (update) {
        updateRef.current = update;
        setVersion(update.version);
        setNotes(update.body ?? null);
        setStatus("available");
      } else {
        // Silent on the startup check; only a manual check confirms "up to date".
        setStatus(manual ? "uptodate" : "idle");
      }
    } catch (cause) {
      // Auto-check failures stay quiet (offline / ghproxy down); manual surfaces.
      setError(cause instanceof Error ? cause.message : String(cause));
      setStatus(manual ? "error" : "idle");
    }
  }, []);

  const install = useCallback(async () => {
    const update = updateRef.current;
    if (!update) return;
    setStatus("downloading");
    setPercent(0);
    try {
      // On success this relaunches the app, so control usually never returns.
      await installUpdate(update, (progress) => setPercent(progress.percent));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setStatus("error");
    }
  }, []);

  // 失败后的重试入口:安装失败时 updateRef 仍在 → 重试安装;否则(检查失败)重新检查。
  const retry = useCallback(() => {
    if (updateRef.current) void install();
    else void check(true);
  }, [install, check]);

  const dismiss = useCallback(() => setStatus("idle"), []);

  // One silent check shortly after startup.
  useEffect(() => {
    void check(false);
  }, [check]);

  return { status, version, notes, percent, error, check, install, retry, dismiss };
}
