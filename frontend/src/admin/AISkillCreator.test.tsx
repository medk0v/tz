import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiRequestError, type AiProvider } from "../api";
import * as api from "../ai-skills-api";
import { createI18n, I18nContext, type Locale } from "../i18n";
import { AISkillCreator } from "./AISkillCreator";
import { DemoReadOnlyContext } from "./DemoReadOnly";

vi.mock("../ai-skills-api", () => ({ generateAiSkill: vi.fn() }));

const auth = { kind: "session" } as const;
const provider: AiProvider = { id: "provider-1", name: "Project AI", provider_kind: "openai", base_url: "https://api.openai.com/v1", default_model: "model-with-vision", status: "active", api_key_configured: true, created_at: "", updated_at: "" };
const generated = { name: "Guide review", description: "Apply source guidance", instructions: "Use the supplied guidance to review the request." };

function show({ providers = [provider], demo = false, locale = "en" }: { providers?: AiProvider[]; demo?: boolean; locale?: Locale } = {}) {
  const callbacks = { onGenerated: vi.fn(), onCancel: vi.fn(), onDirtyChange: vi.fn(), onBusyChange: vi.fn() };
  const rendered = render(<I18nContext.Provider value={createI18n(locale, vi.fn())}><DemoReadOnlyContext.Provider value={demo}>
    <AISkillCreator auth={demo ? { ...auth, isDemo: true } : auth} providers={providers} providersLoading={false} {...callbacks} />
  </DemoReadOnlyContext.Provider></I18nContext.Provider>);
  return { ...rendered, ...callbacks };
}

function setGoal(value = " Create a review skill ") {
  fireEvent.change(screen.getByRole("textbox", { name: "What should the skill do?" }), { target: { value } });
}

function addFiles(...files: File[]) {
  fireEvent.change(screen.getByLabelText("Source files", { selector: "input" }), { target: { files } });
}

