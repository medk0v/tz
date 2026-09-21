import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronDown, MessagesSquare, Search, X } from "lucide-react";
import { listReplyTemplates, type OperatorAuth, type ReplyTemplate } from "../api";
import { useI18n } from "../i18n";
import "./ReplyTemplatePicker.css";

export interface ReplyTemplatePickerProps {
  auth: OperatorAuth;
  inboxId: string;
  disabled: boolean;
  onInsert: (body: string) => boolean;
}

function preview(body: string): string {
  return body.replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/(^|\n)\s*(?:>\s*|[-*+]\s+|\d+\.\s+)/g, "$1")
    .replace(/[*_~`]/g, "").replace(/\s+/g, " ").trim();
}

function TemplateChoices({ auth, inboxId, onInsert, onClose }: Omit<ReplyTemplatePickerProps, "disabled"> & { onClose: (restoreFocus?: boolean) => void }) {
  const { t } = useI18n();
  const titleId = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [items, setItems] = useState<ReplyTemplate[]>([]);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const [tooLong, setTooLong] = useState(false);
  const [revision, setRevision] = useState(0);

  useEffect(() => { searchRef.current?.focus(); }, []);
  useEffect(() => {
    const controller = new AbortController();
    listReplyTemplates(auth, inboxId, controller.signal)
      .then((templates) => { if (!controller.signal.aborted) setItems(templates); })
      .catch(() => { if (!controller.signal.aborted) setLoadError(true); })
      .finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [auth, inboxId, revision]);

  const search = query.trim().toLocaleLowerCase();
  const matches = items.filter((item) => `${item.title}\n${item.body}`.toLocaleLowerCase().includes(search));

  return createPortal(
    <div className="reply-template-picker-overlay" onPointerDown={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}>
      <div ref={panelRef} className="reply-template-picker" role="dialog" aria-modal="true" aria-labelledby={titleId}
        onKeyDown={(event) => {
          if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); onClose(); }
          if (event.key !== "Tab") return;
          const controls = panelRef.current?.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled])');
          if (!controls?.length) return;
          const first = controls[0];
          const last = controls[controls.length - 1];
          if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
          else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
        }}>
        <header>
          <h2 id={titleId}>{t("replyTemplates.choose")}</h2>
          <button className="icon-button" type="button" aria-label={t("replyTemplates.close")} onClick={() => onClose()}><X size={18} /></button>
        </header>
        <p className="reply-template-picker-hint">{t("replyTemplates.pickerHint")}</p>
        <label className="reply-template-picker-search">
          <Search size={17} aria-hidden="true" />
          <span className="visually-hidden">{t("replyTemplates.search")}</span>
          <input ref={searchRef} type="search" value={query} placeholder={t("replyTemplates.searchPlaceholder")} onChange={(event) => setQuery(event.target.value)} />
        </label>
        {tooLong && <p role="alert" className="message-editor-error">{t("replyTemplates.insertTooLong")}</p>}
        <div className="reply-template-picker-results" aria-busy={loading}>
          {loading ? <p role="status">{t("replyTemplates.loading")}</p> : loadError ? (
            <div role="alert">
              <p>{t("replyTemplates.loadError")}</p>
              <button type="button" className="secondary-button" onClick={() => { setLoadError(false); setLoading(true); setRevision((value) => value + 1); }}>{t("replyTemplates.retry")}</button>
            </div>
          ) : matches.length === 0 ? <p role="status">{t(items.length ? "replyTemplates.noResults" : "replyTemplates.empty")}</p> : (
            <ul>
              {matches.map((item) => (
                <li key={item.id}>
                  <button type="button" aria-label={t("replyTemplates.insert", { title: item.title })} onClick={() => {
                    if (onInsert(item.body)) onClose(false);
                    else setTooLong(true);
                  }}>
                    <strong>{item.title}</strong>
                    <span>{preview(item.body)}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>, document.body,
  );
}

export function ReplyTemplatePicker({ disabled, ...props }: ReplyTemplatePickerProps) {
  const { t } = useI18n();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  return (
    <>
      <button ref={triggerRef} type="button" className="reply-template-picker-trigger" disabled={disabled}
        aria-haspopup="dialog" aria-expanded={open && !disabled} title={t("replyTemplates.choose")} onClick={() => setOpen(true)}>
        <MessagesSquare size={16} /><span>{t("replyTemplates.button")}</span><ChevronDown size={13} />
      </button>
      {open && !disabled && <TemplateChoices key={props.inboxId} {...props} onClose={(restoreFocus = true) => {
        setOpen(false);
        if (restoreFocus) triggerRef.current?.focus();
      }} />}
    </>
  );
}
