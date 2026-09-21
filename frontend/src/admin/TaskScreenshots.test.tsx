import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiRequestError, downloadTaskScreenshot, type OperatorAuth } from "../api";
import { uploadTaskScreenshot } from "../task-orchestration-api";
import { TaskScreenshots } from "./TaskScreenshots";
import { taskWorkspaceText } from "./task-workspace-i18n";

vi.mock("../api", async (importOriginal) => ({ ...await importOriginal<typeof import("../api")>(), downloadTaskScreenshot: vi.fn() }));
vi.mock("../task-orchestration-api", () => ({ uploadTaskScreenshot: vi.fn() }));
const auth: OperatorAuth = { kind: "session" };
const text = taskWorkspaceText("en");
const file = () => new File(["screenshot"], "screen.png", { type: "image/png" });

function Form({ initial = [], readOnly = false }: { initial?: string[]; readOnly?: boolean }) {
  const [ids, setIds] = useState(initial);
  const [busy, setBusy] = useState(false);
  return <><TaskScreenshots auth={auth} value={ids} text={text} onChange={readOnly ? undefined : setIds} onBusyChange={setBusy}>
    <textarea aria-label="Description" defaultValue="Keep my draft" />
  </TaskScreenshots><button type="button" disabled={busy}>Save</button><output>{ids.join(",")}</output></>;
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.stubGlobal("URL", { createObjectURL: vi.fn(() => "blob:screenshot"), revokeObjectURL: vi.fn() });
  vi.mocked(downloadTaskScreenshot).mockResolvedValue(file());
  vi.mocked(uploadTaskScreenshot).mockResolvedValue({ id: "uploaded" });
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe("task attachments", () => {
  it("uploads a selected image, blocks saving until it finishes, and removes its preview", async () => {
    let complete!: (value: { id: string }) => void;
    vi.mocked(uploadTaskScreenshot).mockReturnValue(new Promise((resolve) => { complete = resolve; }));
    render(<Form />);
    const image = file();
    fireEvent.change(screen.getByLabelText(text("addScreenshots")), { target: { files: [image] } });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(uploadTaskScreenshot).toHaveBeenCalledWith(auth, image, expect.any(AbortSignal));
    await act(async () => { complete({ id: "uploaded" }); });
    expect(await screen.findByRole("img", { name: "Attachment 1" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: `${text("removeScreenshot")} 1` }));
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:screenshot");
  });

  it.each(["paste", "drop"])("accepts screenshots by %s over the description", async (method) => {
    render(<Form />);
    const image = file();
    if (method === "paste") fireEvent.paste(screen.getByLabelText("Description"), { clipboardData: { files: [image] } });
    else {
      expect(fireEvent.dragEnter(screen.getByLabelText("Description"), { dataTransfer: { types: ["Files"] } })).toBe(false);
      fireEvent.drop(screen.getByLabelText("Description"), { dataTransfer: { files: [image] } });
    }
    expect(await screen.findByRole("img")).toBeInTheDocument();
    expect(screen.getByLabelText("Description")).toHaveValue("Keep my draft");
  });

  it.each(["select", "paste", "drop"])("uploads a document by %s and downloads it with its original name", async (method) => {
    const document = new File(["document"], "Требования.docx", { type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document" });
    vi.mocked(downloadTaskScreenshot).mockResolvedValue(new File([document], document.name, { type: "application/octet-stream" }));
    render(<Form />);
    if (method === "select") fireEvent.change(screen.getByLabelText(text("addScreenshots")), { target: { files: [document] } });
    else if (method === "paste") fireEvent.paste(screen.getByLabelText("Description"), { clipboardData: { files: [document] } });
    else fireEvent.drop(screen.getByLabelText("Description"), { dataTransfer: { files: [document] } });
    const link = await screen.findByRole("link", { name: `${text("downloadAttachment")}: ${document.name}` });
    expect(link).toHaveAttribute("download", document.name);
    expect(link).toHaveAttribute("href", "blob:screenshot");
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Description")).toHaveValue("Keep my draft");
    expect(uploadTaskScreenshot).toHaveBeenCalledWith(auth, document, expect.any(AbortSignal));
    fireEvent.click(screen.getByRole("button", { name: `${text("removeScreenshot")} 1` }));
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("rejects mismatched, oversized, and excess files before sending a request", async () => {
    render(<Form initial={["one", "two", "three", "four"]} />);
    const picker = screen.getByLabelText(text("addScreenshots"));
    fireEvent.change(picker, { target: { files: [new File(["svg"], "screen.png", { type: "image/svg+xml" })] } });
    expect(screen.getByRole("alert")).toHaveTextContent(text("screenshotInvalid"));
    const large = file(); Object.defineProperty(large, "size", { value: 10 * 1024 * 1024 + 1 });
    fireEvent.change(picker, { target: { files: [large] } });
    expect(screen.getByRole("alert")).toHaveTextContent(text("screenshotInvalid"));
    fireEvent.change(picker, { target: { files: [file(), file()] } });
    expect(screen.getByRole("alert")).toHaveTextContent(text("screenshotLimit"));
    expect(uploadTaskScreenshot).not.toHaveBeenCalled();
    await screen.findAllByRole("img");
  });

  it("preserves the draft and existing images after an upload fails", async () => {
    vi.mocked(uploadTaskScreenshot).mockRejectedValue(new Error("Upload failed"));
    render(<Form initial={["existing"]} />);
    fireEvent.change(screen.getByLabelText(text("addScreenshots")), { target: { files: [file()] } });
    expect(await screen.findByRole("alert")).toHaveTextContent("Upload failed");
    expect(screen.getByLabelText("Description")).toHaveValue("Keep my draft");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    expect(screen.getByRole("img")).toBeInTheDocument();
  });

  it.each([400, 415, 413, 429, 503])("localizes a rejected upload (%s) and preserves the draft", async (status) => {
    vi.mocked(uploadTaskScreenshot).mockRejectedValue(new ApiRequestError("Internal English rejection", status));
    const ru = taskWorkspaceText("ru");
    render(<TaskScreenshots auth={auth} value={[]} text={ru} onChange={vi.fn()}><textarea aria-label="Description" defaultValue="Мой черновик" /></TaskScreenshots>);
    fireEvent.change(screen.getByLabelText(ru("addScreenshots")), { target: { files: [file()] } });
    const expected = status === 413 ? "screenshotStorageLimit" : status === 429 || status === 503 ? "screenshotCheckUnavailable" : "screenshotRejected";
    expect(await screen.findByRole("alert")).toHaveTextContent(ru(expected));
    expect(screen.getByLabelText("Description")).toHaveValue("Мой черновик");
    expect(screen.getByRole("button", { name: ru("addScreenshots") })).toBeEnabled();
  });

  it("shows saved images to a reader and allows retrying a failed preview", async () => {
    vi.mocked(downloadTaskScreenshot).mockRejectedValueOnce(new Error("Offline"));
    render(<Form initial={["existing"]} readOnly />);
    fireEvent.click(await screen.findByRole("button", { name: new RegExp(text("retry")) }));
    expect(await screen.findByRole("img")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: text("addScreenshots") })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Remove attachment/ })).not.toBeInTheDocument();
  });

  it("aborts an upload when its draft is closed", async () => {
    vi.mocked(uploadTaskScreenshot).mockReturnValue(new Promise(() => {}));
    const { unmount } = render(<Form />);
    fireEvent.change(screen.getByLabelText(text("addScreenshots")), { target: { files: [file()] } });
    const signal = vi.mocked(uploadTaskScreenshot).mock.calls[0][2];
    unmount();
    await waitFor(() => expect(signal?.aborted).toBe(true));
  });
});
