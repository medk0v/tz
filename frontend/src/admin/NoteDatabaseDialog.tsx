import { useContext, useEffect, useId, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { useI18n } from "../i18n";
import { useTaskDialog } from "./useTaskDialog";
import { noteDatabasesText } from "./note-databases-i18n";
import { NoteDatabaseSuspendedContext } from "./note-database-state";
import "./NoteDatabase.css";

export function NoteDatabaseDialog({ title, children, onClose, dirty = false, busy = false, onDirtyChange, onBusyChange, wide = false, suspended: suspendDialog = false }: {
  title: string; children: ReactNode; onClose: () => void; dirty?: boolean; busy?: boolean; onDirtyChange?: (dirty: boolean) => void; onBusyChange?: (busy: boolean) => void; wide?: boolean; suspended?: boolean;
}) {
  const { locale } = useI18n(); const t = noteDatabasesText(locale); const id = useId();
  const { dialog, close } = useTaskDialog(onClose);
  const suspendedByParent = useContext(NoteDatabaseSuspendedContext);
  const suspended = suspendDialog || suspendedByParent;
  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    if (suspended) { element.close?.(); element.removeAttribute("open"); }
    else if (!element.open) { if (element.showModal) element.showModal(); else element.setAttribute("open", ""); }
  }, [dialog, suspended]);
  function dismiss() { if (!busy && (!dirty || window.confirm(t("discardConfirm")))) close(); }
  useEffect(() => { onDirtyChange?.(dirty || busy); return () => onDirtyChange?.(false); }, [dirty, busy, onDirtyChange]);
  useEffect(() => { onBusyChange?.(busy); return () => onBusyChange?.(false); }, [busy, onBusyChange]);
  useEffect(() => {
    if (!dirty && !busy) return;
    const guard = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ""; };
    window.addEventListener("beforeunload", guard); return () => window.removeEventListener("beforeunload", guard);
  }, [dirty, busy]);
  return createPortal(<dialog ref={dialog} className={`note-database-dialog${wide ? " note-database-dialog--wide" : ""}`} aria-labelledby={id}
    onCancel={(event) => { event.preventDefault(); event.stopPropagation(); dismiss(); }}><header><h2 id={id}>{title}</h2><button type="button" aria-label={t("close")} disabled={busy} onClick={dismiss}><X size={19} /></button></header>
    <div className="note-database-dialog-body">{children}</div></dialog>, document.body);
}
