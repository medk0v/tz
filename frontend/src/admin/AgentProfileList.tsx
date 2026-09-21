import { useState } from "react";
import { Bot, Box, ChartNoAxesCombined, ChevronRight, CodeXml, Headset, Landmark, Megaphone, Network, Plus, Scale, Search, ShieldCheck, Users } from "lucide-react";
import type { AiProfile } from "../api";
import { useI18n } from "../i18n";
import { OperatorAvatar } from "./OperatorAvatar";
import "./AgentProfileList.css";

const presetIcons = { coordinator: Network, sales: ChartNoAxesCombined, marketing: Megaphone, finance: Landmark, legal: Scale, programmer: CodeXml, support: Headset, operations: Network, quality: ShieldCheck, hr: Users, universal: Box };

export function AgentProfileMark({ profile }: { profile: AiProfile }) {
  const key = profile.preset_key;
  if (!key || profile.avatar_url) return <OperatorAvatar displayName={profile.name} avatarUrl={profile.avatar_url} />;
  return <PresetMark presetKey={key} />;
}

function PresetMark({ presetKey }: { presetKey: NonNullable<AiProfile["preset_key"]> }) {
  const Icon = presetIcons[presetKey];
  return <span className={`agent-preset-mark agent-preset-mark--${presetKey}`} aria-hidden="true"><Icon size={22} strokeWidth={1.8} /></span>;
}

interface Props {
  profiles: AiProfile[];
  selectedId: string | null;
  loading: boolean;
  disabled: boolean;
  onSelect: (profile: AiProfile) => void;
  onCreate: () => void;
}

export function AgentProfileList({ profiles, selectedId, loading, disabled, onSelect, onCreate }: Props) {
  const { t } = useI18n();
  const [search, setSearch] = useState("");
  const query = search.trim().toLocaleLowerCase();
  const matches = (item: { name: string; description?: string }) => `${item.name} ${item.description ?? ""}`.toLocaleLowerCase().includes(query);
  const custom = profiles.filter(matches);
  const matching = custom;

  return (
    <aside className="settings-list agent-profile-list" aria-label={t("ai.profileList")}>
      <label className="agent-profile-search">
        <Search size={17} aria-hidden="true" />
        <input type="search" aria-label={t("ai.profileSearch")} placeholder={t("ai.profileSearch")} value={search} onChange={(event) => setSearch(event.target.value)} />
      </label>
      <section className="agent-profile-group" aria-label={t("ai.customAgents")}>
        <h2>{t("ai.customAgents")}</h2>
        <div>
          {custom.map((profile) => (
            <button
              className={`agent-profile-row${selectedId === profile.id ? " agent-profile-row--active" : ""}${profile.preset_key ? ` agent-profile-row--${profile.preset_key}` : ""}`}
              type="button"
              key={profile.id}
              aria-pressed={selectedId === profile.id}
              disabled={disabled}
              onClick={() => onSelect(profile)}
            >
              <AgentProfileMark profile={profile} />
              <span className="agent-profile-row-copy">
                <strong>{profile.name}</strong>
                {profile.description && <span className="agent-profile-row-description">{profile.description}</span>}
                <span className={`agent-profile-row-status agent-profile-row-status--${profile.status}`}>
                  {t(profile.status === "active" ? "ai.statusActive" : profile.status === "disabled" ? "ai.statusDisabled" : "ai.statusDraft")}
                  {" · "}{t("ai.customAgent")}
                </span>
              </span>
              <ChevronRight size={16} aria-hidden="true" />
            </button>
          ))}
          {!loading && custom.length === 0 && !query && <p className="agent-profile-list-empty">{t("ai.customAgentsEmpty")}</p>}
        </div>
        <button className="agent-profile-create" type="button" disabled={disabled} onClick={onCreate}><Plus size={16} />{t("ai.createCustomAgent")}</button>
      </section>
      {loading && <div className="empty-state" role="status">{t("ai.loading")}</div>}
      {!loading && query && matching.length === 0 && <p className="agent-profile-list-empty" role="status">{t("ai.profileSearchEmpty")}</p>}
      {!loading && profiles.length === 0 && !query && <div className="empty-state"><Bot size={20} />{t("ai.profileEmpty")}</div>}
    </aside>
  );
}
