import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiRequestError } from "../api";
import { createI18n, I18nContext } from "../i18n";
import * as api from "../note-shares-api";
import * as notesApi from "../notes-api";
import { materializeShareSettings, sharePageRows, shareSettingsInput } from "./note-share-settings";
import { NoteShareDialog } from "./NoteShareDialog";

vi.mock("../note-shares-api", async (original) => ({ ...await original<typeof import("../note-shares-api")>(), listNoteShares: vi.fn(), createNoteShare: vi.fn(), revokeNoteShare: vi.fn(), updateNoteShare: vi.fn() }));
vi.mock("../notes-api", async (original) => ({ ...await original<typeof import("../notes-api")>(), listNotes: vi.fn() }));
const auth = { kind: "session", projectId: "project-one" } as const;
const settings: api.NoteShareSettings = { theme_palette: null, theme_mode: null, allow_theme_change: false, include_descendants: false, included_note_ids: [], can_edit: false };
const share: api.NoteShare = { ...settings, version: 3, id: "share-one", note_id: "note-one", custom_slug: null, has_password: true, can_edit: false, created_at: "2026-09-21T10:00:00Z" };
const custom: api.NoteShare = { ...share, id: "share-custom", custom_slug: "team-notes" };
const note: notesApi.NoteSummary = { id: "note-one", parent_id: null, title: "Handbook", icon: "", is_favorite: false, sort_order: 0, version: 1, created_at: share.created_at, updated_at: share.created_at };
const notes: notesApi.NoteSummary[] = [note, { ...note, id: "child", parent_id: note.id, title: "Guidelines" }, { ...note, id: "grandchild", parent_id: "child", title: "Examples" }, { ...note, id: "other", title: "Private page", sort_order: 1 }];
const clipboard = vi.fn();
function show(noteId: string | null = "note-one") {
  const onClose = vi.fn();
  render(<I18nContext.Provider value={createI18n("en", vi.fn())}><NoteShareDialog auth={auth} noteId={noteId} title="Handbook" onClose={onClose} /></I18nContext.Provider>);
  return { onClose };
}
async function ready() { await waitFor(() => expect(screen.getByRole("button", { name: "Create link" })).toBeEnabled()); }

