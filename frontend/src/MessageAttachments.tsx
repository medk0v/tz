import { useEffect, useRef, useState } from "react";
import { Download, FileText, Image as ImageIcon, Mic, Play } from "lucide-react";
import type { MessageAttachment } from "./api";

interface AttachmentLabels {
  download: string;
  loadVideo: string;
  unavailable: string;
  loadError: string;
  securityChecked: string;
}

interface MessageAttachmentsProps {
  attachments: MessageAttachment[] | undefined;
  loadAttachment: (attachmentId: string) => Promise<Blob>;
  labels: AttachmentLabels;
  variant: "operator" | "widget";
}

function formatByteSize(value: number): string {
  if (value < 1_024) return `${value} B`;
  if (value < 1_024 * 1_024) return `${(value / 1_024).toFixed(1)} KB`;
  return `${(value / (1_024 * 1_024)).toFixed(1)} MB`;
}

function fileIcon(contentType: string) {
  if (contentType.startsWith("image/")) return <ImageIcon aria-hidden="true" size={17} />;
  if (contentType.startsWith("video/")) return <Play aria-hidden="true" size={17} />;
  if (contentType.startsWith("audio/")) return <Mic aria-hidden="true" size={17} />;
  return <FileText aria-hidden="true" size={17} />;
}

function AttachmentItem({
  attachment,
  loadAttachment,
  labels,
  variant,
}: {
  attachment: MessageAttachment;
  loadAttachment: (attachmentId: string) => Promise<Blob>;
  labels: AttachmentLabels;
  variant: MessageAttachmentsProps["variant"];
}) {
  const loaderRef = useRef(loadAttachment);
  const [objectUrl, setObjectUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);
  const image = attachment.content_type.startsWith("image/");
  const video = attachment.content_type.startsWith("video/");
  // Voice recordings are small, so they load immediately like image previews.
  const audio = attachment.content_type.startsWith("audio/");
  const preview = image || audio;
  const busy = loading || (attachment.available && preview && !objectUrl && !error);

  useEffect(() => {
    loaderRef.current = loadAttachment;
  }, [loadAttachment]);

  useEffect(() => {
    if (!attachment.available || !preview) return undefined;
    let active = true;
    loaderRef.current(attachment.id)
      .then((blob) => {
        if (active) setObjectUrl(URL.createObjectURL(blob));
      })
      .catch(() => {
        if (active) setError(true);
      })
    return () => { active = false; };
  }, [attachment.available, attachment.id, preview]);

  useEffect(() => () => {
    if (objectUrl) URL.revokeObjectURL(objectUrl);
  }, [objectUrl]);

  async function ensureObjectUrl(): Promise<string> {
    if (objectUrl) return objectUrl;
    setLoading(true);
    setError(false);
    try {
      const url = URL.createObjectURL(await loaderRef.current(attachment.id));
      setObjectUrl(url);
      return url;
    } catch (cause) {
      setError(true);
      throw cause;
    } finally {
      setLoading(false);
    }
  }

  async function handleDownload() {
    try {
      const url = await ensureObjectUrl();
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = attachment.file_name;
      anchor.rel = "noopener";
      anchor.click();
    } catch {
      // The inline error below is enough; no unhandled rejection is exposed.
    }
  }

  async function handleLoadVideo() {
    try {
      await ensureObjectUrl();
    } catch {
      // The inline error below is enough; no unhandled rejection is exposed.
    }
  }

  return (
    <section className={`chat-attachment chat-attachment--${variant}`}>
      {image && objectUrl && (
        <img className="chat-attachment-preview" src={objectUrl} alt={attachment.file_name} />
      )}
      {video && objectUrl && (
        <video
          className="chat-attachment-preview"
          src={objectUrl}
          aria-label={attachment.file_name}
          controls
          preload="metadata"
        />
      )}
      {audio && objectUrl && (
        <audio
          className="chat-attachment-audio"
          src={objectUrl}
          aria-label={attachment.file_name}
          controls
          preload="metadata"
        />
      )}
      <div className="chat-attachment-row">
        <span className="chat-attachment-icon">{fileIcon(attachment.content_type)}</span>
        <span className="chat-attachment-details">
          <strong title={attachment.file_name}>{attachment.file_name}</strong>
          <span>{formatByteSize(attachment.byte_size)} · {labels.securityChecked}</span>
        </span>
        {attachment.available && !objectUrl && video && (
          <button type="button" disabled={busy} onClick={() => void handleLoadVideo()}>
            <Play aria-hidden="true" size={16} />
            <span className="visually-hidden">{labels.loadVideo}</span>
          </button>
        )}
        {attachment.available && (!video || objectUrl) && (
          <button type="button" disabled={busy} onClick={() => void handleDownload()}>
            <Download aria-hidden="true" size={16} />
            <span className="visually-hidden">{labels.download}</span>
          </button>
        )}
      </div>
      {!attachment.available && <p className="chat-attachment-status">{labels.unavailable}</p>}
      {error && <p className="chat-attachment-status" role="alert">{labels.loadError}</p>}
    </section>
  );
}

export function MessageAttachments({
  attachments,
  loadAttachment,
  labels,
  variant,
}: MessageAttachmentsProps) {
  if (!attachments?.length) return null;
  return (
    <div className="chat-attachments">
      {attachments.map((attachment) => (
        <AttachmentItem
          attachment={attachment}
          key={attachment.id}
          labels={labels}
          loadAttachment={loadAttachment}
          variant={variant}
        />
      ))}
    </div>
  );
}
