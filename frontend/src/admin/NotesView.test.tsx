import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiRequestError } from "../api";
import * as api from "../notes-api";
import * as shareApi from "../note-shares-api";
import { createI18n, I18nContext, type Locale } from "../i18n";
import { NotesView } from "./NotesView";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";

vi.mock("../notes-api", () => ({ listNotes: vi.fn(), getNote: vi.fn(), createNote: vi.fn(), moveNote: vi.fn(), updateNote: vi.fn(), deleteNote: vi.fn() }));
vi.mock("./NoteDatabaseEmbeds", () => ({ NoteDatabaseEmbeds: ({ suspended, onDirtyChange }: { suspended?: boolean; onDirtyChange: (dirty: boolean) => void }) => <div data-testid="embedded-tables" data-suspended={suspended || undefined}><input aria-label="Embedded table draft" defaultValue="" onChange={() => onDirtyChange(true)} /></div> }));
vi.mock("../note-shares-api", async (original) => ({ ...await original<typeof import("../note-shares-api")>(), listNoteShares: vi.fn() }));
vi.mock("./NoteEditor", () => ({ default: ({ value, onChange, readOnly }: { value: string; onChange: (value: string) => void; readOnly: boolean }) =>
  <textarea aria-label="Page content" value={value} readOnly={readOnly} onChange={(event) => onChange(event.target.value)} /> }));

const auth = { kind: "session" } as const;
const scopedAuth = { ...auth, projectId: "project-a" };
const rootId = "10000000-0000-4000-8000-000000000001";
const childId = "10000000-0000-4000-8000-000000000002";
const otherId = "10000000-0000-4000-8000-000000000003";
const newId = "10000000-0000-4000-8000-000000000004";
const note: api.Note = {
  id: rootId, title: "Project handbook", body: "# Welcome\nProject decisions.", parent_id: null, icon: "📚", is_favorite: false,
  version: 3, sort_order: 0, created_at: "2026-09-21T09:00:00Z", updated_at: "2026-09-21T12:00:00Z",
};
const child: api.Note = { ...note, id: childId, title: "Launch checklist", body: "- [ ] Ship", parent_id: rootId, is_favorite: true };
const other: api.Note = { ...note, id: otherId, title: "Team ideas", body: "Discuss next steps.", parent_id: null, icon: "💡", sort_order: 1 };

function show({ segments = [], canWrite = true, locale = "en", onDirtyChange = vi.fn() }: { segments?: string[]; canWrite?: boolean; locale?: Locale; onDirtyChange?: (dirty: boolean) => void } = {}) {
  return render(<I18nContext.Provider value={createI18n(locale, vi.fn())}><MemoryPageRoute initialSegments={segments}>
    <NotesView auth={auth} projectId="project-a" canWrite={canWrite} onDirtyChange={onDirtyChange} /><CurrentPageRoute />
  </MemoryPageRoute></I18nContext.Provider>);
}

async function loaded() { await waitFor(() => expect(screen.getByRole("button", { name: "Refresh pages" })).toBeEnabled()); }
const pageRoute = () => screen.getByTestId("page-route").textContent;

const transfer = () => ({ setData: vi.fn(), effectAllowed: "", dropEffect: "" });
function treeRow(title: string) { return screen.getByRole("button", { name: `Move ${title}` }).closest<HTMLElement>(".notes-tree-row")!; }
function dragEvent(target: HTMLElement, kind: "dragenter" | "dragover" | "drop", dataTransfer: ReturnType<typeof transfer>, position: "before" | "inside" | "after" = "inside") {
  vi.spyOn(target, "getBoundingClientRect").mockReturnValue({ top: 100, height: 40 } as DOMRect);
  const event = new MouseEvent(kind, { bubbles: true, cancelable: true, clientY: position === "before" ? 102 : position === "after" ? 138 : 120 });
  Object.defineProperty(event, "dataTransfer", { value: dataTransfer }); fireEvent(target, event); return event;
}
function dragPage(source: string, target: HTMLElement, position: "before" | "inside" | "after" = "inside") {
  const dataTransfer = transfer(); fireEvent.dragStart(screen.getByRole("button", { name: `Move ${source}` }), { dataTransfer });
  dragEvent(target, "dragover", dataTransfer, position); return { dataTransfer, drop: () => dragEvent(target, "drop", dataTransfer, position) };
}

