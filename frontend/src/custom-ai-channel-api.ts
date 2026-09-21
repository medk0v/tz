import type { ResourceVisibility } from "./resource-visibility";
import { managementApiRequest, type OperatorAuth } from "./api";

export interface CustomAiChannelInput {
  visibility?: ResourceVisibility;
  name: string;
  icon: string;
  inbox_id: string;
  connection_type: "ai_agent" | "external_api";
  destination: "conversations" | "tasks" | "custom";
  mode: "instructions" | "agent";
  source_url: string;
  source_kind: "website" | "api";
  instructions: string;
  ai_profile_id: string | null;
}

export interface CustomAiChannel extends CustomAiChannelInput {
  inbox_name?: string;
  id: string;
  status: "draft";
  created_at: string;
  updated_at: string;
}

const path = "/api/v1/custom-ai-channels";

export async function listCustomAiChannels(auth: OperatorAuth): Promise<CustomAiChannel[]> {
  const response = await managementApiRequest<{ items: CustomAiChannel[] }>(auth, path);
  return response.items;
}

export function createCustomAiChannel(auth: OperatorAuth, input: CustomAiChannelInput): Promise<CustomAiChannel> {
  return managementApiRequest(auth, path, { method: "POST", body: JSON.stringify(input) });
}

export function updateCustomAiChannel(auth: OperatorAuth, id: string, input: CustomAiChannelInput): Promise<CustomAiChannel> {
  return managementApiRequest(auth, `${path}/${encodeURIComponent(id)}`, { method: "PUT", body: JSON.stringify(input) });
}

export function deleteCustomAiChannel(auth: OperatorAuth, id: string): Promise<void> {
  return managementApiRequest(auth, `${path}/${encodeURIComponent(id)}`, { method: "DELETE" });
}
