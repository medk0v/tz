import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { I18nContext, createI18n } from "../i18n";
import { KnowledgeBaseView } from "./KnowledgeBaseView";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";
import { usePageRoute } from "./page-route";

vi.mock("../api", () => ({ managementApiRequest: vi.fn(),
  createKnowledgeArticle: vi.fn(),
  createKnowledgeBase: vi.fn(),
  deleteKnowledgeArticle: vi.fn(),
  deleteKnowledgeBase: vi.fn(),
  generateKnowledgeArticleDraft: vi.fn(),
  listKnowledgeArticleDraftAgents: vi.fn(),
  listKnowledgeArticles: vi.fn(),
  listKnowledgeBases: vi.fn(),
  resolveAvatarUrl: (url: string | null) => url,
  updateKnowledgeArticle: vi.fn(),
  updateKnowledgeBase: vi.fn(),
}));

const projectId = "00000000-0000-4000-8000-000000000001";
const auth: api.OperatorAuth = { kind: "session", projectId };
const savedBase: api.KnowledgeBase = {
  id: "00000000-0000-4000-8000-000000000101",
  name: "Product support",
  description: "Product support policies",
  status: "active",
  article_count: 0,
  published_article_count: 0,
  created_at: "2026-08-27T00:00:00Z",
  updated_at: "2026-08-27T00:00:00Z",
};

const publishedArticle: api.KnowledgeArticle = {
  id: "00000000-0000-4000-8000-000000000201",
  knowledge_base_id: savedBase.id,
  title: "Usage terms",
  body: "**Published answer** for customers.",
  status: "published",
  source_url: "https://example.com/terms",
  version: 3,
  published_at: "2026-09-01T00:00:00Z",
  created_at: "2026-08-28T00:00:00Z",
  updated_at: "2026-09-01T00:00:00Z",
};

const draftArticle: api.KnowledgeArticle = {
  id: "00000000-0000-4000-8000-000000000202",
  knowledge_base_id: savedBase.id,
  title: "Refund procedure",
  body: "Internal refund steps.",
  status: "draft",
  source_url: null,
  version: 1,
  published_at: null,
  created_at: "2026-09-02T00:00:00Z",
  updated_at: "2026-09-02T00:00:00Z",
};

const otherBase: api.KnowledgeBase = { ...savedBase, id: "00000000-0000-4000-8000-000000000102", name: "Sales playbook" };

/** Moves the page URL as Back/Forward or a pasted link would. */
function RouteTo({ segments }: { segments: string[] }) {
  const route = usePageRoute();
  return <button type="button" onClick={() => void route.navigate(segments)}>{`Open /${segments.join("/")}`}</button>;
}

function renderRouted(segments: string[], moves: string[][] = []) {
  return render(
    <I18nContext.Provider value={createI18n("en", vi.fn())}>
      <MemoryPageRoute initialSegments={segments}>
        <KnowledgeBaseView auth={auth} projectId={projectId} />
        <CurrentPageRoute />
        {moves.map((move) => <RouteTo key={move.join("/")} segments={move} />)}
      </MemoryPageRoute>
    </I18nContext.Provider>,
  );
}

const pageRoute = () => screen.getByTestId("page-route");

