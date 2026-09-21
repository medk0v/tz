import { describe, expect, it } from "vitest";
import { isAllowedAttachmentFile, isAllowedTaskAttachmentFile, TASK_ATTACHMENT_ACCEPT } from "./attachment-files";

describe("chat attachment client policy", () => {
  it("matches the server MIME, extension, and per-type size limits", () => {
    expect(isAllowedAttachmentFile({
      name: "screenshot.PNG",
      size: 10 * 1_024 * 1_024,
      type: "image/png",
    })).toBe(true);
    expect(isAllowedAttachmentFile({
      name: "document.pdf",
      size: 20 * 1_024 * 1_024,
      type: "application/pdf",
    })).toBe(true);
    expect(isAllowedAttachmentFile({
      name: "clip.webm",
      size: 50 * 1_024 * 1_024,
      type: "video/webm",
    })).toBe(true);
  });

  it("rejects empty, oversized, mismatched, and unlisted files", () => {
    expect(isAllowedAttachmentFile({ name: "empty.png", size: 0, type: "image/png" }))
      .toBe(false);
    expect(isAllowedAttachmentFile({
      name: "large.jpg",
      size: 10 * 1_024 * 1_024 + 1,
      type: "image/jpeg",
    })).toBe(false);
    expect(isAllowedAttachmentFile({ name: "renamed.pdf", size: 10, type: "image/png" }))
      .toBe(false);
    expect(isAllowedAttachmentFile({ name: "payload.svg", size: 10, type: "image/svg+xml" }))
      .toBe(false);
  });
});

describe("task attachment client policy", () => {
  it.each([
    ["requirements.docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document", 20],
    ["budget.xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", 20],
    ["slides.pptx", "application/vnd.openxmlformats-officedocument.presentationml.presentation", 20],
    ["document.odt", "application/vnd.oasis.opendocument.text", 20],
    ["table.ods", "application/vnd.oasis.opendocument.spreadsheet", 20],
    ["slides.odp", "application/vnd.oasis.opendocument.presentation", 20],
    ["export.csv", "application/vnd.ms-excel", 20],
    ["export.tsv", "text/tab-separated-values", 20],
    ["notes.md", "text/markdown", 20],
    ["notes.txt", "text/plain", 20],
    ["sources.zip", "application/zip", 20],
    ["screen.PNG", "image/png", 10],
    ["screen.png", "", 10],
    ["document.pdf", "application/octet-stream", 20],
    ["video.mp4", "video/mp4", 50],
    ["video.webm", "video/webm", 50],
  ])("accepts %s and enforces its size limit", (name, type, limitMb) => {
    const size = limitMb * 1_024 * 1_024;
    expect(isAllowedTaskAttachmentFile({ name, type, size })).toBe(true);
    expect(isAllowedTaskAttachmentFile({ name, type, size: size + 1 })).toBe(false);
    expect(isAllowedTaskAttachmentFile({ name, type, size: 0 })).toBe(false);
  });

  it("keeps media MIME validation and chat restrictions", () => {
    expect(isAllowedTaskAttachmentFile({ name: "renamed.png", type: "text/html", size: 10 })).toBe(false);
    expect(isAllowedAttachmentFile({ name: "notes.txt", type: "text/plain", size: 10 })).toBe(false);
  });

  it.each(["README", "png", ".png", "setup.exe", "invoice.pdf.exe", "budget.xlsm", "legacy.doc", "contract.docm", "page.html", "script.js", "image.svg", "archive.rar", "file.txt:payload.exe", "../file.txt"])("rejects unlisted or unsafe filename %s", (name) => {
    expect(isAllowedTaskAttachmentFile({ name, type: "", size: 10 })).toBe(false);
  });

  it("filters the picker with the same allowed extensions", () => {
    expect(TASK_ATTACHMENT_ACCEPT.split(",")).toEqual(expect.arrayContaining([".docx", ".pdf", ".txt", ".xlsx", ".pptx", ".zip"]));
    expect(TASK_ATTACHMENT_ACCEPT).not.toContain(".exe");
    expect(isAllowedTaskAttachmentFile({ name: "notes.docx", type: "text/html", size: 10 })).toBe(false);
  });
});
