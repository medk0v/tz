export const ATTACHMENT_ACCEPT =
  "image/jpeg,image/png,image/webp,application/pdf,video/mp4,video/webm";

const attachmentLimits = new Map<string, { bytes: number; extensions: ReadonlySet<string> }>([
  ["image/jpeg", { bytes: 10 * 1_024 * 1_024, extensions: new Set(["jpg", "jpeg"]) }],
  ["image/png", { bytes: 10 * 1_024 * 1_024, extensions: new Set(["png"]) }],
  ["image/webp", { bytes: 10 * 1_024 * 1_024, extensions: new Set(["webp"]) }],
  ["application/pdf", { bytes: 20 * 1_024 * 1_024, extensions: new Set(["pdf"]) }],
  ["video/mp4", { bytes: 50 * 1_024 * 1_024, extensions: new Set(["mp4"]) }],
  ["video/webm", { bytes: 50 * 1_024 * 1_024, extensions: new Set(["webm"]) }],
]);

export function isAllowedAttachmentFile(file: Pick<File, "name" | "size" | "type">): boolean {
  const policy = attachmentLimits.get(file.type);
  const extension = file.name.split(".").pop()?.toLocaleLowerCase("en-US");
  return file.size > 0
    && policy !== undefined
    && file.size <= policy.bytes
    && extension !== undefined
    && policy.extensions.has(extension);
}

export function isAllowedTaskAttachmentFile(file: Pick<File, "name" | "size" | "type">): boolean {
  const name = file.name.trim();
  if (!name || name.length > 200 || /[\\/:\p{Cc}\u200b-\u200f\u202a-\u202e\u2066-\u2069\ufeff]/u.test(name)) return false;
  const extension = /^.+\.([^.]+)$/.exec(name)?.[1].toLocaleLowerCase("en-US");
  const media = Array.from(attachmentLimits).find(([, policy]) => extension && policy.extensions.has(extension));
  const businessMimes = extension ? taskBusinessTypes.get(extension) : undefined;
  const maxBytes = media?.[1].bytes ?? 20 * 1_024 * 1_024;
  return Boolean(media || businessMimes) && file.size > 0 && file.size <= maxBytes
    && (!file.type || file.type === "application/octet-stream" || file.type === media?.[0] || Boolean(businessMimes?.includes(file.type)));
}

const taskBusinessTypes = new Map<string, readonly string[]>([
  ["docx", ["application/vnd.openxmlformats-officedocument.wordprocessingml.document"]],
  ["xlsx", ["application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"]],
  ["pptx", ["application/vnd.openxmlformats-officedocument.presentationml.presentation"]],
  ["odt", ["application/vnd.oasis.opendocument.text"]],
  ["ods", ["application/vnd.oasis.opendocument.spreadsheet"]],
  ["odp", ["application/vnd.oasis.opendocument.presentation"]],
  ["txt", ["text/plain"]],
  ["csv", ["text/csv", "text/plain", "application/vnd.ms-excel"]],
  ["tsv", ["text/tab-separated-values", "text/plain"]],
  ["md", ["text/markdown", "text/x-markdown", "text/plain"]],
  ["zip", ["application/zip", "application/x-zip-compressed"]],
]);

export const TASK_ATTACHMENT_ACCEPT = [
  ...Array.from(attachmentLimits.values()).flatMap(({ extensions }) => Array.from(extensions)),
  ...taskBusinessTypes.keys(),
].map((extension) => `.${extension}`).join(",");
