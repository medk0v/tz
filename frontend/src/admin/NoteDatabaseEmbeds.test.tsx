import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NoteDatabaseEmbed, NoteDatabasesClient } from "../note-databases-api";
import { createI18n, I18nContext } from "../i18n";
import { NoteDatabaseEmbeds } from "./NoteDatabaseEmbeds";

vi.mock("./NoteDatabaseTable", () => ({
  NoteDatabaseTable: ({ databaseId, canManage, canEditFields, onDirtyChange }: { databaseId: string; canManage: boolean; canEditFields: boolean; onDirtyChange: (dirty: boolean) => void }) =>
    <div><p>{databaseId}</p><input aria-label="Table draft" defaultValue="" onChange={() => onDirtyChange(true)} /><button type="button" onClick={() => onDirtyChange(true)}>Edit table draft</button><button type="button" onClick={() => onDirtyChange(false)}>Discard table draft</button>{canManage && <span>Manage table</span>}{canEditFields && <span>Edit fields</span>}</div>,
  NoteDatabasePicker: ({ onSelect }: { onSelect: (database: string, view: string | null) => void }) => <button type="button" onClick={() => onSelect("shared-database", "saved-view")}>Choose saved view</button>,
}));

const embed: NoteDatabaseEmbed = { id: "embed", note_id: "note", database_id: "shared-database", view_id: "saved-view", position: 0, version: 3, created_at: "2026-09-21T10:00:00Z", updated_at: "2026-09-21T10:00:00Z" };
function client(items: NoteDatabaseEmbed[]) {
  return { listEmbeds: vi.fn().mockResolvedValue(items), createEmbed: vi.fn().mockResolvedValue(embed), deleteEmbed: vi.fn().mockResolvedValue(undefined), deleteDatabase: vi.fn() } as unknown as NoteDatabasesClient;
}
function show(api: NoteDatabasesClient, props: Partial<React.ComponentProps<typeof NoteDatabaseEmbeds>> = {}) {
  return render(<I18nContext.Provider value={createI18n("en", vi.fn())}><NoteDatabaseEmbeds client={api} noteId="note" canWrite canManage {...props} /></I18nContext.Provider>);
}
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

describe("tables embedded in a note", () => {
  it("inserts an existing saved view and refreshes the note version after the mutation", async () => {
    const api = client([]); const changed = vi.fn().mockResolvedValue(undefined);
    show(api, { onNoteChanged: changed });
    fireEvent.click(await screen.findByRole("button", { name: "Insert table" }));
    fireEvent.click(screen.getByRole("button", { name: "Choose saved view" }));
    await waitFor(() => expect(api.createEmbed).toHaveBeenCalledWith("note", { database_id: "shared-database", view_id: "saved-view" }));
    await waitFor(() => expect(changed).toHaveBeenCalledOnce());
    expect(api.listEmbeds).toHaveBeenCalledTimes(2);
  });
  it("removes only the note reference, keeping shared database data", async () => {
    const api = client([embed]); vi.spyOn(window, "confirm").mockReturnValue(true);
    show(api);
    fireEvent.click(await screen.findByRole("button", { name: "Remove from note" }));
    await waitFor(() => expect(api.deleteEmbed).toHaveBeenCalledWith("note", "embed", 3));
    expect(api.deleteDatabase).not.toHaveBeenCalled();
  });
  it("keeps public field editing separate from workspace table management and reports unsaved row drafts", async () => {
    const changed = vi.fn(); show(client([embed]), { canManage: false, onDirtyChange: changed });
    await screen.findByText("shared-database");
    expect(screen.getByText("Edit fields")).toBeInTheDocument();
    expect(screen.queryByText("Manage table")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Insert table" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Edit table draft" }));
    await waitFor(() => expect(changed).toHaveBeenLastCalledWith(true));
  });
  it("retains a dirty table when a renewed public client no longer lists its embed, until the draft is discarded", async () => {
    const api = client([embed]); const changed = vi.fn();
    const content = (current: NoteDatabasesClient) => <I18nContext.Provider value={createI18n("en", vi.fn())}><NoteDatabaseEmbeds client={current} noteId="note" canWrite canManage={false} onDirtyChange={changed} /></I18nContext.Provider>;
    const result = render(content(api));
    const input = await screen.findByRole("textbox", { name: "Table draft" });
    fireEvent.change(input, { target: { value: "Preserve my edits" } });
    await waitFor(() => expect(changed).toHaveBeenLastCalledWith(true));

    const renewedClient = client([]);
    let resolveList!: (items: NoteDatabaseEmbed[]) => void;
    vi.mocked(renewedClient.listEmbeds).mockImplementation(() => new Promise((resolve) => { resolveList = resolve; }));
    result.rerender(content(renewedClient));
    await waitFor(() => expect(renewedClient.listEmbeds).toHaveBeenCalledWith("note", expect.any(AbortSignal)));
    await act(async () => resolveList([]));
    expect(screen.getByRole("textbox", { name: "Table draft" })).toBe(input);
    expect(input).toHaveValue("Preserve my edits");
    expect(screen.getByText("shared-database")).toBeInTheDocument();
    expect(changed).toHaveBeenLastCalledWith(true);

    fireEvent.click(screen.getByRole("button", { name: "Discard table draft" }));
    await waitFor(() => expect(screen.queryByRole("textbox", { name: "Table draft" })).not.toBeInTheDocument());
    expect(screen.queryByText("shared-database")).not.toBeInTheDocument();
    expect(changed).toHaveBeenLastCalledWith(false);
  });
  it("keeps a dirty embed's original database until a retargeted embed can safely replace it", async () => {
    const content = (api: NoteDatabasesClient) => <I18nContext.Provider value={createI18n("en", vi.fn())}><NoteDatabaseEmbeds client={api} noteId="note" canWrite canManage={false} /></I18nContext.Provider>;
    const result = render(content(client([embed])));
    const input = await screen.findByRole("textbox", { name: "Table draft" });
    fireEvent.change(input, { target: { value: "Original table draft" } });
    const renewedClient = client([{ ...embed, database_id: "replacement-database", version: 4 }]);
    let resolveList!: (items: NoteDatabaseEmbed[]) => void;
    vi.mocked(renewedClient.listEmbeds).mockImplementation(() => new Promise((resolve) => { resolveList = resolve; }));
    result.rerender(content(renewedClient));
    await waitFor(() => expect(renewedClient.listEmbeds).toHaveBeenCalled());
    await act(async () => resolveList([{ ...embed, database_id: "replacement-database", version: 4 }]));
    expect(screen.getByText("shared-database")).toBeInTheDocument();
    expect(screen.queryByText("replacement-database")).not.toBeInTheDocument();
    expect(input).toHaveValue("Original table draft");
    fireEvent.click(screen.getByRole("button", { name: "Discard table draft" }));
    await screen.findByText("replacement-database");
    expect(screen.queryByText("shared-database")).not.toBeInTheDocument();
  });
});
