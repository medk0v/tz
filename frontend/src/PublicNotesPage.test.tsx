import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createI18n, I18nContext } from "./i18n";
import * as api from "./note-shares-api";
import { PublicNotesPage } from "./PublicNotesPage";

const edition = vi.hoisted(() => ({ lite: false }));
vi.mock("./admin/NoteDatabaseEmbeds", () => ({ NoteDatabaseEmbeds: () => null }));
vi.mock("./product-edition", async (original) => ({ ...await original<typeof import("./product-edition")>(), get isLiteEdition() { return edition.lite; } }));
vi.mock("./note-shares-api", async (original) => ({ ...await original<typeof import("./note-shares-api")>(), resolvePublicNotes: vi.fn(), unlockPublicNotes: vi.fn(), updatePublicNote: vi.fn() }));
vi.mock("./admin/NoteEditor", () => ({ default: ({ value, onChange, readOnly }: { value: string; onChange: (value: string) => void; readOnly: boolean }) =>
  <div data-testid="note-editor"><button type="button">Visual</button><button type="button">Markdown</button><textarea aria-label="Page content" value={value} readOnly={readOnly} onChange={(event) => onChange(event.target.value)} /></div> }));

const rootId = "10000000-0000-4000-8000-000000000001";
const childId = "10000000-0000-4000-8000-000000000002";
const note: api.PublicNote = { id: rootId, parent_id: null, sort_order: 0, title: "Public handbook", body: "# Shared content", icon: "📚", version: 4, created_at: "2026-09-21T10:00:00Z", updated_at: "2026-09-21T11:00:00Z" };
const child: api.PublicNote = { ...note, id: childId, parent_id: rootId, title: "Launch checklist", body: "- [ ] Publish" };
const base: api.PublicNotes = { can_edit: true, root_note_id: rootId, items: [note, child], note, theme_palette: null, theme_mode: null, allow_theme_change: false, include_descendants: true };
const clipboard = vi.fn();
function failure(status: number) { return new api.PublicNoteShareError("Failed", status, "request_failed"); }
function show({ noteId, onDirtyChange = vi.fn() }: { noteId?: string; onDirtyChange?: (value: boolean) => void } = {}) {
  const router = createMemoryRouter([{ path: "*", element: <PublicNotesPage token="secret-token" onDirtyChange={onDirtyChange} /> }], {
    initialEntries: [api.publicNoteSharePath("secret-token", noteId)],
  });
  render(<I18nContext.Provider value={createI18n("en", vi.fn())}><RouterProvider router={router} /></I18nContext.Provider>);
  return router;
}
async function body() { return screen.findByRole("textbox", { name: "Page content" }); }

