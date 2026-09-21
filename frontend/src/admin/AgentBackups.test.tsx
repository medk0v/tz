import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { I18nContext, createI18n } from "../i18n";
import { AgentBackups } from "./AgentBackups";
import { createAiProfileBackup, deleteAiProfileBackup, listAiProfileBackups, restoreAiProfileBackup, type AiProfile, type AiProfileBackup } from "../api";

vi.mock("../api", () => ({ createAiProfileBackup: vi.fn(), deleteAiProfileBackup: vi.fn(), listAiProfileBackups: vi.fn(), restoreAiProfileBackup: vi.fn() }));
const auth = { kind: "session" } as const;
const profile = { id: "agent-1", instructions: "Saved", knowledge_base_ids: [] } as unknown as AiProfile;
const backup: AiProfileBackup = { id: "copy-1", reason: "manual", created_at: "2026-09-05T12:00:00Z", knowledge_base_count: 1, article_count: 2 };
const onRestored = vi.fn();
const onSubmit = vi.fn((event: React.FormEvent) => event.preventDefault());

function Harness({ dirty = false }: { dirty?: boolean }) {
  const [busy, setBusy] = useState(false);
  return <I18nContext.Provider value={createI18n("ru", () => {})}><form onSubmit={onSubmit}><AgentBackups auth={auth} profile={profile} dirty={dirty} busy={busy} onBusyChange={setBusy} onRestored={onRestored} /></form></I18nContext.Provider>;
}

