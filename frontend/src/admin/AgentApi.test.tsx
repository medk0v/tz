import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { createI18n, I18nContext } from "../i18n";
import { AgentApi } from "./AgentApi";

vi.mock("../api", async (importOriginal) => ({
  ...await importOriginal<typeof api>(),
  getAiAgentApiSettings: vi.fn(),
  getAiAgentApiRun: vi.fn(),
  updateAiAgentApiSettings: vi.fn(),
  listAiAgentApiRuns: vi.fn(),
  createAiAgentApiEndpoint: vi.fn(),
  updateAiAgentApiEndpoint: vi.fn(),
  deleteAiAgentApiEndpoint: vi.fn(),
  createAiAgentApiKey: vi.fn(),
  revokeAiAgentApiKey: vi.fn(),
  testAiAgentApiEndpoint: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const profileId = "00000000-0000-4000-8000-000000000020";
const endpoint: api.AiAgentApiEndpoint = {
  id: "00000000-0000-4000-8000-000000000030", name: "Check address", slug: "check-address",
  instructions: "Check the given address and return whether the native SOL balance is zero. Fail if the check cannot be completed.",
  input_example: { address: "SOL_ADDRESS" }, output_example: { address_empty: true },
  input_schema: { type: "object" }, output_schema: { type: "object" },
  enabled: true, timeout_seconds: 180, cache_refresh_seconds: null, response_wait_seconds: 20, version: 1,
  created_at: "2026-09-08T10:00:00Z", updated_at: "2026-09-08T10:00:00Z",
};
const settings: api.AiAgentApiSettings = {
  enabled: false, key_configured: false, key_prefix: null, can_manage_key: true, endpoints: [endpoint],
};
const pending: api.AiAgentApiRun = {
  id: "00000000-0000-4000-8000-000000000040", endpoint_id: endpoint.id, endpoint_name: endpoint.name,
  input: { address: "submitted-address" }, status: "pending", result: null, error: null,
  created_at: "2026-09-08T11:00:00Z", started_at: null, completed_at: null,
};

function view(profileDirty = false, onSubmit = vi.fn()) {
  return render(<I18nContext.Provider value={createI18n("en", vi.fn())}>
    <form onSubmit={onSubmit}><AgentApi auth={auth} profileId={profileId} profileDirty={profileDirty} /></form>
  </I18nContext.Provider>);
}

async function openSettings() {
  fireEvent.click(screen.getByRole("button", { name: "API settings" }));
  await screen.findByRole("checkbox", { name: "Enable API" });
}

async function editEndpoint() {
  await openSettings();
  fireEvent.click(screen.getByRole("button", { name: /Check address \/check-address/ }));
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.getAiAgentApiSettings).mockResolvedValue(settings);
  vi.mocked(api.listAiAgentApiRuns).mockResolvedValue([]);
  vi.mocked(api.getAiAgentApiRun).mockResolvedValue(pending);
  vi.mocked(api.updateAiAgentApiSettings).mockResolvedValue({ ...settings, enabled: true });
  vi.spyOn(window, "confirm").mockReturnValue(true);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: vi.fn().mockResolvedValue(undefined) } });
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); });