describe("AI skill creator", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.generateAiSkill).mockResolvedValue(generated);
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

  it("requires a goal and sends pasted text, books and images only when generation is requested", async () => {
    const { onGenerated, onBusyChange } = show();
    expect(screen.getByRole("button", { name: "Generate instructions" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "What should the skill do?" })).toHaveFocus();
    expect(api.generateAiSkill).not.toHaveBeenCalled();
    setGoal();
    fireEvent.change(screen.getByRole("textbox", { name: "Source text (optional)" }), { target: { value: "Original pasted text" } });
    const pdf = new File(["book text"], "book.pdf", { type: "application/pdf" });
    const image = new File(["image data"], "diagram.png", { type: "image/png" });
    const removed = new File(["draft"], "unused.md", { type: "text/markdown" });
    addFiles(pdf, image, removed);
    fireEvent.click(screen.getByRole("button", { name: "Remove unused.md" }));
    expect(screen.queryByText("unused.md")).not.toBeInTheDocument();
    expect(api.generateAiSkill).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Generate instructions" }));
    await waitFor(() => expect(onGenerated).toHaveBeenCalledWith(generated));
    expect(api.generateAiSkill).toHaveBeenCalledWith(auth, { provider_connection_id: provider.id, goal: "Create a review skill", source_text: "Original pasted text", files: [pdf, image] }, expect.any(AbortSignal));
    expect(onBusyChange).toHaveBeenCalledWith(true);
    expect(onBusyChange).toHaveBeenLastCalledWith(false);
  });

  it("supports goal-only generation and allows selection among active compatible connections", async () => {
    const { onGenerated } = show({ providers: [provider, { ...provider, id: "local-model", name: "Local model", provider_kind: "openai_compatible" }, { ...provider, id: "disabled", name: "Disabled", status: "disabled" }, { ...provider, id: "anthropic", name: "Anthropic", provider_kind: "anthropic" }, { ...provider, id: "no-model", name: "No model", default_model: " " }] });
    expect(screen.getAllByRole("option")).toHaveLength(2);
    setGoal();
    fireEvent.change(screen.getByRole("combobox", { name: "Model connection" }), { target: { value: "local-model" } });
    fireEvent.click(screen.getByRole("button", { name: "Generate instructions" }));
    await waitFor(() => expect(onGenerated).toHaveBeenCalled());
    expect(api.generateAiSkill).toHaveBeenCalledWith(auth, expect.objectContaining({ provider_connection_id: "local-model", files: [], source_text: "" }), expect.any(AbortSignal));
  });

  it("explains missing connections and prevents generation", () => {
    show({ providers: [] });
    setGoal();
    expect(screen.getByText(/Add an active OpenAI or OpenAI-compatible connection/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Generate instructions" })).toBeDisabled();
    expect(api.generateAiSkill).not.toHaveBeenCalled();
  });

  it.each([415, 422])("shows source guidance and preserves all form data after error %s for retry", async (status) => {
    vi.mocked(api.generateAiSkill).mockRejectedValueOnce(new ApiRequestError("book.pdf has no readable text.", status));
    const { onGenerated } = show();
    setGoal();
    fireEvent.change(screen.getByRole("textbox", { name: "Source text (optional)" }), { target: { value: "Keep this text" } });
    const book = new File(["book"], "book.pdf");
    addFiles(book);
    fireEvent.click(screen.getByRole("button", { name: "Generate instructions" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("book.pdf has no readable text.");
    expect(screen.getByRole("textbox", { name: "What should the skill do?" })).toHaveValue(" Create a review skill ");
    expect(screen.getByRole("textbox", { name: "Source text (optional)" })).toHaveValue("Keep this text");
    expect(screen.getByText("book.pdf")).toBeInTheDocument();
    expect(onGenerated).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Generate instructions" }));
    await waitFor(() => expect(onGenerated).toHaveBeenCalledWith(generated));
    expect(api.generateAiSkill).toHaveBeenCalledTimes(2);
  });

  it("cancels an in-flight request, keeps sources, and ignores a late result", async () => {
    let finish!: (result: api.AiSkillGenerationResult) => void;
    vi.mocked(api.generateAiSkill).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const { onGenerated, onBusyChange } = show();
    setGoal();
    addFiles(new File(["reference"], "reference.txt"));
    fireEvent.click(screen.getByRole("button", { name: "Generate instructions" }));
    const signal = vi.mocked(api.generateAiSkill).mock.calls[0][2];
    expect(screen.getByRole("textbox", { name: "What should the skill do?" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Cancel generation" }));
    expect(signal?.aborted).toBe(true);
    expect(screen.getByText("reference.txt")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Generate instructions" })).toBeEnabled();
    expect(onBusyChange).toHaveBeenLastCalledWith(false);
    await act(async () => finish(generated));
    expect(onGenerated).not.toHaveBeenCalled();
  });

  it("aborts generation when unmounted", () => {
    vi.mocked(api.generateAiSkill).mockImplementationOnce(() => new Promise(() => {}));
    const { unmount, onGenerated } = show();
    setGoal();
    fireEvent.click(screen.getByRole("button", { name: "Generate instructions" }));
    const signal = vi.mocked(api.generateAiSkill).mock.calls[0][2];
    unmount();
    expect(signal?.aborted).toBe(true);
    expect(onGenerated).not.toHaveBeenCalled();
  });

  it("rejects unsupported, empty, oversized and excess sources without replacing accepted files", () => {
    show();
    addFiles(new File(["notes"], "notes.md"));
    for (const file of [new File(["exe"], "program.exe"), new File([], "empty.pdf")]) {
      addFiles(file);
      expect(screen.getByRole("alert")).toHaveTextContent("Choose a non-empty");
    }
    const image = new File(["image"], "large.png");
    Object.defineProperty(image, "size", { value: 10 * 1024 * 1024 + 1 });
    addFiles(image);
    expect(screen.getByRole("alert")).toHaveTextContent("File is too large");
    const document = new File(["book"], "large.pdf");
    Object.defineProperty(document, "size", { value: 20 * 1024 * 1024 + 1 });
    addFiles(document);
    expect(screen.getByRole("alert")).toHaveTextContent("File is too large");
    addFiles(...Array.from({ length: 10 }, (_, index) => new File(["page"], `book-${index}.pdf`)));
    expect(screen.getByRole("alert")).toHaveTextContent("up to 10 files");
    const books = ["one.pdf", "two.epub"].map((name) => {
      const file = new File(["book"], name);
      Object.defineProperty(file, "size", { value: 16 * 1024 * 1024 });
      return file;
    });
    addFiles(...books);
    expect(screen.getByRole("alert")).toHaveTextContent("must not exceed 30 MB");
    expect(screen.getByText("notes.md")).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(api.generateAiSkill).not.toHaveBeenCalled();
  });

  it("blocks generation and source changes for demo users", () => {
    show({ demo: true });
    expect(screen.getByRole("textbox", { name: "What should the skill do?" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Add files" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Generate instructions" })).toBeDisabled();
    expect(api.generateAiSkill).not.toHaveBeenCalled();
  });

  it.each([["ru", "Что должен делать скилл?", "Подготовить инструкции"], ["ro", "Ce trebuie să facă abilitatea?", "Generează instrucțiuni"]] as const)("localizes creation controls in %s", (locale, goalLabel, generateLabel) => {
    show({ locale });
    expect(screen.getByRole("textbox", { name: goalLabel })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: generateLabel })).toBeDisabled();
  });
});
