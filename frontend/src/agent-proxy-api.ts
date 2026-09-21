import { managementApiRequest, type OperatorAuth } from "./api";

export interface AgentProxySettings {
  enabled: boolean;
  service_url: string;
  region: string | null;
  country: string | null;
  token_configured: boolean;
}

export type AgentProxyInput = Omit<AgentProxySettings, "token_configured"> & {
  token?: string;
  clear_token?: boolean;
};

const path = (profileId: string) => `/api/v1/ai/profiles/${encodeURIComponent(profileId)}/proxy`;
export function getAgentProxy(auth: OperatorAuth, profileId: string, signal?: AbortSignal): Promise<AgentProxySettings> {
  return managementApiRequest(auth, path(profileId), { signal });
}
export function saveAgentProxy(auth: OperatorAuth, profileId: string, input: AgentProxyInput): Promise<AgentProxySettings> {
  return managementApiRequest(auth, path(profileId), { method: "PUT", body: JSON.stringify(input) });
}
