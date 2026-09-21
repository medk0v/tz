import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";
import { PublicRatingPage } from "./PublicRatingPage";

vi.mock("./api", () => ({
  getPublicSupportRating: vi.fn(),
  submitPublicSupportRating: vi.fn(),
}));

describe("PublicRatingPage", () => {
  beforeEach(() => {
    window.history.replaceState(null, "", "/rate-chat#access=rating-token");
    vi.mocked(api.getPublicSupportRating).mockResolvedValue({
      language: "en",
      rating_prompt: "How was the support?",
      rating_thanks: "Thank you for the feedback.",
      already_rated: false,
      expires_at: "2026-09-18T00:00:00Z",
    });
    vi.mocked(api.submitPublicSupportRating).mockResolvedValue(true);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("removes the secret from the URL and submits the same structured rating", async () => {
    render(<PublicRatingPage />);

    expect(window.location.hash).toBe("");
    expect(await screen.findByRole("heading", { name: "How was the support?" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Rate 5 out of 5" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Issue resolved" }));
    fireEvent.change(screen.getByRole("textbox", { name: /Leave a comment/ }), {
      target: { value: "Everything was clear." },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send feedback" }));

    await waitFor(() => expect(api.submitPublicSupportRating).toHaveBeenCalledWith(
      "rating-token",
      5,
      ["resolved"],
      "Everything was clear.",
    ));
    expect(await screen.findByText("Thank you for the feedback.")).toBeInTheDocument();
  });

  it("shows the completion state when the chat was already rated in the widget", async () => {
    vi.mocked(api.getPublicSupportRating).mockResolvedValue({
      language: "ru",
      rating_prompt: "Оцените поддержку",
      rating_thanks: "Спасибо, оценка уже сохранена.",
      already_rated: true,
      expires_at: "2026-09-18T00:00:00Z",
    });

    render(<PublicRatingPage />);

    expect(await screen.findByText("Спасибо, оценка уже сохранена.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Оценка 5/ })).not.toBeInTheDocument();
  });
});