describe("Notes workspace", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    const records = new Map([note, child, other].map((item) => [item.id, item]));
    vi.mocked(api.listNotes).mockResolvedValue([note, child, other]);
    vi.mocked(shareApi.listNoteShares).mockResolvedValue([]);
    vi.mocked(api.getNote).mockImplementation(async (_auth, id) => records.get(id) ?? { ...note, id });
    vi.mocked(api.createNote).mockImplementation(async (_auth, input) => {
      const saved = { ...note, ...input, id: newId, version: 1 }; records.set(newId, saved); return saved;
    });
    vi.mocked(api.updateNote).mockImplementation(async (_auth, id, input, version) => ({ ...note, ...input, id, version: version + 1 }));
    vi.mocked(api.moveNote).mockImplementation(async (_auth, id, input) => {
      const moved = { ...records.get(id)!, parent_id: input.parent_id, version: input.expected_version + 1 };
      const siblings = [...records.values()].filter((item) => item.id !== id && item.parent_id === input.parent_id).sort((a, b) => a.sort_order - b.sort_order);
      const index = input.before_id ? siblings.findIndex((item) => item.id === input.before_id) : siblings.length;
      siblings.splice(index, 0, moved);
      siblings.forEach((item, order) => records.set(item.id, { ...item, sort_order: order }));
      return { note: records.get(id)!, items: [...records.values()] };
    });
    vi.mocked(api.deleteNote).mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

  it("persists before and after drops using precise sibling anchors and indicators", async () => {
    show(); await loaded();
    const before = dragPage("Team ideas", treeRow("Project handbook"), "before");
    expect(treeRow("Project handbook")).toHaveAttribute("data-drop", "before"); expect(screen.getByText("Before Project handbook")).toBeInTheDocument(); before.drop();
    await waitFor(() => expect(api.moveNote).toHaveBeenCalledWith(scopedAuth, otherId, { parent_id: null, before_id: rootId, expected_version: 3 }));
    await screen.findByText("Page moved.");
    const rootList = treeRow("Project handbook").closest("ul")!;
    expect(within(rootList).getAllByRole("button", { name: /^Move / }).map((button) => button.getAttribute("aria-label"))).toEqual(["Move Team ideas", "Move Project handbook", "Move Launch checklist"]);
    const after = dragPage("Launch checklist", treeRow("Team ideas"), "after"); expect(treeRow("Team ideas")).toHaveAttribute("data-drop", "after"); after.drop();
    await waitFor(() => expect(api.moveNote).toHaveBeenLastCalledWith(scopedAuth, childId, { parent_id: null, before_id: rootId, expected_version: 3 }));
  });

  it("nests inside a collapsed page, expands it and moves a child back to root", async () => {
    show(); await loaded(); fireEvent.click(screen.getByRole("button", { name: "Collapse Project handbook" }));
    const nesting = dragPage("Team ideas", treeRow("Project handbook")); expect(treeRow("Project handbook")).toHaveAttribute("data-drop", "inside"); nesting.drop();
    await waitFor(() => expect(api.moveNote).toHaveBeenCalledWith(scopedAuth, otherId, { parent_id: rootId, before_id: null, expected_version: 3 }));
    await screen.findByRole("button", { name: "Collapse Project handbook" });
    expect(treeRow("Team ideas").closest("ul")).toBe(treeRow("Launch checklist").closest("ul"));
    const rootDrop = screen.getByLabelText("Move to top level"); const out = dragPage("Team ideas", rootDrop); expect(rootDrop).toHaveAttribute("data-drop", "true"); out.drop();
    await waitFor(() => expect(api.moveNote).toHaveBeenLastCalledWith(scopedAuth, otherId, { parent_id: null, before_id: null, expected_version: 4 }));
    await waitFor(() => expect(treeRow("Team ideas").closest("ul")).toBe(treeRow("Project handbook").closest("ul")));
  });

  it("accepts a fast drop across row zones while skipping unchanged placements", async () => {
    show(); await loaded();
    const target = treeRow("Project handbook");
    const dataTransfer = transfer();
    fireEvent.dragStart(screen.getByRole("button", { name: "Move Team ideas" }), { dataTransfer });
    expect(dragEvent(target, "dragenter", dataTransfer, "after").defaultPrevented).toBe(true);
    expect(dragEvent(target, "dragover", dataTransfer, "after").defaultPrevented).toBe(true);
    dragEvent(target, "drop", dataTransfer, "after");
    expect(api.moveNote).not.toHaveBeenCalled();
    fireEvent.dragStart(screen.getByRole("button", { name: "Move Team ideas" }), { dataTransfer });
    dragEvent(target, "dragover", dataTransfer, "after");
    expect(dragEvent(target, "dragenter", dataTransfer, "before").defaultPrevented).toBe(true);
    dragEvent(target, "drop", dataTransfer, "before");
    await waitFor(() => expect(api.moveNote).toHaveBeenCalledWith(scopedAuth, otherId,
      { parent_id: null, before_id: rootId, expected_version: 3 }));
  });

  it("rejects self and descendant targets in every drop position", async () => {
    show(); await loaded();
    for (const title of ["Project handbook", "Launch checklist"]) for (const position of ["before", "inside", "after"] as const) {
      const attempt = dragPage("Project handbook", treeRow(title), position); expect(treeRow(title)).not.toHaveAttribute("data-drop"); attempt.drop();
    }
    expect(api.moveNote).not.toHaveBeenCalled();
  });

  it("moves a selected dirty page without resetting Markdown, properties, tables or the editor", async () => {
    show({ segments: [childId] }); const body = await screen.findByRole("textbox", { name: "Page content" }); await loaded();
    fireEvent.change(body, { target: { value: "## Unsaved decisions" } }); fireEvent.change(screen.getByRole("textbox", { name: "Page title" }), { target: { value: "My draft title" } }); fireEvent.click(screen.getByRole("button", { name: "Remove from favorites" }));
    const tableDraft = screen.getByRole("textbox", { name: "Embedded table draft" }); fireEvent.change(tableDraft, { target: { value: "Unsaved table row" } });
    let resolveMove!: (result: api.NoteMoveResult) => void; vi.mocked(api.moveNote).mockImplementationOnce(() => new Promise((resolve) => { resolveMove = resolve; }));
    dragPage("Launch checklist", screen.getByLabelText("Move to top level")).drop();
    await waitFor(() => expect(api.moveNote).toHaveBeenCalled()); expect(screen.getByRole("button", { name: "Save" })).toBeDisabled(); expect(body.closest("form")).toHaveAttribute("inert"); expect(body).not.toHaveAttribute("readonly"); expect(screen.getByTestId("embedded-tables")).toHaveAttribute("data-suspended", "true");
    const moved = { ...child, parent_id: null, version: 4, sort_order: 2 };
    await act(async () => resolveMove({ note: moved, items: [note, other, moved] }));
    expect(screen.getByRole("textbox", { name: "Page content" })).toBe(body); expect(body).toHaveValue("## Unsaved decisions"); expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue("My draft title"); expect(screen.getByRole("button", { name: "Parent page" })).toHaveValue(""); expect(screen.getByRole("button", { name: "Add to favorites" })).toBeInTheDocument(); expect(screen.getByRole("textbox", { name: "Embedded table draft" })).toBe(tableDraft); expect(tableDraft).toHaveValue("Unsaved table row"); expect(window.confirm).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Save" })); await waitFor(() => expect(api.updateNote).toHaveBeenCalledWith(scopedAuth, childId, expect.objectContaining({ parent_id: null, body: "## Unsaved decisions", title: "My draft title", is_favorite: false }), 4));
  });

  it("keeps a different selected page's unsaved parent and body when another page moves", async () => {
    show({ segments: [childId] }); const body = await screen.findByRole("textbox", { name: "Page content" }); await loaded();
    fireEvent.change(body, { target: { value: "Unrelated draft" } }); fireEvent.click(screen.getByRole("button", { name: "Parent page" })); fireEvent.click(screen.getByRole("option", { name: /Team ideas/ }));
    dragPage("Team ideas", treeRow("Project handbook"), "before").drop(); await screen.findByText("Page moved.");
    expect(body).toHaveValue("Unrelated draft"); expect(screen.getByRole("button", { name: "Parent page" })).toHaveValue(otherId); fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.updateNote).toHaveBeenCalledWith(scopedAuth, childId, expect.objectContaining({ parent_id: otherId, body: "Unrelated draft" }), 3));
  });

  it("uses the draft's detail version after the sidebar refreshes, preserving conflict protection", async () => {
    show({ segments: [childId] }); const body = await screen.findByRole("textbox", { name: "Page content" }); await loaded(); fireEvent.change(body, { target: { value: "Draft from version three" } });
    vi.mocked(api.listNotes).mockResolvedValueOnce([note, { ...child, version: 4 }, other]); fireEvent.click(screen.getByRole("button", { name: "Refresh pages" })); await loaded();
    vi.mocked(api.moveNote).mockRejectedValueOnce(new ApiRequestError("note changed", 409)); dragPage("Launch checklist", screen.getByLabelText("Move to top level")).drop();
    await screen.findByText(/Save your draft or reopen/); expect(api.moveNote).toHaveBeenCalledWith(scopedAuth, childId, { parent_id: null, before_id: null, expected_version: 3 }); expect(body).toHaveValue("Draft from version three"); expect(screen.getByRole("button", { name: "Parent page" })).toHaveValue(rootId);
    fireEvent.click(screen.getByRole("button", { name: "Save" })); await waitFor(() => expect(api.updateNote).toHaveBeenCalledWith(scopedAuth, childId, expect.objectContaining({ body: "Draft from version three" }), 3));
  });

  it("leaves order unchanged on network failure and never retries a move automatically", async () => {
    show(); await loaded(); vi.mocked(api.moveNote).mockRejectedValueOnce(new Error("offline")); const beforeOrder = [...document.querySelectorAll(".notes-tree-row")].map((row) => row.getAttribute("data-note-id"));
    dragPage("Team ideas", treeRow("Project handbook"), "before").drop(); await screen.findByText("Could not move the page. The tree and your draft are unchanged.");
    expect([...document.querySelectorAll(".notes-tree-row")].map((row) => row.getAttribute("data-note-id"))).toEqual(beforeOrder); expect(api.moveNote).toHaveBeenCalledTimes(1);
  });

  it("disables dragging in search and favorites and supports keyboard ordering in the full tree", async () => {
    show(); await loaded(); fireEvent.change(screen.getByRole("searchbox", { name: "Search page titles" }), { target: { value: "ideas" } }); expect(screen.getByRole("button", { name: "Move Team ideas" })).toBeDisabled(); expect(screen.getByText(/Clear search and select All pages/)).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search page titles" }), { target: { value: "" } }); fireEvent.click(screen.getByRole("button", { name: "Favorites" })); expect(screen.getByRole("button", { name: "Move Launch checklist" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "All pages" })); fireEvent.keyDown(screen.getByRole("button", { name: "Move Team ideas" }), { key: "ArrowUp", altKey: true }); await waitFor(() => expect(api.moveNote).toHaveBeenCalledWith(scopedAuth, otherId, { parent_id: null, before_id: rootId, expected_version: 3 }));
  });

  it("cancels a drop onto an editor without exposing a text payload or changing the draft", async () => {
    show({ segments: [childId] }); const body = await screen.findByRole("textbox", { name: "Page content" }); await loaded(); const dataTransfer = transfer();
    fireEvent.dragStart(screen.getByRole("button", { name: "Move Team ideas" }), { dataTransfer }); expect(dataTransfer.setData).toHaveBeenCalledWith("application/x-tz-note-id", otherId); expect(dragEvent(body, "drop", dataTransfer).defaultPrevented).toBe(true); expect(api.moveNote).not.toHaveBeenCalled(); expect(body).toHaveValue(child.body);
  });

  it("waits for the open page's detail version before allowing a drag", async () => {
    let resolveDetail!: (value: api.Note) => void;
    vi.mocked(api.getNote).mockImplementationOnce(() => new Promise((resolve) => { resolveDetail = resolve; }));
    show({ segments: [childId] }); await loaded();
    expect(screen.getByRole("button", { name: "Move Launch checklist" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Move Team ideas" })).toBeDisabled();
    await act(async () => resolveDetail(child));
    await waitFor(() => expect(screen.getByRole("button", { name: "Move Launch checklist" })).toBeEnabled());
  });

  it("lists a nested tree, searches children by title and filters favorites", async () => {
    show(); await loaded();
    expect(api.listNotes).toHaveBeenCalledWith(scopedAuth, expect.any(AbortSignal));
    fireEvent.click(screen.getByRole("button", { name: "Collapse Project handbook" }));
    expect(screen.queryByRole("button", { name: /^Launch checklist/ })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search page titles" }), { target: { value: "launch" } });
    expect(screen.getByRole("button", { name: /^Launch checklist/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Team ideas" })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Favorites" }));
    expect(screen.getByRole("button", { name: /^Launch checklist/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Project handbook" })).not.toBeInTheDocument();
  });

  it("creates a project-scoped page and sends edited text with the current version", async () => {
    show(); await loaded();
    fireEvent.click(screen.getAllByRole("button", { name: "New page" })[0]);
    const title = await screen.findByRole("textbox", { name: "Page title" });
    const body = await screen.findByRole("textbox", { name: "Page content" });
    fireEvent.change(title, { target: { value: " Meeting notes " } });
    fireEvent.change(body, { target: { value: "## Decisions\n- [x] Accepted" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.createNote).toHaveBeenCalledWith(scopedAuth, {
      title: "Meeting notes", body: "## Decisions\n- [x] Accepted", parent_id: null, icon: "", is_favorite: false,
    }));
    await waitFor(() => expect(pageRoute()).toBe(newId));
    await screen.findByRole("textbox", { name: "Page content" });
    fireEvent.change(screen.getByRole("textbox", { name: "Page title" }), { target: { value: "Updated notes" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.updateNote).toHaveBeenCalledWith(scopedAuth, newId, expect.objectContaining({ title: "Updated notes" }), 1));
  });

  it("opens a direct link and excludes the current page and descendants from parent choices", async () => {
    show({ segments: [rootId] });
    await screen.findByRole("textbox", { name: "Page title" });
    fireEvent.click(screen.getByRole("button", { name: "Parent page" }));
    const choices = screen.getByRole("listbox", { name: "Parent page" });
    expect(within(choices).queryByRole("option", { name: /Project handbook/ })).not.toBeInTheDocument();
    expect(within(choices).queryByRole("option", { name: /^Launch checklist/ })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("combobox", { name: "Search page titles" }), { target: { value: "IDEAS" } });
    expect(within(choices).queryByRole("option", { name: "Top level" })).not.toBeInTheDocument();
    fireEvent.click(within(choices).getByRole("option", { name: /Team ideas/ }));
    fireEvent.click(screen.getByRole("button", { name: "Add to favorites" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.updateNote).toHaveBeenCalledWith(scopedAuth, rootId,
      expect.objectContaining({ parent_id: otherId, is_favorite: true }), 3));
  });

  it("retains unsaved content when switching pages is cancelled and notifies the outer guard", async () => {
    const onDirtyChange = vi.fn();
    show({ segments: [rootId], onDirtyChange });
    const body = await screen.findByRole("textbox", { name: "Page content" });
    fireEvent.change(body, { target: { value: "Keep my draft" } });
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(true));
    vi.mocked(window.confirm).mockReturnValueOnce(false);
    fireEvent.click(screen.getByRole("button", { name: "Team ideas" }));
    expect(pageRoute()).toBe(rootId);
    expect(body).toHaveValue("Keep my draft");
    expect(window.confirm).toHaveBeenCalledWith("Discard your unsaved changes?");
    fireEvent.click(screen.getByRole("button", { name: "Team ideas" }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue("Team ideas"));
  });

  it("keeps a conflicting draft and saves it separately without overwriting the newer version", async () => {
    show({ segments: [rootId] });
    const body = await screen.findByRole("textbox", { name: "Page content" });
    fireEvent.change(body, { target: { value: "My unsaved revision" } });
    vi.mocked(api.updateNote).mockRejectedValueOnce(new ApiRequestError("note changed", 409));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByRole("button", { name: "Save as a copy" });
    expect(body).toHaveValue("My unsaved revision");
    fireEvent.click(screen.getByRole("button", { name: "Save as a copy" }));
    await waitFor(() => expect(api.createNote).toHaveBeenCalledWith(scopedAuth,
      expect.objectContaining({ body: "My unsaved revision", title: "Project handbook (copy)" })));
    expect(api.updateNote).toHaveBeenCalledTimes(1);
  });

  it("duplicates into a draft and creates child pages under the selected page", async () => {
    show({ segments: [rootId] });
    await screen.findByRole("textbox", { name: "Page content" });
    fireEvent.click(screen.getByRole("button", { name: "Duplicate" }));
    expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue("Project handbook (copy)");
    expect(screen.getByRole("textbox", { name: "Page content" })).toHaveValue(note.body);
    expect(api.createNote).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Project handbook" }));
    await screen.findByRole("button", { name: "Add subpage" });
    fireEvent.click(screen.getByRole("button", { name: "Add subpage" }));
    expect(screen.getByRole("button", { name: "Parent page" })).toHaveValue(rootId);
    expect(pageRoute()).toBe("new");
  });

  it("blocks deleting a parent and confirms deletion of a leaf with its version", async () => {
    show({ segments: [rootId] });
    await screen.findByRole("textbox", { name: "Page content" });
    fireEvent.click(screen.getByRole("button", { name: "Delete page" }));
    expect(api.deleteNote).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("Move or delete the subpages");
    fireEvent.click(screen.getByRole("button", { name: /^Launch checklist/ }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue("Launch checklist"));
    fireEvent.click(screen.getByRole("button", { name: "Delete page" }));
    await waitFor(() => expect(api.deleteNote).toHaveBeenCalledWith(scopedAuth, childId, 3));
    await screen.findByText("Page deleted.");
    expect(pageRoute()).toBe("");
  });

  it("keeps content after a save failure and supports an explicit retry", async () => {
    show({ segments: [rootId] });
    const body = await screen.findByRole("textbox", { name: "Page content" });
    fireEvent.change(body, { target: { value: "Offline work" } });
    vi.mocked(api.updateNote).mockRejectedValueOnce(new Error("offline"));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Could not save. Your changes are still here.");
    expect(body).toHaveValue("Offline work");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.updateNote).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeDisabled());
  });

  it("retries list and detail failures without presenting them as empty results", async () => {
    vi.mocked(api.listNotes).mockRejectedValueOnce(new Error("offline"));
    vi.mocked(api.getNote).mockRejectedValueOnce(new ApiRequestError("not found", 404));
    show({ segments: [rootId] });
    await screen.findByText("Could not load pages.");
    await screen.findByText("This page was deleted or is no longer available.");
    const retry = screen.getAllByRole("button", { name: "Retry" });
    retry.forEach((button) => fireEvent.click(button));
    await screen.findByRole("textbox", { name: "Page title" });
    await loaded();
    expect(screen.queryByText("Could not load pages.")).not.toBeInTheDocument();
  });

  it("warns before closing a dirty page and removes the warning after saving", async () => {
    show({ segments: [rootId] });
    const body = await screen.findByRole("textbox", { name: "Page content" });
    fireEvent.change(body, { target: { value: "Protect this draft" } });
    const dirtyUnload = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(dirtyUnload);
    expect(dirtyUnload.defaultPrevented).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeDisabled());
    await waitFor(() => expect(screen.queryByText("Saving…")).not.toBeInTheDocument());
    await waitFor(() => {
      const savedUnload = new Event("beforeunload", { cancelable: true });
      window.dispatchEvent(savedUnload);
      expect(savedUnload.defaultPrevented).toBe(false);
    });
  });

  it("keeps oversized content in the editor and prevents invalid save requests", async () => {
    show({ segments: [rootId] });
    const body = await screen.findByRole("textbox", { name: "Page content" });
    fireEvent.change(body, { target: { value: "a".repeat(200_001) } });
    expect(screen.getByRole("alert")).toHaveTextContent("Page content is too large.");
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.keyDown(body, { key: "s", ctrlKey: true });
    expect(api.updateNote).not.toHaveBeenCalled();
  });

  it("bounds duplicate titles while keeping the localized copy suffix", async () => {
    vi.mocked(api.getNote).mockResolvedValueOnce({ ...note, title: "a".repeat(200) });
    show({ segments: [rootId] });
    await screen.findByRole("textbox", { name: "Page content" });
    fireEvent.click(screen.getByRole("button", { name: "Duplicate" }));
    expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue(`${"a".repeat(193)} (copy)`);
  });

  it("shares only saved page state and exposes a separate all-project sharing scope", async () => {
    show({ segments: [rootId] });
    const body = await screen.findByRole("textbox", { name: "Page content" });
    fireEvent.change(body, { target: { value: "Unshared draft" } });
    expect(screen.getByRole("button", { name: "Share page" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Share page" })).toHaveAttribute("title", "Save your changes before sharing this page.");
    fireEvent.click(screen.getByRole("button", { name: "Share all notes" }));
    expect(await screen.findByRole("dialog", { name: "Public links" })).toHaveTextContent("All notes in this project");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
    fireEvent.click(screen.getByRole("button", { name: "Share page" }));
    expect(await screen.findByRole("dialog", { name: "Public links" })).toHaveTextContent("Page: Project handbook");
    expect(shareApi.listNoteShares).toHaveBeenCalledWith(scopedAuth, expect.any(AbortSignal));
  });

  it.each(["en", "ru", "ro"] as const)("provides a read-only view and localized controls in %s", async (locale) => {
    show({ segments: [rootId], canWrite: false, locale });
    const body = await screen.findByRole("textbox", { name: "Page content" });
    expect(body).toHaveAttribute("readonly");
    expect(document.querySelector(".notes-tree-grip")).toBeNull();
    expect(document.querySelector(".notes-root-drop")).toBeNull();
    expect(screen.queryByRole("button", { name: /^(Save|Сохранить|Salvează)$/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^(New page|Новая страница|Pagină nouă)$/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^(Share page|Поделиться страницей|Distribuie pagina)$/ })).not.toBeInTheDocument();
    expect(api.createNote).not.toHaveBeenCalled();
    expect(api.updateNote).not.toHaveBeenCalled();
  });
});

describe("Notes Markdown editor", () => {
  afterEach(cleanup);

  it("shows formatted Markdown and localized editing tools without changing the source on mount", async () => {
    const { default: Editor } = await vi.importActual<typeof import("./NoteEditor")>("./NoteEditor");
    const onChange = vi.fn();
    render(<I18nContext.Provider value={createI18n("ru", vi.fn())}><Editor value={"## Решение\n\n**Готово**\n\n- [x] Проверить\n\n| Имя | Статус |\n| --- | --- |\n| Версия | Готово |"} onChange={onChange} readOnly={false} /></I18nContext.Provider>);
    expect(await screen.findByRole("heading", { name: "Решение" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Жирный" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Список задач" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Добавить таблицу" })).toBeInTheDocument();
    expect(screen.getByRole("table")).toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("round-trips between visual and source modes without dropping the Markdown draft", async () => {
    const { default: Editor } = await vi.importActual<typeof import("./NoteEditor")>("./NoteEditor");
    const onChange = vi.fn();
    const editor = (value: string) => <I18nContext.Provider value={createI18n("en", vi.fn())}><Editor value={value} onChange={onChange} readOnly={false} /></I18nContext.Provider>;
    const { rerender } = render(editor("# Original\n\nSome **text**."));
    await screen.findByRole("heading", { name: "Original" });
    fireEvent.click(screen.getByRole("button", { name: "Markdown" }));
    const source = screen.getByRole("textbox", { name: "Page content" });
    expect(source).toHaveValue("# Original\n\nSome **text**.");
    fireEvent.change(source, { target: { value: "## Revised\n\n- [ ] Check" } });
    expect(onChange).toHaveBeenCalledWith("## Revised\n\n- [ ] Check");
    rerender(editor("## Revised\n\n- [ ] Check"));
    fireEvent.click(screen.getByRole("button", { name: "Visual" }));
    expect(await screen.findByRole("heading", { name: "Revised" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Markdown" }));
    expect(screen.getByRole("textbox", { name: "Page content" })).toHaveValue("## Revised\n\n- [ ] Check");
  });

  it("renders read-only Markdown with raw HTML suppressed and unsafe links disabled", async () => {
    const { default: Editor } = await vi.importActual<typeof import("./NoteEditor")>("./NoteEditor");
    const { container } = render(<I18nContext.Provider value={createI18n("en", vi.fn())}><Editor
      value={'# Safe page\n\n[Documentation](https://example.com)\n\n[Unsafe](javascript:alert(1))\n\n<script>alert("unsafe")</script>'}
      onChange={vi.fn()} readOnly /></I18nContext.Provider>);
    expect(screen.getByRole("heading", { name: "Safe page" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Documentation" })).toHaveAttribute("rel", "noopener noreferrer");
    expect(screen.getByText("Unsafe").closest("a")).not.toHaveAttribute("href");
    expect(container.querySelector("script")).toBeNull();
  });
});
