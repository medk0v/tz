import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { FileText, LoaderCircle, Paperclip, X } from "lucide-react";
import { ApiRequestError, downloadTaskScreenshot, type OperatorAuth } from "../api";
import { isAllowedTaskAttachmentFile, TASK_ATTACHMENT_ACCEPT } from "../attachment-files";
import { uploadTaskScreenshot } from "../task-orchestration-api";
import { useDemoReadOnly } from "./DemoReadOnly";
import { errorText } from "./task-schedule";
import type { TaskWorkspaceText } from "./task-workspace-i18n";
import "./TaskScreenshots.css";

const MAX_ATTACHMENTS = 5;

function AttachmentPreview({ auth, id, label, text }: { auth: OperatorAuth; id: string; label: string; text: TaskWorkspaceText }) {
  const [preview, setPreview] = useState<{ auth: OperatorAuth; id: string; revision: number; url: string; file: File | null; failed: boolean } | null>(null);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let active = true;
    let objectUrl = "";
    void downloadTaskScreenshot(auth, id).then((file) => {
      if (!active) return;
      objectUrl = URL.createObjectURL(file);
      setPreview({ auth, id, revision, url: objectUrl, file, failed: false });
    }).catch(() => { if (active) setPreview({ auth, id, revision, url: "", file: null, failed: true }); });
    return () => { active = false; if (objectUrl) URL.revokeObjectURL(objectUrl); };
  }, [auth, id, revision]);
  const current = preview?.auth === auth && preview.id === id && preview.revision === revision ? preview : null;
  const url = current?.url;
  if (current?.failed) return <button type="button" className="task-screenshot-retry" onClick={() => setRevision((value) => value + 1)}>{text("screenshotLoadError")}<span>{text("retry")}</span></button>;
  if (!url || !current?.file) return <span className="task-screenshot-loading" role="status"><LoaderCircle size={18} className="task-spin" />{text("loading")}</span>;
  const file = current.file;
  return ["image/png", "image/jpeg", "image/webp"].includes(file.type)
    ? <a href={url} target="_blank" rel="noreferrer" title={`${text("openScreenshot")}: ${file.name}`}><img src={url} alt={label} /></a>
    : <a className="task-attachment-file" href={url} download={file.name} title={file.name} aria-label={`${text("downloadAttachment")}: ${file.name}`}>
      <FileText size={22} aria-hidden="true" /><span className="task-attachment-name">{file.name}</span><span className="task-attachment-size">{Math.max(1, Math.ceil(file.size / 1_024))} KB</span>
    </a>;
}

interface Props {
  auth: OperatorAuth;
  value: string[];
  text: TaskWorkspaceText;
  onChange?: (ids: string[]) => void;
  onBusyChange?: (busy: boolean) => void;
  disabled?: boolean;
  /** The description field shares paste and drop handling with the upload control. */
  children?: ReactNode;
}

export function TaskScreenshots({ auth, value, text, onChange, onBusyChange, disabled = false, children }: Props) {
  const inputId = useId();
  const input = useRef<HTMLInputElement>(null);
  const upload = useRef<AbortController | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const demo = useDemoReadOnly();
  const editable = Boolean(onChange) && !demo && !auth.isDemo;
  const blocked = disabled || busy || !editable;
  useEffect(() => () => { upload.current?.abort(); onBusyChange?.(false); }, [onBusyChange]);

  async function addFiles(files: File[]) {
    if (blocked || upload.current || !files.length) return;
    if (files.some((file) => !isAllowedTaskAttachmentFile(file))) {
      setError(text("screenshotInvalid")); return;
    }
    if (value.length + files.length > MAX_ATTACHMENTS) { setError(text("screenshotLimit")); return; }
    const controller = new AbortController();
    upload.current = controller;
    setBusy(true); setError(""); onBusyChange?.(true);
    const ids = [...value];
    try {
      for (const file of files) {
        const saved = await uploadTaskScreenshot(auth, file, controller.signal);
        if (controller.signal.aborted) return;
        ids.push(saved.id);
        onChange?.([...ids]);
      }
    } catch (failure) {
      if (!controller.signal.aborted) {
        const status = failure instanceof ApiRequestError ? failure.status : undefined;
        setError(status === 400 || status === 415 ? text("screenshotRejected")
          : status === 413 ? text("screenshotStorageLimit")
            : status === 503 || status === 429 ? text("screenshotCheckUnavailable")
              : errorText(failure, text("screenshotUploadError")));
      }
    } finally {
      if (!controller.signal.aborted) { setBusy(false); onBusyChange?.(false); upload.current = null; }
    }
  }

  return <div className="task-screenshots" onPaste={(event) => {
    if (blocked) return;
    const files = Array.from(event.clipboardData.files);
    if (!files.length) return;
    event.preventDefault(); void addFiles(files);
  }} onDragEnter={(event) => {
    if (editable && event.dataTransfer.types.includes("Files")) event.preventDefault();
  }} onDragOver={(event) => {
    if (editable && event.dataTransfer.types.includes("Files")) { event.preventDefault(); event.dataTransfer.dropEffect = blocked ? "none" : "copy"; }
  }} onDrop={(event) => {
    if (!event.dataTransfer.files.length) return;
    event.preventDefault(); if (!blocked) void addFiles(Array.from(event.dataTransfer.files));
  }}>
    {children}
    {(onChange || value.length > 0) && <div className="task-screenshots-heading">
      <span>{text("screenshots")}</span>
      {onChange && <><input id={inputId} ref={input} type="file" accept={TASK_ATTACHMENT_ACCEPT} multiple hidden aria-label={text("addScreenshots")} disabled={blocked || value.length >= MAX_ATTACHMENTS}
        onChange={(event) => { const files = Array.from(event.target.files ?? []); event.target.value = ""; void addFiles(files); }} />
        <button type="button" className="task-text-button" disabled={blocked || value.length >= MAX_ATTACHMENTS} onClick={() => input.current?.click()}><Paperclip size={16} />{text("addScreenshots")}</button></>}
    </div>}
    {onChange && <p className="task-muted task-screenshots-help">{text("screenshotsHelp")}</p>}
    {value.length > 0 && <ul className="task-screenshot-list">{value.map((id, index) => <li key={id}>
      <AttachmentPreview auth={auth} id={id} label={`${text("screenshot")} ${index + 1}`} text={text} />
      {onChange && <button type="button" className="task-screenshot-remove" aria-label={`${text("removeScreenshot")} ${index + 1}`} disabled={blocked}
        onClick={() => { setError(""); onChange(value.filter((item) => item !== id)); }}><X size={14} /></button>}
    </li>)}</ul>}
    {busy && <p className="task-muted" role="status">{text("screenshotsUploading")}</p>}
    {error && <p className="task-error" role="alert">{error}</p>}
  </div>;
}