describe("Public notes", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    edition.lite = false;
    localStorage.removeItem("public-notes-appearance");
    vi.mocked(api.resolvePublicNotes).mockImplementation(async (_token, _access, id) => ({ ...base, note: id === childId ? child : note }));
    vi.mocked(api.unlockPublicNotes).mockResolvedValue({ access_token: "short-lived-access" });
    vi.mocked(api.updatePublicNote).mockImplementation(async (_token, _access, id, input) => ({ ...note, ...input, id, version: input.expected_version + 1 }));
    vi.spyOn(window, "confirm").mockReturnValue(true);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: clipboard.mockResolvedValue(undefined) } });
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

  it("opens a scoped page directly and only sends the public token to the public helper", async () => {
    show({ noteId: childId });
    expect(await body()).toHaveValue(child.body);
    expect(api.resolvePublicNotes).toHaveBeenCalledWith("secret-token", undefined, childId, expect.any(AbortSignal));
    expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue("Launch checklist");
    expect(document.head.querySelector('meta[name="referrer"]')).toHaveAttribute("content", "no-referrer");
    expect(document.head.querySelector('meta[name="robots"]')).toHaveAttribute("content", "noindex,nofollow");
  });

  it("unlocks password-protected content without storing the password or access token", async () => {
    const persist = vi.spyOn(Storage.prototype, "setItem");
    vi.mocked(api.resolvePublicNotes).mockRejectedValueOnce(failure(401));
    show();
    await screen.findByRole("heading", { name: "This link is password protected" });
    expect(screen.queryByRole("textbox", { name: "Page content" })).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "owner-password" } });
    fireEvent.click(screen.getByRole("button", { name: "Open notes" }));
    await body();
    expect(api.unlockPublicNotes).toHaveBeenCalledWith("secret-token", "owner-password");
    expect(api.resolvePublicNotes).toHaveBeenLastCalledWith("secret-token", "short-lived-access", undefined, expect.any(AbortSignal));
    expect(persist).not.toHaveBeenCalled();
  });

  it("shows a wrong-password error and never renders the private page on failure", async () => {
    vi.mocked(api.resolvePublicNotes).mockRejectedValueOnce(failure(401));
    vi.mocked(api.unlockPublicNotes).mockRejectedValueOnce(failure(401));
    show(); await screen.findByLabelText("Password");
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "wrong" } });
    fireEvent.click(screen.getByRole("button", { name: "Open notes" }));
    await screen.findByText("Incorrect password. Try again.");
    expect(screen.queryByRole("textbox", { name: "Page content" })).not.toBeInTheDocument();
  });

  it("renders a standalone read-only document without branding, navigation or editor controls", async () => {
    vi.mocked(api.resolvePublicNotes).mockResolvedValueOnce({ ...base, items: [note], can_edit: false });
    show(); const title = await screen.findByRole("heading", { level: 1, name: note.title });
    expect(screen.getByRole("heading", { name: "Shared content" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Show pages" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("note-editor")).not.toBeInTheDocument();
    expect(screen.queryByText(/Tzomet|Shared notes|Read only/)).not.toBeInTheDocument();
    expect(document.querySelector(".public-notes-header, .public-notes-footer")).toBeNull();
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
    fireEvent.keyDown(title, { key: "s", ctrlKey: true });
    expect(api.updatePublicNote).not.toHaveBeenCalled();
    expect(document.title).toBe(note.title);
  });

  it("renders safe Markdown including tables and lists while suppressing raw HTML and external images", async () => {
    vi.mocked(api.resolvePublicNotes).mockResolvedValueOnce({ ...base, can_edit: false, note: { ...note, body: "- [x] Done\n\n| Name | Status |\n| --- | --- |\n| Release | Ready |\n\n[Safe](https://example.com) [Unsafe](javascript:alert%281%29)\n\n<img src='https://example.com/private.png' />\n\n![Remote](https://example.com/image.png)\n\n<script>alert('unsafe')</script>" } });
    show(); await screen.findByRole("heading", { name: note.title });
    const table = screen.getByRole("table"); expect(table).toHaveTextContent("Release"); expect(table.parentElement).toHaveClass("public-notes-table-scroll");
    expect(within(table).getByRole("columnheader", { name: "Name" })).toBeInTheDocument(); expect(within(table).getByRole("cell", { name: "Ready" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox")).toBeChecked();
    expect(screen.getByRole("link", { name: "Safe" })).toHaveAttribute("rel", "noopener noreferrer");
    expect(screen.getByText("Unsafe")).not.toHaveAttribute("href");
    expect(document.querySelector(".public-notes-prose img, .public-notes-prose script")).toBeNull();
  });

  it("keeps navigation for a published hierarchy and opens the visible child", async () => {
    vi.mocked(api.resolvePublicNotes).mockImplementation(async (_token, _access, id) => ({ ...base, can_edit: false, note: id === childId ? child : note }));
    const router = show(); await screen.findByRole("heading", { name: note.title });
    const navigation = screen.getByRole("navigation", { name: "Pages" });
    const parent = within(navigation).getByRole("button", { name: note.title });
    const childButton = within(navigation).getByRole("button", { name: child.title });
    expect(parent.closest("li")).toContainElement(childButton);
    fireEvent.click(childButton); await screen.findByRole("heading", { name: child.title });
    expect(router.state.location.search).toBe(`?note=${childId}`); expect(document.title).toBe(child.title);
  });

  it.each([1, 2])("shows project-share navigation only when more than one page is available (%i)", async (count) => {
    vi.mocked(api.resolvePublicNotes).mockResolvedValueOnce({ ...base, can_edit: false, root_note_id: null, include_descendants: false, items: [note, child].slice(0, count) });
    show(); await screen.findByRole("heading", { name: note.title });
    expect(screen.queryByRole("navigation", { name: "Pages" }) !== null).toBe(count > 1);
  });

  it("saves title and Markdown with version protection and the keyboard shortcut", async () => {
    const onDirtyChange = vi.fn();
    show({ onDirtyChange });
    const editor = await body();
    fireEvent.change(screen.getByRole("textbox", { name: "Page title" }), { target: { value: " Revised handbook " } });
    fireEvent.change(editor, { target: { value: "## Approved\n- [x] Done" } });
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(true));
    fireEvent.keyDown(editor, { key: "s", metaKey: true });
    await waitFor(() => expect(api.updatePublicNote).toHaveBeenCalledWith("secret-token", undefined, rootId, { title: "Revised handbook", body: "## Approved\n- [x] Done", icon: "📚", expected_version: 4 }));
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(false));
    expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue("Revised handbook");
  });

  it("retains conflicting edits, allows copying them and reloads only after confirmation", async () => {
    show(); const editor = await body();
    fireEvent.change(editor, { target: { value: "My draft" } });
    vi.mocked(api.updatePublicNote).mockRejectedValueOnce(failure(409));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText(/This page was changed by someone else/);
    expect(editor).toHaveValue("My draft");
    fireEvent.click(screen.getByRole("button", { name: "Copy draft" }));
    await screen.findByText("Draft copied");
    expect(clipboard).toHaveBeenCalledWith("# Public handbook\n\nMy draft");
    vi.mocked(window.confirm).mockReturnValueOnce(false);
    fireEvent.click(screen.getByRole("button", { name: "Load latest version" }));
    expect(api.resolvePublicNotes).toHaveBeenCalledTimes(1);
    vi.mocked(api.resolvePublicNotes).mockResolvedValueOnce({ ...base, note: { ...note, body: "Newer server text", version: 6 } });
    fireEvent.click(screen.getByRole("button", { name: "Load latest version" }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Page content" })).toHaveValue("Newer server text"));
  });

  it("keeps edits when password access expires and preserves the original version after re-unlocking", async () => {
    show(); const editor = await body();
    fireEvent.change(editor, { target: { value: "Unsaved during expiration" } });
    vi.mocked(api.updatePublicNote).mockRejectedValueOnce(failure(401));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Your access has expired. Enter the password again.");
    expect(screen.getByRole("textbox", { name: "Page content" })).toHaveValue("Unsaved during expiration");
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "owner-password" } });
    vi.mocked(api.resolvePublicNotes).mockResolvedValueOnce({ ...base, note: { ...note, body: "Server revision", version: 6 } });
    fireEvent.click(screen.getByRole("button", { name: "Open notes" }));
    await waitFor(() => expect(screen.queryByLabelText("Password")).not.toBeInTheDocument());
    expect(screen.getByRole("textbox", { name: "Page content" })).toHaveValue("Unsaved during expiration");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.updatePublicNote).toHaveBeenLastCalledWith("secret-token", "short-lived-access", rootId, expect.objectContaining({ body: "Unsaved during expiration", expected_version: 4 })));
  });

  it.each([child, null])("preserves a project-link draft if re-unlock resolves another page or an empty project (%j)", async (replacement) => {
    vi.mocked(api.resolvePublicNotes).mockResolvedValueOnce({ ...base, root_note_id: null });
    show(); fireEvent.change(await body(), { target: { value: "Do not replace my project draft" } });
    vi.mocked(api.updatePublicNote).mockRejectedValueOnce(failure(401));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByLabelText("Password");
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "owner-password" } });
    vi.mocked(api.resolvePublicNotes).mockResolvedValueOnce({ ...base, root_note_id: null, note: replacement });
    fireEvent.click(screen.getByRole("button", { name: "Open notes" }));
    await waitFor(() => expect(screen.queryByLabelText("Password")).not.toBeInTheDocument());
    expect(api.resolvePublicNotes).toHaveBeenLastCalledWith("secret-token", "short-lived-access", rootId, expect.any(AbortSignal));
    expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue(note.title);
    expect(screen.getByRole("textbox", { name: "Page content" })).toHaveValue("Do not replace my project draft");
  });

  it("keeps Lite free of normal-edition branding and language controls", async () => {
    edition.lite = true;
    vi.mocked(api.resolvePublicNotes).mockResolvedValueOnce({ ...base, theme_palette: "plum", theme_mode: "dark", allow_theme_change: true });
    show(); await body();
    expect(screen.queryByText("Tzomet")).not.toBeInTheDocument();
    expect(screen.queryByText("Shared with Tzomet")).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "Language" })).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "Theme" })).not.toBeInTheDocument();
    expect(document.documentElement).toHaveAttribute("data-palette", "graphite"); expect(document.documentElement).toHaveAttribute("data-theme", "light");
  });

  it("retains a revoked-link draft without offering further edits", async () => {
    show(); fireEvent.change(await body(), { target: { value: "Keep this local draft" } });
    vi.mocked(api.updatePublicNote).mockRejectedValueOnce(failure(404));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Access to this page is no longer available. Your unsaved draft is preserved below.");
    expect(screen.getByRole("textbox", { name: "Page content" })).toHaveValue("Keep this local draft");
    expect(screen.getByRole("textbox", { name: "Page content" })).toHaveAttribute("readonly");
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy draft" })).toBeInTheDocument();
  });

  it("guards page navigation and browser unload when edits are unsaved", async () => {
    const router = show(); fireEvent.change(await body(), { target: { value: "Keep here" } });
    const event = new Event("beforeunload", { cancelable: true }); window.dispatchEvent(event); expect(event.defaultPrevented).toBe(true);
    vi.mocked(window.confirm).mockReturnValueOnce(false);
    fireEvent.click(screen.getByRole("button", { name: "Launch checklist" }));
    expect(router.state.location.search).toBe("");
    expect(screen.getByRole("textbox", { name: "Page content" })).toHaveValue("Keep here");
    fireEvent.click(screen.getByRole("button", { name: "Launch checklist" }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Page content" })).toHaveValue(child.body));
    expect(router.state.location.search).toBe(`?note=${childId}`);
  });

  it("does not apply a completed save to another page after external navigation", async () => {
    let complete!: (value: api.PublicNote) => void;
    vi.mocked(api.updatePublicNote).mockReturnValueOnce(new Promise((resolve) => { complete = resolve; }));
    const router = show(); fireEvent.change(await body(), { target: { value: "First page draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await act(async () => { await router.navigate(api.publicNoteSharePath("secret-token", childId)); });
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue(child.title));
    await act(async () => { complete({ ...note, body: "First page draft", version: 5 }); });
    expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue(child.title);
    expect(screen.getByRole("textbox", { name: "Page content" })).toHaveValue(child.body);
  });
});
