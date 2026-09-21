import { useEffect, useId, useRef, useState, type FormEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Check, Copy, Link2, LockKeyhole, Settings2, Trash2, X } from "lucide-react";
import { ApiRequestError, type OperatorAuth } from "../api";
import { createNoteShare, listNoteShares, publicNoteSharePath, revokeNoteShare, updateNoteShare, type CreatedNoteShare, type NoteShare } from "../note-shares-api";
import { listNotes, type NoteSummary } from "../notes-api";
import { useI18n } from "../i18n";
import { noteSharesText } from "../note-shares-i18n";
import {  } from "./PreferenceDropdown";
import {    } from "./preference-options";
import { materializeShareSettings, sharePageRows, shareSettingsInput, toggleSharedPage, type SharePageRow, type ShareSettingsDraft } from "./note-share-settings";
import { useTaskDialog } from "./useTaskDialog";
import "./NoteShareDialog.css";

interface Props {
  auth: OperatorAuth;
  noteId: string | null;
  title?: string;
  onClose: () => void;
}

type Message = Parameters<ReturnType<typeof noteSharesText>>[0];
type Editing = { share: NoteShare; settings: ShareSettingsDraft };

function newSettings(rows: SharePageRow[], noteId: string | null): ShareSettingsDraft {
  return { theme_palette: null, theme_mode: null, allow_theme_change: false, include_descendants: noteId === null,
    included_note_ids: noteId === null ? rows.map((row) => row.note.id) : [], can_edit: false };
}

function ShareSettingsFields({ settings, rows, noteId, disabled, onChange }: {
  settings: ShareSettingsDraft; rows: SharePageRow[]; noteId: string | null; disabled: boolean; onChange: (settings: ShareSettingsDraft) => void;
}) {
  const { locale } = useI18n();
  const t = noteSharesText(locale);
  const id = useId();
  const selected = new Set(settings.included_note_ids);
  const shown = rows.filter((row) => settings.include_descendants || (noteId === null && row.ancestors.length === 0));
  const children = new Map<string | null, SharePageRow[]>();
  for (const row of shown) children.set(row.parentId, [...(children.get(row.parentId) ?? []), row]);
  function pages(parent: string | null): ReactNode {
    const items = children.get(parent);
    return items?.length ? <ul>{items.map((row) => <li key={row.note.id}>
      <label><input type="checkbox" disabled={disabled} checked={selected.has(row.note.id)} aria-label={row.path}
        onChange={(event) => onChange({ ...settings, included_note_ids: toggleSharedPage(settings.included_note_ids, rows, row.note.id, event.target.checked) })} />
        {row.note.icon && <span aria-hidden="true">{row.note.icon}</span>}<span>{row.note.title}</span></label>
      {pages(row.note.id)}
    </li>)}</ul> : null;
  }
  return <>
    
    <fieldset className="note-share-pages" aria-describedby={`${id}-pages-help`}>
      <legend>{t("sharedPages")}</legend>
      <label className="note-share-edit"><input type="checkbox" checked={settings.include_descendants} disabled={disabled}
        onChange={(event) => onChange({ ...settings, include_descendants: event.target.checked })} />{t("includeDescendants")}</label>
      {noteId && <p className="note-share-pages-help">{t("rootAlwaysShared")}</p>}
      <div className="note-share-page-tree">{pages(null)}</div>
      <p id={`${id}-pages-help`} className="note-share-pages-help">{t("selectedPagesHelp")}</p>
      {noteId === null && !shown.some((row) => selected.has(row.note.id)) && <p className="note-share-pages-help">{t("selectionEmpty")}</p>}
    </fieldset>
    <label className="note-share-edit"><input type="checkbox" checked={settings.can_edit} disabled={disabled} aria-describedby={`${id}-edit-help`}
      onChange={(event) => onChange({ ...settings, can_edit: event.target.checked })} />{t("canEdit")}</label>
    <p id={`${id}-edit-help`} className="note-share-edit-help">{t("editHelp")}</p>
  </>;
}

