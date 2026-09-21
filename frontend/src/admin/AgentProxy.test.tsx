import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createI18n, I18nContext } from "../i18n";
import * as api from "../agent-proxy-api";
import { AgentProxy } from "./AgentProxy";

vi.mock("../agent-proxy-api", () => ({ getAgentProxy: vi.fn(), saveAgentProxy: vi.fn() }));
const auth = { kind: "session" } as const;
const configured: api.AgentProxySettings = { enabled: true, service_url: "https://proxy.example/api/proxies/random", region: null, country: null, token_configured: true };
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.getAgentProxy).mockResolvedValue(configured);
  vi.mocked(api.saveAgentProxy).mockImplementation(async (_auth, _profile, input) => ({ ...input, token_configured: !input.clear_token }));
});
afterEach(cleanup);
function view() {
  const onSubmit = vi.fn(event => event.preventDefault());
  render(<I18nContext.Provider value={createI18n("en", vi.fn())}><form onSubmit={onSubmit}><AgentProxy auth={auth} profileId="agent" /></form></I18nContext.Provider>);
  return onSubmit;
}
it("enables an unconfigured proxy through its label and saves the checked state", async () => {
  vi.mocked(api.getAgentProxy).mockResolvedValue({ enabled: false, service_url: "", region: null, country: null, token_configured: false });
  const onSubmit = view();
  const enabled = await screen.findByRole("checkbox", { name: "Connect a proxy" });
  expect(enabled).not.toBeChecked();
  fireEvent.click(screen.getByText("Connect a proxy"));
  expect(enabled).toBeChecked();
  fireEvent.change(screen.getByLabelText("Proxy discovery URL", { exact: false }), { target: { value: configured.service_url } });
  fireEvent.change(screen.getByLabelText("Bearer token", { exact: false }), { target: { value: "new-token" } });
  expect(enabled).toBeChecked();
  fireEvent.click(screen.getByRole("button", { name: "Save proxy" }));
  await screen.findByText("Proxy settings saved");
  expect(enabled).toBeChecked();
  expect(api.saveAgentProxy).toHaveBeenCalledWith(auth, "agent", { enabled: true, service_url: configured.service_url, region: null, country: null, token: "new-token", clear_token: false });
  expect(onSubmit).not.toHaveBeenCalled();
});
it("saves country without resending the stored token or submitting the agent form", async () => {
  const onSubmit = view();
  await screen.findByText("Token saved. Leave blank to keep it.");
  fireEvent.change(screen.getByLabelText("Proxy selection"), { target: { value: "country" } });
  fireEvent.change(screen.getByLabelText("Country code"), { target: { value: "de" } });
  fireEvent.click(screen.getByRole("button", { name: "Save proxy" }));
  await screen.findByText("Proxy settings saved");
  expect(api.saveAgentProxy).toHaveBeenCalledWith(auth, "agent", { enabled: true, service_url: configured.service_url, region: null, country: "DE", clear_token: false });
  expect(onSubmit).not.toHaveBeenCalled();
});
it("requires a new token for a changed destination and clears the input after saving", async () => {
  view(); await screen.findByText("Token saved. Leave blank to keep it.");
  fireEvent.change(screen.getByLabelText("Proxy discovery URL", { exact: false }), { target: { value: "https://new.example/random" } });
  fireEvent.click(screen.getByRole("button", { name: "Save proxy" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Enter a new token");
  expect(api.saveAgentProxy).not.toHaveBeenCalled();
  fireEvent.change(screen.getByLabelText("Bearer token", { exact: false }), { target: { value: "new-token" } });
  fireEvent.click(screen.getByRole("button", { name: "Save proxy" }));
  await screen.findByText("Proxy settings saved");
  expect(screen.getByLabelText("Bearer token", { exact: false })).toHaveValue("");
});
it("disables proxy when removing its saved token", async () => {
  view(); await screen.findByText("Token saved. Leave blank to keep it.");
  fireEvent.click(screen.getByLabelText("Remove saved token"));
  expect(screen.getByLabelText("Connect a proxy")).not.toBeChecked();
  fireEvent.click(screen.getByRole("button", { name: "Save proxy" }));
  await waitFor(() => expect(api.saveAgentProxy).toHaveBeenCalledWith(auth, "agent", expect.objectContaining({ enabled: false, clear_token: true })));
});
