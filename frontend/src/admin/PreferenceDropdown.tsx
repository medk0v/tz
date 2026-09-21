import { AnimatedDisclosure } from "../AnimatedDisclosure";
import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { Check, ChevronDown, Search } from "lucide-react";
import "./PreferenceDropdown.css";

export interface PreferenceDropdownOption<T extends string> {
  value: T;
  label: string;
  lang?: string;
  /** Two colors (base, accent) rendered as a small preview swatch. */
  swatch?: readonly [string, string];
  /** One nesting level, drawn as an indent: a subproject under its parent. */
  depth?: 0 | 1;
}

interface PreferenceDropdownProps<T extends string> {
  label: string;
  options: ReadonlyArray<PreferenceDropdownOption<T>>;
  value: T;
  onChange: (value: T) => void;
  disabled?: boolean;
  /** Stands in for the value while it has no option yet, such as a project list that is still loading. */
  placeholder?: string;
  /** Id of the text that explains the choice, such as why it cannot be changed. */
  describedBy?: string;
  /** Adds a visible filter for long lists. */
  search?: { placeholder: string; emptyLabel: string };
  /** Open above the field when the viewport or enclosing dialog has less room below. */
  placement?: "bottom" | "auto";
}

function OptionContent<T extends string>({ option }: { option: PreferenceDropdownOption<T> }) {
  return <span className="preference-dropdown-value">
    {option.swatch && <span
      className="preference-dropdown-swatch"
      aria-hidden="true"
      style={{ "--swatch-base": option.swatch[0], "--swatch-accent": option.swatch[1] } as CSSProperties}
    />}
    <span lang={option.lang}>{option.label}</span>
  </span>;
}