describe("AgentApi", () => {
  it("loads on opening and saves its toggle without submitting the profile form", async () => {
    const onSubmit = vi.fn((event: Event) => event.preventDefault());
    const rendered = view(false, onSubmit);
    expect(api.getAiAgentApiSettings).not.toHaveBeenCalled();
    await openSettings();
    fireEvent.click(screen.getByRole("checkbox", { name: "Enable API" }));
    await waitFor(() => expect(api.updateAiAgentApiSettings).toHaveBeenCalledWith(auth, profileId, true));
    expect(await screen.findByText("API settings saved.")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Enable API" })).toBeChecked();
    expect(rendered.container.querySelectorAll("form")).toHaveLength(1);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("rejects an invalid JSON object and saves all method fields when corrected", async () => {
    vi.mocked(api.createAiAgentApiEndpoint).mockResolvedValue({ ...endpoint, name: "Another method", slug: "another-method" });
    view();
    await openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Add method" }));
    fireEvent.change(screen.getByLabelText("Method name"), { target: { value: "Another method" } });
    fireEvent.change(screen.getByLabelText("URL path"), { target: { value: "another-method" } });
    fireEvent.change(screen.getByLabelText("Task for the agent"), { target: { value: "Check the address." } });
    fireEvent.change(screen.getByLabelText("Example response JSON"), { target: { value: "[]" } });
    fireEvent.click(screen.getByRole("button", { name: "Save method" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Example response JSON: enter a valid JSON object");
    expect(api.createAiAgentApiEndpoint).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Example response JSON"), { target: { value: '{"empty":false}' } });
    fireEvent.click(screen.getByRole("button", { name: "Save method" }));
    await waitFor(() => expect(api.createAiAgentApiEndpoint).toHaveBeenCalledWith(auth, profileId, {
      name: "Another method", slug: "another-method", instructions: "Check the address.",
      input_example: { address: "SOL_ADDRESS" }, output_example: { empty: false }, timeout_seconds: 180, enabled: true, cache_refresh_seconds: null, response_wait_seconds: 180,
    }));
  });

  it("saves the cache interval, restores it and removes polling advice", async () => {
    const cached = { ...endpoint, cache_refresh_seconds: 300 };
    vi.mocked(api.updateAiAgentApiEndpoint).mockResolvedValue(cached);
    view();
    await editEndpoint();
    fireEvent.change(screen.getByLabelText("Response mode"), { target: { value: "cached" } });
    expect(screen.getByLabelText("Refresh interval, seconds")).toHaveValue("300");
    fireEvent.change(screen.getByLabelText("Refresh interval, seconds"), { target: { value: "30" } });
    fireEvent.click(screen.getByRole("button", { name: "Save method" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("60 to 86400");
    expect(api.updateAiAgentApiEndpoint).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Refresh interval, seconds"), { target: { value: "300" } });
    fireEvent.click(screen.getByRole("button", { name: "Save method" }));
    await waitFor(() => expect(api.updateAiAgentApiEndpoint).toHaveBeenCalledWith(auth, profileId, endpoint.id, expect.objectContaining({ cache_refresh_seconds: 300 })));
    const request = screen.getByRole("region", { name: "Request example" });
    expect(request).toHaveTextContent("updated_at");
    expect(request).not.toHaveTextContent("HTTP 202");
    expect(request.querySelector("pre")).not.toHaveTextContent("Idempotency-Key");
    cleanup();
    vi.mocked(api.getAiAgentApiSettings).mockResolvedValue({ ...settings, endpoints: [cached] });
    view();
    await editEndpoint();
    expect(screen.getByLabelText("Response mode")).toHaveValue("cached");
    expect(screen.queryByLabelText("Response wait, seconds")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Refresh interval, seconds")).toHaveValue("300");
    vi.mocked(api.updateAiAgentApiEndpoint).mockResolvedValue(endpoint);
    fireEvent.change(screen.getByLabelText("Response mode"), { target: { value: "live" } });
    fireEvent.click(screen.getByRole("button", { name: "Save method" }));
    await waitFor(() => expect(api.updateAiAgentApiEndpoint).toHaveBeenLastCalledWith(auth, profileId, endpoint.id, expect.objectContaining({ cache_refresh_seconds: null })));
  });

  it("always waits for live results without an early response setting", async () => {
    vi.mocked(api.updateAiAgentApiEndpoint).mockResolvedValue(endpoint);
    view();
    await editEndpoint();
    expect(screen.getByLabelText("Response mode")).toHaveValue("live");
    expect(screen.queryByLabelText("Response wait, seconds")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Execution timeout, seconds"), { target: { value: "300" } });
    fireEvent.click(screen.getByRole("button", { name: "Save method" }));
    await waitFor(() => expect(api.updateAiAgentApiEndpoint).toHaveBeenCalledWith(auth, profileId, endpoint.id, expect.objectContaining({ timeout_seconds: 300, cache_refresh_seconds: null })));
    expect(screen.getByRole("region", { name: "Request example" })).toHaveTextContent("waits for completion");
    expect(screen.getByRole("region", { name: "Request example" })).not.toHaveTextContent("poll_url");
  });

  it("blocks tests when method or profile settings have unsaved changes", async () => {
    view();
    await editEndpoint();
    const testButton = screen.getByRole("button", { name: "Test method" });
    expect(testButton).toBeEnabled();
    fireEvent.change(screen.getByLabelText("Task for the agent"), { target: { value: "Changed task" } });
    expect(testButton).toBeDisabled();
    expect(screen.getByText("Save the agent and method changes before testing.")).toBeInTheDocument();
    expect(api.testAiAgentApiEndpoint).not.toHaveBeenCalled();
    cleanup();
    view(true);
    await editEndpoint();
    expect(screen.getByRole("button", { name: "Test method" })).toBeDisabled();
  });

  it("tests a disabled public API without a key, polls the run, and preserves a false JSON result", async () => {
    vi.mocked(api.testAiAgentApiEndpoint).mockResolvedValue(pending);
    view();
    await editEndpoint();
    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("Test request JSON"), { target: { value: '{"address":"submitted-address"}' } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Test method" })); });
    expect(api.testAiAgentApiEndpoint).toHaveBeenCalledWith(auth, profileId, endpoint.id, { address: "submitted-address" });
    expect(within(screen.getByRole("region", { name: "Test method" })).getByText(/request is running/)).toBeInTheDocument();
    vi.mocked(api.listAiAgentApiRuns).mockResolvedValue([{ ...pending, status: "completed", result: { address_empty: false }, started_at: pending.created_at, completed_at: "2026-09-08T11:00:02Z" }]);
    vi.mocked(api.getAiAgentApiRun).mockResolvedValue({ ...pending, status: "completed", result: { address_empty: false }, started_at: pending.created_at, completed_at: "2026-09-08T11:00:02Z" });
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    const testRegion = screen.getByRole("region", { name: "Test method" });
    expect(within(testRegion).getByText(/"address_empty": false/)).toBeInTheDocument();
    expect(within(testRegion).getByText("Duration: 2.0 s")).toBeInTheDocument();
    const calls = vi.mocked(api.listAiAgentApiRuns).mock.calls.length;
    await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
    expect(api.listAiAgentApiRuns).toHaveBeenCalledTimes(calls);
  });

  it("keeps following the selected test after it falls outside recent history", async () => {
    vi.mocked(api.testAiAgentApiEndpoint).mockResolvedValue(pending);
    view();
    await editEndpoint();
    vi.useFakeTimers();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Test method" })); });
    vi.mocked(api.listAiAgentApiRuns).mockResolvedValue([]);
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    const testRegion = screen.getByRole("region", { name: "Test method" });
    expect(within(testRegion).getByText(/request is running/)).toBeInTheDocument();
    expect(api.getAiAgentApiRun).toHaveBeenCalledWith(auth, profileId, pending.id);
    vi.mocked(api.getAiAgentApiRun).mockResolvedValue({ ...pending, status: "completed", result: { address_empty: true } });
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    expect(within(testRegion).getByText(/"address_empty": true/)).toBeInTheDocument();
    expect(within(testRegion).getByRole("button", { name: "Test method" })).toBeEnabled();
  });

  it("shows failures as errors without manufacturing a boolean result", async () => {
    vi.mocked(api.listAiAgentApiRuns).mockResolvedValue([{ ...pending, status: "failed", error: { code: "verification_failed", message: "The source did not load." } }]);
    view();
    await openSettings();
    const history = screen.getByRole("region", { name: "Recent calls" });
    expect(within(history).getByRole("alert", { hidden: true })).toHaveTextContent("verification_failedThe source did not load.");
    expect(within(history).queryByText(/"address_empty"/)).not.toBeInTheDocument();
  });

  it("shows a newly created key once and keeps the key out of request examples", async () => {
    vi.mocked(api.createAiAgentApiKey).mockResolvedValue({ key: "tzagent_secret_value", key_prefix: "tzagent" });
    view();
    await editEndpoint();
    fireEvent.click(screen.getByRole("button", { name: "Create key" }));
    expect(await screen.findByText("tzagent_secret_value")).toBeInTheDocument();
    const request = screen.getByRole("region", { name: "Request example" });
    expect(request).toHaveTextContent("Authorization: Bearer YOUR_API_KEY");
    expect(request).toHaveTextContent("Idempotency-Key: 7b792dd9-2ee5-4f42-b7e2-989070598209");
    expect(request).not.toHaveTextContent("tzagent_secret_value");
    fireEvent.click(within(screen.getByRole("region", { name: "Invocation key" })).getByRole("button", { name: "Copy" }));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("tzagent_secret_value");
    fireEvent.click(screen.getByRole("button", { name: "API settings" }));
    expect(screen.queryByText("tzagent_secret_value")).not.toBeInTheDocument();
    await openSettings();
    expect(screen.queryByText("tzagent_secret_value")).not.toBeInTheDocument();
  });

  it("hides key management when the server disallows it and displays validation errors", async () => {
    vi.mocked(api.getAiAgentApiSettings).mockResolvedValue({ ...settings, can_manage_key: false });
    vi.mocked(api.updateAiAgentApiSettings).mockRejectedValue(new api.ApiRequestError("Activate the agent first", 400));
    view();
    await openSettings();
    expect(screen.queryByRole("button", { name: "Create key" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "Enable API" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Activate the agent first");
    expect(screen.getByRole("checkbox", { name: "Enable API" })).not.toBeChecked();
  });

  it("hides the previously revealed key after an external rotation is refreshed", async () => {
    vi.mocked(api.createAiAgentApiKey).mockResolvedValue({ key: "tzagent_secret_value", key_prefix: "tzagent_old" });
    view();
    await openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Create key" }));
    expect(await screen.findByText("tzagent_secret_value")).toBeInTheDocument();
    vi.mocked(api.getAiAgentApiSettings).mockResolvedValue({ ...settings, key_configured: true, key_prefix: "tzagent_new" });
    fireEvent.click(screen.getAllByRole("button", { name: "Refresh" })[0]);
    expect(await screen.findByText("tzagent_new…")).toBeInTheDocument();
    expect(screen.queryByText("tzagent_secret_value")).not.toBeInTheDocument();
  });

  it("clears old credentials when auth changes and ignores an in-flight key creation", async () => {
    let completeKey: (key: api.AiAgentApiKey) => void = () => {};
    vi.mocked(api.createAiAgentApiKey).mockImplementation(() => new Promise((resolve) => { completeKey = resolve; }));
    const rendered = view();
    await openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Create key" }));
    const nextAuth: api.OperatorAuth = { kind: "access_token", token: "another-actor-token" };
    rendered.rerender(<I18nContext.Provider value={createI18n("en", vi.fn())}>
      <form><AgentApi auth={nextAuth} profileId={profileId} profileDirty={false} /></form>
    </I18nContext.Provider>);
    expect(screen.getByRole("button", { name: "API settings" })).toHaveAttribute("aria-expanded", "false");
    await act(async () => { completeKey({ key: "previous-actor-secret", key_prefix: "old" }); });
    vi.mocked(api.getAiAgentApiSettings).mockResolvedValue({ ...settings, can_manage_key: false });
    await openSettings();
    expect(api.getAiAgentApiSettings).toHaveBeenLastCalledWith(nextAuth, profileId);
    expect(screen.queryByText("previous-actor-secret")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create key" })).not.toBeInTheDocument();
  });

  it("revokes the invocation key and removes the secret from view", async () => {
    vi.mocked(api.getAiAgentApiSettings).mockResolvedValue({ ...settings, key_configured: true, key_prefix: "tzagent" });
    vi.mocked(api.revokeAiAgentApiKey).mockResolvedValue(undefined);
    view();
    await openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Revoke key" }));
    await waitFor(() => expect(api.revokeAiAgentApiKey).toHaveBeenCalledWith(auth, profileId));
    expect(await screen.findByRole("button", { name: "Create key" })).toBeInTheDocument();
    expect(screen.queryByText("tzagent…")).not.toBeInTheDocument();
  });
});