async function open() {
  fireEvent.click(screen.getByRole("button", { name: "Резервная копия" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Обновить список" })).toBeEnabled());
}

describe("agent backups", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(listAiProfileBackups).mockResolvedValue([backup]);
    vi.mocked(createAiProfileBackup).mockResolvedValue(undefined);
    vi.mocked(deleteAiProfileBackup).mockResolvedValue(undefined);
    vi.mocked(restoreAiProfileBackup).mockResolvedValue(profile);
    vi.spyOn(window, "confirm").mockReturnValue(true);
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

  it("creates a durable copy without submitting the agent form", async () => {
    render(<Harness />);
    expect(listAiProfileBackups).not.toHaveBeenCalled();
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Создать резервную копию" }));
    await screen.findByText("Резервная копия создана. Теперь можно менять инструкции и базу знаний.");
    expect(createAiProfileBackup).toHaveBeenCalledWith(auth, profile.id);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("requires saving edited instructions before backup but allows explicit restoration", async () => {
    render(<Harness dirty />);
    await open();
    expect(screen.getByRole("button", { name: "Создать резервную копию" })).toBeDisabled();
    expect(screen.getByRole("checkbox", { name: "Создать резервную копию перед восстановлением" })).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: /Восстановить копию от/ }));
    await waitFor(() => expect(onRestored).toHaveBeenCalledWith(profile));
    expect(window.confirm).toHaveBeenCalledWith(expect.stringContaining("Несохранённые инструкции"));
    expect(restoreAiProfileBackup).toHaveBeenCalledWith(auth, profile.id, backup.id, { create_backup: true });
    expect(await screen.findByRole("status")).toHaveTextContent("Предыдущее сохранённое состояние доступно в списке копий.");
    expect(listAiProfileBackups).toHaveBeenCalledTimes(2);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("restores without another backup when the checkbox is cleared", async () => {
    render(<Harness />);
    await open();
    fireEvent.click(screen.getByRole("checkbox", { name: "Создать резервную копию перед восстановлением" }));
    fireEvent.click(screen.getByRole("button", { name: /Восстановить копию от/ }));
    await waitFor(() => expect(onRestored).toHaveBeenCalledWith(profile));
    expect(restoreAiProfileBackup).toHaveBeenCalledWith(auth, profile.id, backup.id, { create_backup: false });
    expect(window.confirm).toHaveBeenCalledWith(expect.stringContaining("Копия текущего сохранённого содержимого создаваться не будет."));
    expect(await screen.findByRole("status")).toHaveTextContent(/^Инструкции и база знаний восстановлены\.$/);
    expect(createAiProfileBackup).not.toHaveBeenCalled();
    expect(listAiProfileBackups).toHaveBeenCalledTimes(2);
  });

  it.each([
    { baseIds: ["restored-base-1"] },
    { baseIds: ["restored-base-1", "restored-base-2"] },
  ])("links directly to restored knowledge in a new tab: $baseIds", async ({ baseIds }) => {
    vi.mocked(restoreAiProfileBackup).mockResolvedValueOnce({ ...profile, knowledge_base_ids: baseIds });
    render(<Harness dirty />);
    await open();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Восстановить копию от/ }));
    const links = await screen.findAllByRole("link", { name: /Открыть восстановленную базу знаний/ });
    expect(links).toHaveLength(baseIds.length);
    links.forEach((link, index) => {
      expect(link).toHaveAttribute("href", `/cabinet/knowledge-base/${baseIds[index]}`);
      expect(link).toHaveAttribute("target", "_blank");
      expect(link).toHaveAttribute("rel", "noreferrer");
    });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("does not restore after cancel or replace the editor when restoration fails", async () => {
    render(<Harness />);
    await open();
    vi.mocked(window.confirm).mockReturnValueOnce(false);
    fireEvent.click(screen.getByRole("button", { name: /Восстановить копию от/ }));
    expect(restoreAiProfileBackup).not.toHaveBeenCalled();
    vi.mocked(restoreAiProfileBackup).mockRejectedValueOnce(new Error("offline"));
    fireEvent.click(screen.getByRole("button", { name: /Восстановить копию от/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось подтвердить восстановление");
    expect(onRestored).not.toHaveBeenCalled();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("distinguishes successful creation from a failed list refresh", async () => {
    render(<Harness />);
    await open();
    vi.mocked(listAiProfileBackups).mockRejectedValueOnce(new Error("offline"));
    fireEvent.click(screen.getByRole("button", { name: "Создать резервную копию" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Копия создана, но обновить список не удалось.");
  });

  it("deletes the selected backup and refreshes without submitting the form or restoring the agent", async () => {
    render(<Harness />);
    await open();
    vi.mocked(listAiProfileBackups).mockResolvedValueOnce([]);
    fireEvent.click(screen.getByRole("button", { name: /Удалить копию от/ }));
    expect(await screen.findByText("Резервная копия удалена.")).toBeInTheDocument();
    expect(window.confirm).toHaveBeenCalledWith(expect.stringContaining("без возможности восстановления"));
    expect(deleteAiProfileBackup).toHaveBeenCalledWith(auth, profile.id, backup.id);
    expect(await screen.findByText("Резервных копий пока нет.")).toBeInTheDocument();
    expect(listAiProfileBackups).toHaveBeenCalledTimes(2);
    expect(restoreAiProfileBackup).not.toHaveBeenCalled();
    expect(onRestored).not.toHaveBeenCalled();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("keeps the backup when deletion is cancelled or fails", async () => {
    render(<Harness />);
    await open();
    vi.mocked(window.confirm).mockReturnValueOnce(false);
    fireEvent.click(screen.getByRole("button", { name: /Удалить копию от/ }));
    expect(deleteAiProfileBackup).not.toHaveBeenCalled();
    vi.mocked(deleteAiProfileBackup).mockRejectedValueOnce(new Error("offline"));
    fireEvent.click(screen.getByRole("button", { name: /Удалить копию от/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось подтвердить удаление копии.");
    expect(screen.getByRole("button", { name: /Удалить копию от/ })).toBeEnabled();
    expect(listAiProfileBackups).toHaveBeenCalledTimes(1);
  });

  it("keeps a deleted backup out of the list when refresh fails", async () => {
    render(<Harness />);
    await open();
    vi.mocked(listAiProfileBackups).mockRejectedValueOnce(new Error("offline"));
    fireEvent.click(screen.getByRole("button", { name: /Удалить копию от/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Копия удалена, но обновить список не удалось.");
    expect(screen.queryByRole("button", { name: /Удалить копию от/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Обновить список" })).toBeEnabled();
  });

  it("prevents backup actions and changing the option during restoration", async () => {
    let finishRestore!: (restored: AiProfile) => void;
    vi.mocked(restoreAiProfileBackup).mockReturnValueOnce(new Promise((resolve) => { finishRestore = resolve; }));
    render(<Harness />);
    await open();
    fireEvent.click(screen.getByRole("button", { name: /Восстановить копию от/ }));
    expect(screen.getByRole("checkbox")).toBeDisabled();
    expect(screen.getByRole("button", { name: /Удалить копию от/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Восстановить копию от/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Создать резервную копию" })).toBeDisabled();
    finishRestore(profile);
    await waitFor(() => expect(screen.getByRole("checkbox")).toBeEnabled());
  });
});
