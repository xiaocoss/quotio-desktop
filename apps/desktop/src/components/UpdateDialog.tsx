import { useT } from "../i18n";
import type { UpdaterStatus } from "../state/useUpdater";

type UpdateDialogProps = {
  status: UpdaterStatus;
  version: string | null;
  notes: string | null;
  percent: number;
  error: string | null;
  onInstall: () => void;
  onRetry: () => void;
  onDismiss: () => void;
};

// Modal shown by the auto-updater. Reuses the close-dialog styling for a
// consistent centered card. Nothing renders for idle/checking (the startup
// check is silent until it finds something).
export function UpdateDialog({ status, version, notes, percent, error, onInstall, onRetry, onDismiss }: UpdateDialogProps) {
  const t = useT();
  if (status === "idle" || status === "checking") return null;

  // While downloading, clicking the backdrop must not dismiss the install.
  const dismissable = status !== "downloading";

  return (
    <div className="modal-overlay" onClick={dismissable ? onDismiss : undefined}>
      <div className="close-dialog" onClick={(event) => event.stopPropagation()}>
        {status === "available" ? (
          <>
            <strong className="close-dialog-title">
              {t("update.available")} v{version}
            </strong>
            {notes ? (
              <p className="close-dialog-desc update-notes">{notes}</p>
            ) : (
              <p className="close-dialog-desc">{t("update.availableDesc")}</p>
            )}
            <div className="close-dialog-actions">
              <button type="button" className="ghost-action" onClick={onDismiss}>
                {t("update.later")}
              </button>
              <button type="button" className="secondary-action" onClick={onInstall}>
                {t("update.installNow")}
              </button>
            </div>
          </>
        ) : null}

        {status === "downloading" ? (
          <>
            <strong className="close-dialog-title">{t("update.downloading")}</strong>
            <div className="update-progress" aria-hidden="true">
              <span style={{ width: `${percent}%` }} />
            </div>
            <p className="close-dialog-desc">{percent}%</p>
          </>
        ) : null}

        {status === "uptodate" ? (
          <>
            <strong className="close-dialog-title">{t("update.upToDate")}</strong>
            <div className="close-dialog-actions">
              <button type="button" className="secondary-action" onClick={onDismiss}>
                {t("common.close")}
              </button>
            </div>
          </>
        ) : null}

        {status === "error" ? (
          <>
            <strong className="close-dialog-title">
              {version ? t("update.installFailed") : t("update.failed")}
            </strong>
            <p className="close-dialog-desc">{error}</p>
            <div className="close-dialog-actions">
              <button type="button" className="ghost-action" onClick={onDismiss}>
                {t("common.close")}
              </button>
              <button type="button" className="secondary-action" onClick={onRetry}>
                {t("common.retry")}
              </button>
            </div>
          </>
        ) : null}
      </div>
    </div>
  );
}