export function PreferenceDropdown<T extends string>({ label, options, value, onChange, disabled = false, placeholder, describedBy, search, placement = "bottom" }: PreferenceDropdownProps<T>) {
  const id = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const typeahead = useRef({ text: "", time: 0 });
  const searchRef = useRef<HTMLInputElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [above, setAbove] = useState(false);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const selectedIndex = options.findIndex((option) => option.value === value);
  const shown = options[selectedIndex] ?? (placeholder === undefined ? undefined : { value, label: placeholder });
  const expanded = open && !disabled && options.length > 0;
  const matches = search ? options.filter((option) => option.label.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())) : options;
  const active = Math.min(activeIndex, matches.length - 1);

  useLayoutEffect(() => {
    if (!expanded || placement !== "auto" || !triggerRef.current || !menuRef.current) return;
    const trigger = triggerRef.current.getBoundingClientRect();
    const dialog = rootRef.current?.closest("dialog")?.getBoundingClientRect();
    const top = Math.max(0, dialog?.top ?? 0) + 8;
    const bottom = Math.min(window.innerHeight, dialog?.bottom ?? window.innerHeight) - 8;
    const below = bottom - trigger.bottom - 4;
    setAbove(below < menuRef.current.getBoundingClientRect().height && trigger.top - top - 4 > below);
  }, [expanded, placement]);

  useEffect(() => {
    if (!expanded) return;
    searchRef.current?.focus();
    const closeOutside = (event: PointerEvent) => {
      if (event.target instanceof Node && !rootRef.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", closeOutside);
    return () => document.removeEventListener("pointerdown", closeOutside);
  }, [expanded]);

  /* A long list scrolls, so the keyboard cannot walk past the edge of the menu. */
  useEffect(() => {
    if (expanded) document.getElementById(`${id}-${active}`)?.scrollIntoView?.({ block: "nearest" });
  }, [active, expanded, id, query]);

  function choose(index: number) {
    const option = matches[index];
    if (!option) return;
    setOpen(false);
    triggerRef.current?.focus();
    if (option && option.value !== value) onChange(option.value);
  }

  function reveal() {
    setQuery("");
    setActiveIndex(Math.max(0, selectedIndex));
    setOpen(true);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLButtonElement | HTMLInputElement>) {
    if (event.nativeEvent.isComposing) return;
    const inSearch = event.currentTarget === searchRef.current;
    if (event.key === "Escape" && expanded) {
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
      triggerRef.current?.focus();
      return;
    }
    if (event.key === "Tab") {
      if (!inSearch) setOpen(false);
      return;
    }
    if (event.key === "Enter" || (event.key === " " && !inSearch)) {
      event.preventDefault();
      if (expanded) choose(active);
      else reveal();
      return;
    }
    let nextIndex: number | undefined;
    const start = expanded ? active : Math.max(0, selectedIndex);
    const count = expanded ? matches.length : options.length;
    if (event.key === "ArrowDown" && count) nextIndex = expanded ? (start + 1) % count : start;
    if (event.key === "ArrowUp" && count) nextIndex = expanded ? (start + count - 1) % count : start;
    if (event.key === "Home" && !inSearch) nextIndex = 0;
    if (event.key === "End" && !inSearch) nextIndex = count - 1;
    if (!inSearch && event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
      /* Letters typed in one breath search for a whole name, not for each letter in turn. */
      const now = Date.now();
      const text = (now - typeahead.current.time < 700 ? typeahead.current.text : "") + event.key.toLocaleLowerCase();
      typeahead.current = { text, time: now };
      const match = options.findIndex((option) => option.label.toLocaleLowerCase().startsWith(text));
      if (match !== -1) nextIndex = match;
    }
    if (nextIndex !== undefined) {
      event.preventDefault();
      if (!expanded) setQuery("");
      setActiveIndex(nextIndex);
      setOpen(true);
    }
  }

  return <div
    ref={rootRef}
    className="preference-dropdown"
    data-preferences-dropdown-open={expanded}
    onBlur={(event) => {
      if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setOpen(false);
    }}
  >
    <button
      ref={triggerRef}
      type="button"
      role={search ? undefined : "combobox"}
      className="preference-dropdown-trigger"
      /* The chosen value, for anything that reads the control rather than its label. */
      value={value}
      aria-label={label}
      aria-haspopup="listbox"
      aria-expanded={expanded}
      aria-controls={expanded ? `${id}-options` : undefined}
      aria-activedescendant={expanded && !search && active >= 0 ? `${id}-${active}` : undefined}
      aria-describedby={describedBy}
      disabled={disabled || options.length === 0}
      title={shown?.label}
      onClick={() => expanded ? setOpen(false) : reveal()}
      onKeyDown={handleKeyDown}
    >
      {shown && <OptionContent option={shown} />}
      <ChevronDown size={15} aria-hidden="true" />
    </button>
    <AnimatedDisclosure open={expanded} motion="fade" elementRef={menuRef} data-placement={placement === "auto" && above ? "top" : "bottom"}
      className={`preference-dropdown-options${search ? " preference-dropdown-options--searchable" : ""}`}>
      {search && <div className="preference-dropdown-search">
        <Search size={15} aria-hidden="true" />
        <input ref={searchRef} type="search" role="combobox" value={query}
          aria-label={search.placeholder} placeholder={search.placeholder} aria-expanded="true" aria-autocomplete="list"
          aria-controls={`${id}-options`} aria-activedescendant={active >= 0 ? `${id}-${active}` : undefined}
          onChange={(event) => { setQuery(event.target.value); setActiveIndex(0); }} onKeyDown={handleKeyDown} />
      </div>}
      <div id={`${id}-options`} role="listbox" aria-label={label} className="preference-dropdown-results">
      {matches.map((option, index) => <button
        key={option.value}
        id={`${id}-${index}`}
        type="button"
        role="option"
        aria-selected={value === option.value}
        data-depth={option.depth}
        tabIndex={-1}
        className={`preference-dropdown-option${active === index ? " preference-dropdown-option--active" : ""}`}
        onPointerMove={() => setActiveIndex(index)}
        onPointerDown={(event) => event.preventDefault()}
        onClick={() => choose(index)}
      >
        <OptionContent option={option} />
        {value === option.value && <Check size={15} aria-hidden="true" />}
      </button>)}
      </div>
      {search && matches.length === 0 && <p className="preference-dropdown-empty" role="status">{search.emptyLabel}</p>}
    </AnimatedDisclosure>
  </div>;
}
