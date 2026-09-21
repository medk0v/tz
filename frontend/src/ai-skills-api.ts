import { managementApiRequest, type OperatorAuth } from "./api";

export interface AiSkillInput {
  name: string;
  description: string;
  instructions: string;
  ai_profile_ids: string[];
}

export interface AiSkill extends AiSkillInput {
  id: string;
  created_at: string;
  updated_at: string;
}

export interface AiSkillGenerationInput {
  provider_connection_id: string;
  goal: string;
  source_text: string;
  files: File[];
}

export type AiSkillGenerationResult = Pick<AiSkillInput, "name" | "description" | "instructions">;

export interface AiProfileSkillAssignment {
  profileId: string;
  skillIds: string[];
}

const path = "/api/v1/ai/skills";

export async function listAiSkills(auth: OperatorAuth, signal?: AbortSignal): Promise<AiSkill[]> {
  const response = await managementApiRequest<{ items: AiSkill[] }>(auth, path, { signal });
  return response.items;
}

export function createAiSkill(auth: OperatorAuth, input: AiSkillInput): Promise<AiSkill> {
  return managementApiRequest(auth, path, { method: "POST", body: JSON.stringify(input) });
}

export function updateAiSkill(auth: OperatorAuth, id: string, input: AiSkillInput): Promise<AiSkill> {
  return managementApiRequest(auth, `${path}/${encodeURIComponent(id)}`, { method: "PATCH", body: JSON.stringify(input) });
}

export function deleteAiSkill(auth: OperatorAuth, id: string): Promise<void> {
  return managementApiRequest(auth, `${path}/${encodeURIComponent(id)}`, { method: "DELETE" });
}

export function setAiProfileSkills(auth: OperatorAuth, profileId: string, skillIds: string[]): Promise<{ skill_ids: string[] }> {
  return managementApiRequest(auth, `/api/v1/ai/profiles/${encodeURIComponent(profileId)}/skills`, {
    method: "PUT", body: JSON.stringify({ skill_ids: skillIds }),
  });
}

export function generateAiSkill(auth: OperatorAuth, input: AiSkillGenerationInput, signal?: AbortSignal): Promise<AiSkillGenerationResult> {
  const body = new FormData();
  body.append("provider_connection_id", input.provider_connection_id);
  body.append("goal", input.goal);
  if (input.source_text.trim()) body.append("source_text", input.source_text);
  for (const file of input.files) body.append("file", file);
  return managementApiRequest(auth, `${path}/generate`, { method: "POST", body, signal });
}
