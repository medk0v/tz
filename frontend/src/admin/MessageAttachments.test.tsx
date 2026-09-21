import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MessageAttachments } from "../MessageAttachments";

const labels = {
  download: "Download file",
  loadVideo: "Load video",
  unavailable: "This file is no longer available.",
  loadError: "Could not load this file.",
  securityChecked: "malware scanned",
};

describe("MessageAttachments", () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("loads a voice recording into an audio player", async () => {
    vi.stubGlobal("URL", { ...URL, createObjectURL: vi.fn(() => "blob:voice"), revokeObjectURL: vi.fn() });
    const loadAttachment = vi.fn().mockResolvedValue(new Blob(["voice"], { type: "audio/ogg" }));
    render(
      <MessageAttachments
        attachments={[{
          id: "voice-1",
          file_name: "voice-message.ogg",
          content_type: "audio/ogg",
          byte_size: 5,
          available: true,
          expires_at: "2099-01-01T00:00:00Z",
        }]}
        labels={labels}
        loadAttachment={loadAttachment}
        variant="operator"
      />,
    );

    await waitFor(() => expect(screen.getByLabelText("voice-message.ogg", { selector: "audio" })).toHaveAttribute("src", "blob:voice"));
    expect(loadAttachment).toHaveBeenCalledWith("voice-1");
    expect(screen.getByRole("button", { name: "Download file" })).toBeEnabled();
  });
});
