import type { AiModelType } from "./api";

/** Agents, tasks, and generators can only use chat connections. */
export function isChatModelConnection(provider: { model_type?: AiModelType }): boolean {
  return (provider.model_type ?? "chat") === "chat";
}