describe("Note share management", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.listNoteShares).mockResolvedValue([]);
    vi.mocked(notesApi.listNotes).mockResolvedValue(notes);
    vi.mocked(api.updateNoteShare).mockImplementation(async (_auth, id, input) => ({ ...custom, ...input, id, version: input.expected_version + 1 }));
    vi.mocked(api.createNoteShare).mockImplementation(async (_auth, input) => ({ ...share, ...input, custom_slug: input.custom_slug ?? null, has_password: !!input.password, token: "new-secret-token" }));
    vi.mocked(api.revokeNoteShare).mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: clipboard.mockResolvedValue(undefined) } });
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

  it("creates a read-only secret link for a saved page and only copies it on request", async () => {
    show(); await ready();
    expect(screen.getByRole("checkbox", { name: "Allow editing without an account" })).not.toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "Create link" }));
    const url = await screen.findByRole("textbox", { name: "New public link" });
    expect(api.createNoteShare).toHaveBeenCalledWith(auth, { ...settings, note_id: "note-one", password: null, custom_slug: null });
    expect(url).toHaveValue(`${window.location.origin}/notes/share/new-secret-token`);
    expect(screen.getByText(/Copy this secret link now/)).toBeInTheDocument();
    expect(clipboard).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Copy link" }));
    await screen.findByRole("button", { name: "Copied" });
    expect(clipboard).toHaveBeenCalledWith(`${window.location.origin}/notes/share/new-secret-token`);
  });

  it("creates an editable password-protected project link with a normalized custom address", async () => {
    show(null); await ready();
    fireEvent.change(screen.getByLabelText("Custom address (optional)"), { target: { value: "TEAM-NOTES" } });
    fireEvent.change(screen.getByLabelText("Password (optional)"), { target: { value: "Long password 123" } });
    fireEvent.click(screen.getByRole("checkbox", { name: "Allow editing without an account" }));
    fireEvent.click(screen.getByRole("button", { name: "Create link" }));
    await screen.findByRole("textbox", { name: "New public link" });
    expect(api.createNoteShare).toHaveBeenCalledWith(auth, { ...settings, include_descendants: true, included_note_ids: ["note-one", "child", "grandchild", "other"], note_id: null, password: "Long password 123", custom_slug: "team-notes", can_edit: true });
    expect(screen.getByLabelText("Password (optional)")).toHaveValue("");
    expect(screen.getByRole("textbox", { name: "New public link" })).toHaveValue(`${window.location.origin}/notes/share/team-notes`);
  });

  it("validates password length and custom address boundaries before sending", async () => {
    show(); await ready();
    fireEvent.change(screen.getByLabelText("Password (optional)"), { target: { value: "short" } });
    expect(screen.getByRole("button", { name: "Create link" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Password (optional)"), { target: { value: "" } });
    fireEvent.change(screen.getByLabelText("Custom address (optional)"), { target: { value: "-invalid-" } });
    expect(screen.getByRole("button", { name: "Create link" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Custom address (optional)"), { target: { value: "valid--address" } });
    expect(screen.getByRole("button", { name: "Create link" })).toBeEnabled();
    const pattern = screen.getByLabelText("Custom address (optional)").getAttribute("pattern")!;
    expect(new RegExp(pattern, "v").test("valid--address")).toBe(true);
    expect(api.createNoteShare).not.toHaveBeenCalled();
  });

  it("lists only the selected scope, permits copying custom links and never reconstructs stored secrets", async () => {
    vi.mocked(api.listNoteShares).mockResolvedValue([share, custom, { ...custom, id: "other", note_id: null, custom_slug: "all-project" }]);
    show(); await ready();
    expect(screen.queryByText("all-project")).not.toBeInTheDocument();
    expect(screen.getByText(/The secret address is only shown/)).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Copy link" })).toHaveLength(1);
    expect(screen.getByRole("textbox", { name: "Custom link" })).toHaveValue(`${window.location.origin}/notes/share/team-notes`);
  });

  it("confirms revocation and removes the link only after success", async () => {
    vi.mocked(api.listNoteShares).mockResolvedValue([custom]);
    show(); await ready();
    vi.mocked(window.confirm).mockReturnValueOnce(false);
    fireEvent.click(screen.getByRole("button", { name: "Revoke link" }));
    expect(api.revokeNoteShare).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Revoke link" }));
    await screen.findByText("Link revoked.");
    expect(api.revokeNoteShare).toHaveBeenCalledWith(auth, custom.id);
    expect(screen.queryByRole("textbox", { name: "Custom link" })).not.toBeInTheDocument();
  });

  it("retains settings after a duplicate custom address and retries list failures", async () => {
    vi.mocked(api.listNoteShares).mockRejectedValueOnce(new Error("offline"));
    show(); await screen.findByText("Could not load public links.");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await ready();
    fireEvent.change(screen.getByLabelText("Custom address (optional)"), { target: { value: "existing-address" } });
    vi.mocked(api.createNoteShare).mockRejectedValueOnce(new ApiRequestError("slug exists", 409));
    fireEvent.click(screen.getByRole("button", { name: "Create link" }));
    await screen.findByText("This address is already in use. Choose another.");
    expect(screen.getByLabelText("Custom address (optional)")).toHaveValue("existing-address");
  });

  it("selects descendants with their ancestors and excludes the subtree when a parent is unchecked", async () => {
    show(); await ready();
    expect(screen.queryByRole("checkbox", { name: "Guidelines" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "Show subpages" }));
    expect(screen.getByRole("checkbox", { name: "Guidelines" })).not.toBeChecked();
    expect(screen.queryByRole("checkbox", { name: "Private page" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "Guidelines / Examples" }));
    expect(screen.getByRole("checkbox", { name: "Guidelines" })).toBeChecked();
    fireEvent.click(screen.getByRole("checkbox", { name: "Guidelines" }));
    expect(screen.getByRole("checkbox", { name: "Guidelines / Examples" })).not.toBeChecked();
    fireEvent.click(screen.getByRole("checkbox", { name: "Guidelines / Examples" }));
    fireEvent.click(screen.getByRole("button", { name: "Create link" }));
    await screen.findByRole("textbox", { name: "New public link" });
    expect(api.createNoteShare).toHaveBeenCalledWith(auth, { ...settings, include_descendants: true, included_note_ids: ["child", "grandchild"], note_id: note.id, custom_slug: null, password: null });
  });

  it("only sends checked roots when project descendants are disabled, including an intentionally empty selection", async () => {
    show(null); await ready();
    fireEvent.click(screen.getByRole("checkbox", { name: "Show subpages" }));
    expect(screen.queryByRole("checkbox", { name: "Handbook / Guidelines" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "Handbook" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Private page" }));
    expect(screen.getByText("No pages will be available through this link.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Create link" }));
    await screen.findByRole("textbox", { name: "New public link" });
    expect(api.createNoteShare).toHaveBeenCalledWith(auth, { ...settings, included_note_ids: [], note_id: null, custom_slug: null, password: null });
  });

  it("updates the same link and version while preserving the independent creation draft and secret fields", async () => {
    vi.mocked(api.listNoteShares).mockResolvedValue([custom]);
    show(); await ready();
    fireEvent.change(screen.getByLabelText("Custom address (optional)"), { target: { value: "unfinished-link" } });
    fireEvent.change(screen.getByLabelText("Password (optional)"), { target: { value: "Unfinished password" } });
    fireEvent.click(screen.getByRole("button", { name: "Link settings" }));
    await screen.findByRole("button", { name: "Save settings" });
    fireEvent.click(screen.getByRole("checkbox", { name: "Allow editing without an account" }));
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await screen.findByText("Link settings saved.");
    expect(api.updateNoteShare).toHaveBeenCalledWith(auth, custom.id, { ...settings, can_edit: true, expected_version: 3 });
    expect(api.createNoteShare).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox", { name: "Custom link" })).toHaveValue(`${window.location.origin}/notes/share/team-notes`);
    expect(screen.queryByLabelText("Password (optional)")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByLabelText("Custom address (optional)")).toHaveValue("unfinished-link");
    expect(screen.getByLabelText("Password (optional)")).toHaveValue("Unfinished password");
  });

  it("materializes legacy all-pages links into an explicit checklist when saved", async () => {
    vi.mocked(api.listNoteShares).mockResolvedValue([{ ...custom, include_descendants: true, included_note_ids: null }]);
    show(); await ready();
    fireEvent.click(screen.getByRole("button", { name: "Link settings" }));
    await screen.findByRole("button", { name: "Save settings" });
    expect(screen.getByRole("checkbox", { name: "Guidelines" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Guidelines / Examples" })).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await screen.findByText("Link settings saved.");
    expect(api.updateNoteShare).toHaveBeenCalledWith(auth, custom.id, { ...settings, include_descendants: true, included_note_ids: ["child", "grandchild"], expected_version: 3 });
  });

  it("preserves the draft on a conflict and only adopts the latest version when settings are explicitly reopened", async () => {
    vi.mocked(api.listNoteShares).mockResolvedValue([custom]);
    vi.mocked(api.updateNoteShare).mockRejectedValueOnce(new ApiRequestError("version conflict", 409));
    show(); await ready();
    fireEvent.click(screen.getByRole("button", { name: "Link settings" }));
    await screen.findByRole("button", { name: "Save settings" });
    fireEvent.click(screen.getByRole("checkbox", { name: "Allow editing without an account" }));
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await screen.findByText(/This link's settings changed/);
    expect(screen.getByRole("checkbox", { name: "Allow editing without an account" })).toBeChecked();
    vi.mocked(api.listNoteShares).mockResolvedValue([{ ...custom, version: 7 }]);
    fireEvent.click(screen.getByRole("button", { name: "Link settings" }));
    await waitFor(() => expect(screen.getByRole("checkbox", { name: "Allow editing without an account" })).not.toBeChecked());
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await screen.findByText("Link settings saved.");
    expect(api.updateNoteShare).toHaveBeenLastCalledWith(auth, custom.id, { ...settings, expected_version: 7 });
  });

  it("cannot create links when the page list is unavailable", async () => {
    vi.mocked(notesApi.listNotes).mockRejectedValueOnce(new Error("offline"));
    show(); await screen.findByText("Could not load pages for sharing.");
    expect(screen.getByRole("button", { name: "Create link" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await ready();
  });

  it("prunes moved pages with unselected ancestors and strips response metadata from settings requests", () => {
    const rows = sharePageRows(notes, note.id);
    const draft = materializeShareSettings({ ...custom, include_descendants: true, included_note_ids: ["grandchild"] }, rows);
    expect(draft.included_note_ids).toEqual([]);
    expect(shareSettingsInput(draft, rows, note.id)).toEqual({ ...settings, include_descendants: true });
  });
});
