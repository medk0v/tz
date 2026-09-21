import { useEffect, useId, useRef, useState } from "react";
import { ChevronDown, Search } from "lucide-react";
import { AnimatedDisclosure } from "../AnimatedDisclosure";
import { useI18n } from "../i18n";
import { projectHierarchyText } from "./project-hierarchy-i18n";
import "./PreferenceDropdown.css";

interface Props {
  projects: readonly { id: string; name: string }[];
  parentId: string;
  selectedIds: readonly string[];
  onChange: (ids: string[]) => void;
}

export function ProjectScopePicker({ projects, parentId, selectedIds, onChange }: Props) {
  const { locale } = useI18n();
  const text = projectHierarchyText(locale);
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const allCheckbox = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const allSelected = selectedIds.length === projects.length;
  const summary = allSelected ? text("currentAndSubprojects") : selectedIds.length === 1
    ? selectedIds[0] === parentId ? text("currentProject") : projects.find((project) => project.id === selectedIds[0])!.name
    : text(selectedIds.length ? "selectedProjects" : "selectProjects", { count: String(selectedIds.length) });
  const matches = projects.filter((project) => `${project.name} ${project.id === parentId ? text("currentProject") : ""}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));

  useEffect(() => {
    if (!open) return;
    search.current?.focus();
    const closeOutside = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", closeOutside);
    return () => document.removeEventListener("pointerdown", closeOutside);
  }, [open]);
  useEffect(() => {
    if (allCheckbox.current) allCheckbox.current.indeterminate = selectedIds.length > 0 && !allSelected;
  }, [open, selectedIds.length, allSelected]);

  return <div ref={root} className="preference-dropdown project-scope-picker" data-preferences-dropdown-open={open}
    onBlur={(event) => {
      // Safari can blur the search with no next focus target when clicking a label.
      // Outside pointer presses are handled separately, before focus changes.
      if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) setOpen(false);
    }}
    onKeyDown={(event) => {
      if (event.key === "Escape" && open) {
        event.preventDefault(); event.stopPropagation(); setOpen(false); trigger.current?.focus();
      }
    }}>
    <button ref={trigger} type="button" className="preference-dropdown-trigger" aria-label={text("dataScope")}
      aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? id : undefined} title={summary}
      onClick={() => { setQuery(""); setOpen(!open); }}>
      <span className="preference-dropdown-value"><span>{summary}</span></span>
      <ChevronDown size={15} aria-hidden="true" />
    </button>
    <AnimatedDisclosure open={open} motion="fade" id={id} role="dialog" aria-label={text("dataScope")}
      className="preference-dropdown-options preference-dropdown-options--searchable">
      <div className="preference-dropdown-search">
        <Search size={15} aria-hidden="true" />
        <input ref={search} type="search" value={query} aria-label={text("searchProjects")} placeholder={text("searchProjects")}
          onChange={(event) => setQuery(event.target.value)} />
      </div>
      <label className="preference-dropdown-option project-scope-all">
        <input ref={allCheckbox} type="checkbox" checked={allSelected}
          onChange={(event) => {
            event.currentTarget.indeterminate = selectedIds.length > 0 && !allSelected;
            onChange(allSelected ? [] : projects.map((project) => project.id));
          }} />
        <span>{text("currentAndSubprojects")}</span>
      </label>
      <div className="preference-dropdown-results">
        {matches.map((project) => <label key={project.id} className="preference-dropdown-option">
          <input type="checkbox" checked={selectedIds.includes(project.id)} onChange={(event) => onChange(event.target.checked
            ? projects.filter((item) => item.id === project.id || selectedIds.includes(item.id)).map((item) => item.id)
            : selectedIds.filter((id) => id !== project.id))} />
          <span>{project.id === parentId ? text("currentProject") : project.name}</span>
        </label>)}
      </div>
      {!matches.length && <p className="preference-dropdown-empty" role="status">{text("noProjectsFound")}</p>}
    </AnimatedDisclosure>
  </div>;
}