export function NoteShareDialog({ auth, noteId, title = "", onClose }: Props) {
  const { locale } = useI18n();
  const t = noteSharesText(locale);
  const id = useId();
  const { dialog, close } = useTaskDialog(onClose);
  const active = useRef(true);
  const opener = useRef(typeof document === "undefined" ? null : document.activeElement);
  const [shares, setShares] = useState<NoteShare[]>([]);
  const [created, setCreated] = useState<CreatedNoteShare[]>([]);
  const [notes, setNotes] = useState<NoteSummary[]>([]);
  const [creation, setCreation] = useState<ShareSettingsDraft | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [notesLoadFailed, setNotesLoadFailed] = useState(false);
  const [reload, setReload] = useState(0);
  const [busy, setBusy] = useState(false);
  const [password, setPassword] = useState("");
  const [slug, setSlug] = useState("");
  const [error, setError] = useState<Message | null>(null);
  const [notice, setNotice] = useState<Message | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const rows = sharePageRows(notes, noteId);
  const settings = editing?.settings ?? creation;
  const scopeAvailable = noteId === null || notes.some((note) => note.id === noteId);

  useEffect(() => {
    active.current = true;
    const returnFocus = opener.current;
    return () => { active.current = false; if (returnFocus instanceof HTMLElement && returnFocus.isConnected) returnFocus.focus({ preventScroll: true }); };
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    void Promise.allSettled([
      listNoteShares(auth, controller.signal).then((items) => {
        if (controller.signal.aborted) return;
        setShares(items.filter((item) => item.note_id === noteId)); setLoadFailed(false);
      }).catch(() => { if (!controller.signal.aborted) setLoadFailed(true); }),
      listNotes(auth, controller.signal).then((items) => {
        if (controller.signal.aborted) return;
        setNotes(items); setNotesLoadFailed(false);
        setCreation((current) => current ?? newSettings(sharePageRows(items, noteId), noteId));
      }).catch(() => { if (!controller.signal.aborted) setNotesLoadFailed(true); }),
    ]).finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [auth, noteId, reload]);

  const valid = (!password || ([...password].length >= 10 && [...password].length <= 128 && !password.includes("\0")))
    && (!slug.trim() || /^[a-z0-9][a-z0-9-]{1,62}[a-z0-9]$/.test(slug.trim()));
  const unavailable = busy || loading || loadFailed || notesLoadFailed || !scopeAvailable;

  function changeSettings(next: ShareSettingsDraft) {
    if (editing) setEditing({ ...editing, settings: next });
    else setCreation(next);
  }

  async function submit(event: FormEvent) {
    event.preventDefault(); event.stopPropagation();
    if (unavailable || !settings || (!editing && !valid)) return;
    setBusy(true); setError(null); setNotice(null);
    const input = shareSettingsInput(settings, rows, noteId);
    try {
      if (editing) {
        const item = await updateNoteShare(auth, editing.share.id, { ...input, expected_version: editing.share.version });
        if (!active.current) return;
        setShares((items) => items.map((share) => share.id === item.id ? item : share));
        setEditing({ share: item, settings: materializeShareSettings(item, rows) }); setNotice("settingsSaved");
      } else {
        const item = await createNoteShare(auth, { ...input, note_id: noteId, custom_slug: slug.trim() || null, password: password || null });
        if (!active.current) return;
        setShares((items) => [item, ...items]); setCreated((items) => [item, ...items]);
        setPassword(""); setSlug(""); setCreation(newSettings(rows, noteId));
      }
    } catch (failure) {
      if (active.current) setError(failure instanceof ApiRequestError && failure.status === 409
        ? editing ? "settingsConflict" : "slugUnavailable" : editing ? "settingsError" : "createError");
    } finally { if (active.current) setBusy(false); }
  }

  async function edit(share: NoteShare) {
    if (busy || loading) return;
    setBusy(true); setError(null); setNotice(null);
    try {
      // Reopening is an explicit reload after a version conflict. Never replace a draft on a failed save.
      const [currentShares, currentNotes] = await Promise.all([listNoteShares(auth), listNotes(auth)]);
      if (!active.current) return;
      const current = currentShares.find((item) => item.id === share.id && item.note_id === noteId);
      if (!current) { setError("unavailable"); return; }
      setShares(currentShares.filter((item) => item.note_id === noteId)); setNotes(currentNotes);
      setLoadFailed(false); setNotesLoadFailed(false);
      setEditing({ share: current, settings: materializeShareSettings(current, sharePageRows(currentNotes, noteId)) });
    } catch { if (active.current) setError("loadError"); }
    finally { if (active.current) setBusy(false); }
  }

  async function revoke(share: NoteShare) {
    if (busy || !window.confirm(t("revokeConfirm"))) return;
    setBusy(true); setError(null); setNotice(null);
    try {
      await revokeNoteShare(auth, share.id);
      if (!active.current) return;
      setShares((items) => items.filter((item) => item.id !== share.id));
      setCreated((items) => items.filter((item) => item.id !== share.id));
      if (editing?.share.id === share.id) setEditing(null);
      setNotice("revoked");
    } catch { if (active.current) setError("revokeError"); }
    finally { if (active.current) setBusy(false); }
  }

  async function copy(shareId: string, url: string) {
    try {
      await navigator.clipboard.writeText(url);
      if (active.current) { setCopiedId(shareId); setError(null); }
    } catch { if (active.current) setError("copyFailed"); }
  }

  return createPortal(<dialog ref={dialog} className="note-share-dialog" aria-labelledby={`${id}-title`} aria-describedby={`${id}-scope`}
    onCancel={(event) => { event.preventDefault(); if (!busy) close(); }}>
    <header><div><h2 id={`${id}-title`}>{t("title")}</h2><p id={`${id}-scope`}>{noteId ? t("pageScope", { title }) : t("projectScope")}</p></div>
      <button type="button" className="note-share-icon" aria-label={t("close")} disabled={busy} data-autofocus onClick={() => close()}><X size={20} /></button></header>
    <div className="note-share-body">
      <p className="note-share-help">{t(noteId ? "pageHelp" : "projectHelp")}</p>
      <p className="note-share-help">{t("tablesHelp")}</p>
      {error && <p className="note-share-error" role="alert">{t(error)}</p>}{notice && <p role="status">{t(notice)}</p>}
      {loading && <p role="status">{t("loading")}</p>}
      {(loadFailed || notesLoadFailed || (!loading && !scopeAvailable)) && <div role="alert"><p>{t(loadFailed ? "loadError" : "notesLoadError")}</p>
        <button type="button" className="secondary-button" disabled={loading || busy} onClick={() => { setLoading(true); setReload((value) => value + 1); }}>{t("retry")}</button></div>}
      <form className="note-share-form" aria-label={t(editing ? "editSettings" : "create")} onSubmit={(event) => void submit(event)}>
        {editing ? <div className="note-share-settings-heading"><h3>{t("editSettings")}: {editing.share.custom_slug || t("generated")}</h3>
          <button type="button" className="secondary-button" disabled={busy} onClick={() => { setEditing(null); setError(null); setNotice(null); }}>{t("cancelSettings")}</button></div> : <>
          <label htmlFor={`${id}-slug`}>{t("slug")}<input id={`${id}-slug`} aria-label={t("slug")} value={slug} placeholder="project-notes" maxLength={64} pattern={"[a-z0-9][a-z0-9\\-]{1,62}[a-z0-9]"} autoCapitalize="none" autoCorrect="off" spellCheck={false}
            disabled={busy} aria-describedby={`${id}-slug-help`} onChange={(event) => setSlug(event.target.value.toLowerCase())} /><span id={`${id}-slug-help`}>{t("slugHelp")}</span></label>
          <label htmlFor={`${id}-password`}>{t("optionalPassword")}<input id={`${id}-password`} aria-label={t("optionalPassword")} type="password" autoComplete="new-password" value={password} maxLength={256}
            disabled={busy} aria-describedby={`${id}-password-help`} onChange={(event) => setPassword(event.target.value)} /><span id={`${id}-password-help`}>{t("passwordHelp")}</span></label>
        </>}
        {settings && <ShareSettingsFields settings={settings} rows={rows} noteId={noteId} disabled={unavailable} onChange={changeSettings} />}
        <button className="primary-button" type="submit" disabled={unavailable || !settings || (!editing && !valid)}><Link2 size={16} />{t(editing ? busy ? "savingSettings" : "saveSettings" : busy ? "creating" : "create")}</button>
      </form>
      <section className="note-share-links" aria-label={t("title")}>
        {!loading && !loadFailed && shares.length === 0 && <p className="note-share-help">{t("empty")}</p>}
        <ul>{shares.map((share) => {
          const fresh = created.find((item) => item.id === share.id);
          const token = share.custom_slug ?? fresh?.token;
          const url = token ? `${window.location.origin}${publicNoteSharePath(token)}` : null;
          return <li key={share.id}>
            <div className="note-share-link-heading"><strong>{share.custom_slug || t("generated")}</strong>
              <div className="note-share-link-actions"><button type="button" className="secondary-button" disabled={busy || loading} onClick={() => void edit(share)}><Settings2 size={14} />{t("editSettings")}</button>
                <button type="button" className="note-share-icon note-share-revoke" disabled={busy} aria-label={t("revoke")} onClick={() => void revoke(share)}><Trash2 size={16} /></button></div></div>
            <div className="note-share-link-details"><span>{t(share.can_edit ? "editable" : "readOnly")}</span><span>{share.has_password && <LockKeyhole size={12} />}{t(share.has_password ? "protected" : "unprotected")}</span>
              <time dateTime={share.created_at}>{t("createdAt", { date: new Date(share.created_at).toLocaleDateString(locale) })}</time></div>
            {url ? <div className="note-share-url"><input aria-label={fresh ? t("newLink") : t("custom")} readOnly value={url} onFocus={(event) => event.currentTarget.select()} />
              <button type="button" className="secondary-button" aria-label={copiedId === share.id ? t("copied") : t("copy")} onClick={() => void copy(share.id, url)}>{copiedId === share.id ? <Check size={15} /> : <Copy size={15} />}<span>{t(copiedId === share.id ? "copied" : "copy")}</span></button></div> : null}
            {!share.custom_slug && <p className="note-share-secret-hint">{t(fresh ? "secretHint" : "existingSecret")}</p>}
          </li>;
        })}</ul>
      </section>
    </div>
  </dialog>, document.body);
}