describe("KnowledgeBaseView", () => {
  beforeEach(() => {
    vi.mocked(api.managementApiRequest).mockResolvedValue({ projects: [], departments: [] });
    vi.mocked(api.listKnowledgeBases)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([savedBase]);
    vi.mocked(api.listKnowledgeArticles).mockResolvedValue([]);
    vi.mocked(api.createKnowledgeBase).mockResolvedValue(savedBase);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("creates a knowledge base without language metadata", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <KnowledgeBaseView auth={auth} projectId={projectId} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "New knowledge base" }));
    expect(screen.queryByLabelText("Default language")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: " Product support " } });
    fireEvent.change(screen.getByLabelText("Description"), {
      target: { value: "Product support policies" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save knowledge base" }));

    await waitFor(() => expect(api.createKnowledgeBase).toHaveBeenCalledWith(auth, {
      name: "Product support",
      description: "Product support policies",
      status: "active",
    }));
  });

  it("opens the first material in the editor without a preview mode", async () => {
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([{ ...savedBase, article_count: 2, published_article_count: 1 }]);
    vi.mocked(api.listKnowledgeArticles).mockReset().mockResolvedValue([publishedArticle, draftArticle]);

    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <KnowledgeBaseView auth={auth} projectId={projectId} />
      </I18nContext.Provider>,
    );

    expect(await screen.findByDisplayValue("Usage terms")).toBeInTheDocument();
    expect(screen.getByText("Version 3")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Content" })).toHaveValue(publishedArticle.body);
    expect(screen.queryByRole("button", { name: "Preview" })).not.toBeInTheDocument();
  });

  it("drafts material content from its title with a chosen agent and continues it on request", async () => {
    const first = "## Terms\n\nProcessing takes [to confirm: processing time].";
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([{ ...savedBase, article_count: 2, published_article_count: 1 }]);
    vi.mocked(api.listKnowledgeArticles).mockReset().mockResolvedValue([publishedArticle, draftArticle]);
    vi.mocked(api.listKnowledgeArticleDraftAgents).mockResolvedValue([{ id: "agent-1", name: "Exchange assistant", avatar_url: null }]);
    vi.mocked(api.generateKnowledgeArticleDraft)
      .mockResolvedValueOnce({ body: first, body_format: "markdown" })
      .mockResolvedValueOnce({ body: "## Exceptions", body_format: "markdown" });

    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <KnowledgeBaseView auth={auth} projectId={projectId} />
      </I18nContext.Provider>,
    );

    await screen.findByDisplayValue("Usage terms");
    fireEvent.click(screen.getByRole("button", { name: "New material" }));
    expect(screen.queryByRole("button", { name: "Generate with AI" })).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Exchange times" } });
    fireEvent.click(screen.getByRole("button", { name: "Generate with AI" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Exchange assistant" }));

    const content = screen.getByRole("textbox", { name: "Content" });
    await waitFor(() => expect(content).toHaveValue(first));
    expect(api.listKnowledgeArticleDraftAgents).toHaveBeenCalledWith(auth, savedBase.id, expect.any(AbortSignal));
    expect(api.generateKnowledgeArticleDraft).toHaveBeenCalledWith(auth, savedBase.id, "agent-1", "Exchange times", "", expect.any(AbortSignal));
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save material" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Generate with AI" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Exchange assistant" }));
    await waitFor(() => expect(content).toHaveValue(`${first}\n\n## Exceptions`));
    expect(api.generateKnowledgeArticleDraft).toHaveBeenLastCalledWith(auth, savedBase.id, "agent-1", "Exchange times", first, expect.any(AbortSignal));
  });

  it("opens the requested restored base when it is available", async () => {
    const restoredBase = { ...savedBase, id: "00000000-0000-4000-8000-000000000102", name: "Restored support" };
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([savedBase, restoredBase]);

    renderRouted([restoredBase.id]);

    await waitFor(() => expect(api.listKnowledgeArticles).toHaveBeenCalledWith(auth, restoredBase.id));
    expect(screen.getByRole("combobox", { name: "Project knowledge bases" })).toHaveValue(restoredBase.id);
    expect(api.listKnowledgeArticles).not.toHaveBeenCalledWith(auth, savedBase.id);
    expect(pageRoute().textContent).toBe(restoredBase.id);
  });

  it("falls back to the first base without requesting an unavailable URL selection", async () => {
    const unavailableBaseId = "00000000-0000-4000-8000-000000000199";
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([savedBase]);

    renderRouted([unavailableBaseId]);

    await waitFor(() => expect(api.listKnowledgeArticles).toHaveBeenCalledWith(auth, savedBase.id));
    expect(screen.getByRole("combobox", { name: "Project knowledge bases" })).toHaveValue(savedBase.id);
    expect(api.listKnowledgeArticles).not.toHaveBeenCalledWith(auth, unavailableBaseId);
    await waitFor(() => expect(pageRoute().textContent).toBe(""));
  });

  it("opens the material named by the URL and keeps the URL current", async () => {
    const created: api.KnowledgeArticle = { ...draftArticle, id: "00000000-0000-4000-8000-000000000203", title: "Delivery times", body: "Two days." };
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([{ ...savedBase, article_count: 2, published_article_count: 1 }, otherBase]);
    vi.mocked(api.listKnowledgeArticles).mockReset().mockImplementation(async (_auth, baseId) => baseId === savedBase.id ? [publishedArticle, draftArticle] : []);
    vi.mocked(api.createKnowledgeArticle).mockResolvedValue(created);

    renderRouted([savedBase.id, draftArticle.id]);

    expect(await screen.findByDisplayValue("Refund procedure")).toBeInTheDocument();
    expect(screen.getByTitle("Refund procedure")).toHaveAttribute("aria-current", "true");
    expect(pageRoute().textContent).toBe(`${savedBase.id}/${draftArticle.id}`);
    fireEvent.click(screen.getByTitle("Usage terms"));
    expect(screen.getByDisplayValue("Usage terms")).toBeInTheDocument();
    expect(pageRoute().textContent).toBe(`${savedBase.id}/${publishedArticle.id}`);

    fireEvent.click(screen.getByRole("button", { name: "New material" }));
    expect(screen.getByRole("heading", { name: "Create material" })).toBeInTheDocument();
    expect(pageRoute().textContent).toBe(`${savedBase.id}/new`);
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Delivery times" } });
    fireEvent.click(screen.getByRole("button", { name: "Save material" }));
    expect(await screen.findByText("Material saved.")).toBeInTheDocument();
    expect(api.createKnowledgeArticle).toHaveBeenCalledWith(auth, savedBase.id, expect.objectContaining({ title: "Delivery times" }));
    expect(pageRoute().textContent).toBe(`${savedBase.id}/${created.id}`);
    expect(screen.getByTitle("Delivery times")).toHaveAttribute("aria-current", "true");

    fireEvent.change(screen.getByRole("combobox", { name: "Project knowledge bases" }), { target: { value: otherBase.id } });
    await waitFor(() => expect(api.listKnowledgeArticles).toHaveBeenCalledWith(auth, otherBase.id));
    expect(pageRoute().textContent).toBe(otherBase.id);
    expect(await screen.findByText("No materials in this knowledge base.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "New knowledge base" }));
    expect(screen.getByRole("heading", { name: "Create knowledge base" })).toBeInTheDocument();
    expect(pageRoute().textContent).toBe("new");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(pageRoute().textContent).toBe(otherBase.id);
    expect(await screen.findByRole("combobox", { name: "Project knowledge bases" })).toHaveValue(otherBase.id);
  });

  it("opens creation forms named by the URL and names a saved base in it", async () => {
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValueOnce([otherBase]).mockResolvedValue([otherBase, savedBase]);
    renderRouted(["new", "extra"]);

    expect(await screen.findByRole("heading", { name: "Create knowledge base" })).toBeInTheDocument();
    await waitFor(() => expect(pageRoute().textContent).toBe("new"));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Product support" } });
    fireEvent.click(screen.getByRole("button", { name: "Save knowledge base" }));
    expect(await screen.findByText("Knowledge base saved.")).toBeInTheDocument();
    expect(pageRoute().textContent).toBe(savedBase.id);
    expect(screen.getByRole("combobox", { name: "Project knowledge bases" })).toHaveValue(savedBase.id);
    cleanup();

    renderRouted([otherBase.id, "new"]);
    expect(await screen.findByRole("heading", { name: "Create material" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Project knowledge bases" })).toHaveValue(otherBase.id);
    expect(pageRoute().textContent).toBe(`${otherBase.id}/new`);
  });

  it("follows Back and Forward to another material with its saved content", async () => {
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([savedBase]);
    vi.mocked(api.listKnowledgeArticles).mockReset().mockResolvedValue([publishedArticle, draftArticle]);
    renderRouted([], [[savedBase.id, draftArticle.id], []]);

    const title = await screen.findByDisplayValue("Usage terms");
    fireEvent.change(title, { target: { value: "Unsaved title" } });
    fireEvent.click(screen.getByRole("button", { name: `Open /${savedBase.id}/${draftArticle.id}` }));
    expect(screen.getByDisplayValue("Refund procedure")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open /" }));
    expect(screen.getByDisplayValue("Usage terms")).toBeInTheDocument();
    expect(screen.queryByDisplayValue("Unsaved title")).not.toBeInTheDocument();
  });

  it("reports unsaved changes to the shell, which asks before Back/Forward discards them", async () => {
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([savedBase]);
    vi.mocked(api.listKnowledgeArticles).mockReset().mockResolvedValue([publishedArticle]);
    const onDirtyChange = vi.fn();
    const { unmount } = render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <KnowledgeBaseView auth={auth} projectId={projectId} onDirtyChange={onDirtyChange} />
      </I18nContext.Provider>,
    );

    fireEvent.change(await screen.findByDisplayValue("Usage terms"), { target: { value: "Unsaved title" } });
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(true));
    fireEvent.change(screen.getByDisplayValue("Unsaved title"), { target: { value: "Usage terms" } });
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(false));
    fireEvent.change(screen.getByDisplayValue("Usage terms"), { target: { value: "Unsaved again" } });
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(true));
    unmount();
    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
  });

  it.each([
    [[savedBase.id, "missing"], savedBase.id],
    [["missing", draftArticle.id], ""],
    [[savedBase.id, publishedArticle.id, "extra"], `${savedBase.id}/${publishedArticle.id}`],
  ])("replaces the stale knowledge base URL %j with what it shows", async (segments, canonical) => {
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([savedBase]);
    vi.mocked(api.listKnowledgeArticles).mockReset().mockResolvedValue([publishedArticle, draftArticle]);
    renderRouted(segments);

    expect(await screen.findByDisplayValue("Usage terms")).toBeInTheDocument();
    await waitFor(() => expect(pageRoute().textContent).toBe(canonical));
  });

  it("keeps a material link when its materials cannot be loaded", async () => {
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([savedBase]);
    vi.mocked(api.listKnowledgeArticles).mockReset().mockRejectedValue(new Error("offline"));
    renderRouted([savedBase.id, draftArticle.id]);

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load materials.");
    expect(pageRoute().textContent).toBe(`${savedBase.id}/${draftArticle.id}`);
  });

  it("clears the previous project's base and material while the next project loads", async () => {
    vi.mocked(api.listKnowledgeBases).mockReset()
      .mockResolvedValueOnce([savedBase])
      .mockImplementationOnce(() => new Promise(() => {}));
    vi.mocked(api.listKnowledgeArticles).mockReset().mockResolvedValue([publishedArticle]);
    const view = render(<I18nContext.Provider value={createI18n("en", vi.fn())}><KnowledgeBaseView auth={auth} projectId={projectId} /></I18nContext.Provider>);
    await screen.findByDisplayValue("Usage terms");
    fireEvent.click(screen.getByRole("button", { name: "Knowledge base settings" }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Unsaved project name" } });

    const nextProjectId = "00000000-0000-4000-8000-000000000002";
    const nextAuth: api.OperatorAuth = { kind: "session", projectId: nextProjectId };
    view.rerender(<I18nContext.Provider value={createI18n("en", vi.fn())}><KnowledgeBaseView auth={nextAuth} projectId={nextProjectId} /></I18nContext.Provider>);

    expect(screen.queryByDisplayValue("Usage terms")).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("Unsaved project name")).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "Project knowledge bases" })).not.toBeInTheDocument();
    expect(api.listKnowledgeBases).toHaveBeenLastCalledWith(nextAuth);
    expect(api.listKnowledgeArticles).not.toHaveBeenCalledWith(nextAuth, savedBase.id);
  });

  it("discards materials that finish loading after switching projects", async () => {
    let finishArticles!: (items: api.KnowledgeArticle[]) => void;
    vi.mocked(api.listKnowledgeBases).mockReset()
      .mockResolvedValueOnce([savedBase])
      .mockResolvedValueOnce([]);
    vi.mocked(api.listKnowledgeArticles).mockReset().mockImplementationOnce(() => new Promise((resolve) => { finishArticles = resolve; }));
    const view = render(<I18nContext.Provider value={createI18n("en", vi.fn())}><KnowledgeBaseView auth={auth} projectId={projectId} /></I18nContext.Provider>);
    await waitFor(() => expect(api.listKnowledgeArticles).toHaveBeenCalledWith(auth, savedBase.id));

    const nextProjectId = "00000000-0000-4000-8000-000000000002";
    const nextAuth: api.OperatorAuth = { kind: "session", projectId: nextProjectId };
    view.rerender(<I18nContext.Provider value={createI18n("en", vi.fn())}><KnowledgeBaseView auth={nextAuth} projectId={nextProjectId} /></I18nContext.Provider>);
    await waitFor(() => expect(api.listKnowledgeBases).toHaveBeenLastCalledWith(nextAuth));
    await act(async () => { finishArticles([publishedArticle]); });

    expect(screen.queryByDisplayValue("Usage terms")).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "Project knowledge bases" })).not.toBeInTheDocument();
    expect(api.listKnowledgeArticles).not.toHaveBeenCalledWith(nextAuth, savedBase.id);
  });

  it("returns to the current base when new-base creation is canceled", async () => {
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([savedBase]);

    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <KnowledgeBaseView auth={auth} projectId={projectId} />
      </I18nContext.Provider>,
    );

    await screen.findByRole("searchbox", { name: "Search materials" });
    fireEvent.click(screen.getByRole("button", { name: "New knowledge base" }));
    expect(screen.getByRole("heading", { name: "Create knowledge base" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(await screen.findByRole("searchbox", { name: "Search materials" })).toBeInTheDocument();
  });

  it("searches and filters materials without losing the selected editor", async () => {
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([{ ...savedBase, article_count: 2, published_article_count: 1 }]);
    vi.mocked(api.listKnowledgeArticles).mockReset().mockResolvedValue([publishedArticle, draftArticle]);

    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <KnowledgeBaseView auth={auth} projectId={projectId} />
      </I18nContext.Provider>,
    );

    await screen.findByDisplayValue("Usage terms");
    fireEvent.change(screen.getByRole("searchbox", { name: "Search materials" }), { target: { value: "refund" } });
    expect(screen.getByTitle("Refund procedure")).toBeInTheDocument();
    expect(screen.queryByTitle("Usage terms")).not.toBeInTheDocument();
    expect(screen.getByDisplayValue("Usage terms")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Published" }));
    expect(screen.getByText("No materials match your search and filters.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(screen.getByTitle("Usage terms")).toBeInTheDocument();
    expect(screen.getByTitle("Refund procedure")).toBeInTheDocument();
  });

  it.each(["article", "base"] as const)("requires explicit confirmation before deleting a %s", async (kind) => {
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([savedBase]);
    vi.mocked(api.listKnowledgeArticles).mockReset().mockResolvedValue([publishedArticle, draftArticle]);
    vi.mocked(api.deleteKnowledgeArticle).mockResolvedValue(undefined);
    vi.mocked(api.deleteKnowledgeBase).mockResolvedValue(undefined);
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><KnowledgeBaseView auth={auth} projectId={projectId} /></I18nContext.Provider>);
    await screen.findByDisplayValue("Usage terms");
    if (kind === "base") fireEvent.click(screen.getByRole("button", { name: "Knowledge base settings" }));
    const label = kind === "article" ? "Delete material" : "Delete knowledge base";

    fireEvent.click(screen.getByRole("button", { name: label }));
    const confirmation = screen.getByRole("alert");
    expect(confirmation).toHaveTextContent(kind === "article" ? "Usage terms" : "Product support");
    if (kind === "base") expect(confirmation).toHaveTextContent("all its materials");
    expect(within(confirmation).getByRole("button", { name: "Cancel" })).toHaveFocus();
    expect(api.deleteKnowledgeArticle).not.toHaveBeenCalled();
    expect(api.deleteKnowledgeBase).not.toHaveBeenCalled();

    fireEvent.click(within(confirmation).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: label })).toHaveFocus();
    expect(api.deleteKnowledgeArticle).not.toHaveBeenCalled();
    expect(api.deleteKnowledgeBase).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: label }));
    fireEvent.click(within(screen.getByRole("alert")).getByRole("button", { name: label }));
    if (kind === "article") {
      await waitFor(() => expect(api.deleteKnowledgeArticle).toHaveBeenCalledExactlyOnceWith(auth, savedBase.id, publishedArticle.id));
      expect(await screen.findByDisplayValue("Refund procedure")).toBeInTheDocument();
      expect(api.deleteKnowledgeBase).not.toHaveBeenCalled();
    } else {
      await waitFor(() => expect(api.deleteKnowledgeBase).toHaveBeenCalledExactlyOnceWith(auth, savedBase.id));
      expect(api.deleteKnowledgeArticle).not.toHaveBeenCalled();
    }
    await waitFor(() => expect(screen.queryByText(kind === "article" ? "Delete material “Usage terms”?" : "Delete knowledge base “Product support” and all its materials?")).not.toBeInTheDocument());
  });

  it("cancels deletion with Escape and clears confirmation when switching materials", async () => {
    vi.mocked(api.listKnowledgeBases).mockReset().mockResolvedValue([savedBase]);
    vi.mocked(api.listKnowledgeArticles).mockReset().mockResolvedValue([publishedArticle, draftArticle]);
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><KnowledgeBaseView auth={auth} projectId={projectId} /></I18nContext.Provider>);
    await screen.findByDisplayValue("Usage terms");
    fireEvent.click(screen.getByRole("button", { name: "Delete material" }));
    fireEvent.keyDown(within(screen.getByRole("alert")).getByRole("button", { name: "Cancel" }), { key: "Escape" });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete material" })).toHaveFocus();

    fireEvent.click(screen.getByRole("button", { name: "Delete material" }));
    fireEvent.click(screen.getByTitle("Refund procedure"));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Delete material" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Refund procedure");
    expect(api.deleteKnowledgeArticle).not.toHaveBeenCalled();
  });
});
