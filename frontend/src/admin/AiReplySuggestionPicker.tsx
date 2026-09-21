import { AnimatedDisclosure } from "../AnimatedDisclosure";
import { DemoActionButton } from "./DemoReadOnly";
import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Bot, ChevronDown, RefreshCw } from "lucide-react";
import type { ReplySuggestionAgent } from "../api";
import { useI18n } from "../i18n";
import type { AiDraftSource } from "./ai-draft-sources";
import { OperatorAvatar } from "./OperatorAvatar";
import "./AiReplySuggestionPicker.css";

export interface AiReplySuggestionPickerProps {
  source: AiDraftSource;
  draftBody: string;
  disabled: boolean;
  onInsert: (body: string) => boolean;
  triggerClassName?: string;
}

interface MenuPosition {
  left: number;
  top?: number;
  bottom?: number;
}

const menuWidth = 280;

/** Lets a person choose an agent that drafts text for the current editor. */
export function AiReplySuggestionPicker({
  source,
  draftBody,
  disabled,
  onInsert,
  triggerClassName,
}: AiReplySuggestionPickerProps) {
  const { t } = useI18n();
  const { target: draftTarget, text } = source;
  // Generation finishes after later renders, so it uses the current title and editor.
  const latest = useRef({ source, onInsert });
  const menuId = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const generationControllerRef = useRef<AbortController | null>(null);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<MenuPosition | null>(null);
  const [agents, setAgents] = useState<ReplySuggestionAgent[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [generatingAgentId, setGeneratingAgentId] = useState<string | null>(null);
  const [generationError, setGenerationError] = useState(false);
  const [tooLong, setTooLong] = useState(false);
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    latest.current = { source, onInsert };
  });

  function close(restoreFocus = true) {
    setOpen(false);
    if (restoreFocus) triggerRef.current?.focus();
  }

  function showMenu() {
    const rect = triggerRef.current?.getBoundingClientRect();
    if (!rect) return;
    const opensUpward = window.innerHeight - rect.bottom < 220 && rect.top > 220;
    setPosition({
      left: Math.max(8, Math.min(rect.right - menuWidth, window.innerWidth - menuWidth - 8)),
      ...(opensUpward
        ? { bottom: Math.max(8, window.innerHeight - rect.top + 6) }
        : { top: Math.max(8, rect.bottom + 6) }),
    });
    setLoading(true);
    setLoadError(false);
    setGenerationError(false);
    setTooLong(false);
    setOpen(true);
  }

  useEffect(() => {
    if (!open || disabled) return undefined;
    const controller = new AbortController();
    latest.current.source.listAgents(controller.signal)
      .then((items) => {
        if (!controller.signal.aborted) setAgents(items);
      })
      .catch(() => {
        if (!controller.signal.aborted) setLoadError(true);
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, [draftTarget, disabled, open, revision]);

  useEffect(() => {
    if (!open) return undefined;
    function handlePointerDown(event: PointerEvent) {
      const target = event.target as Node;
      if (!menuRef.current?.contains(target) && !triggerRef.current?.contains(target)) {
        setOpen(false);
          }
    }
    function handleViewportChange() {
      setOpen(false);
      }
    document.addEventListener("pointerdown", handlePointerDown);
    window.addEventListener("resize", handleViewportChange);
    window.addEventListener("scroll", handleViewportChange, true);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      window.removeEventListener("resize", handleViewportChange);
      window.removeEventListener("scroll", handleViewportChange, true);
    };
  }, [open]);

  useEffect(() => () => {
    generationControllerRef.current?.abort();
    generationControllerRef.current = null;
  }, [draftTarget]);

  useEffect(() => {
    if (!open || loading || loadError || agents.length === 0) return undefined;
    const frame = window.requestAnimationFrame(() => {
      menuRef.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [agents.length, loadError, loading, open]);

  async function generate(agent: ReplySuggestionAgent) {
    if (generatingAgentId) return;
    const controller = new AbortController();
    generationControllerRef.current = controller;
    setGeneratingAgentId(agent.id);
    setGenerationError(false);
    setTooLong(false);
    try {
      const body = await latest.current.source.generate(agent.id, draftBody, controller.signal);
      if (controller.signal.aborted) return;
      if (latest.current.onInsert(body)) close(false);
      else setTooLong(true);
    } catch {
      if (!controller.signal.aborted) setGenerationError(true);
    } finally {
      if (generationControllerRef.current === controller) {
        generationControllerRef.current = null;
        setGeneratingAgentId(null);
      }
    }
  }

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className={triggerClassName ? `ai-reply-suggestion-trigger ${triggerClassName}` : "ai-reply-suggestion-trigger"}
        disabled={disabled}
        aria-haspopup="menu"
        aria-expanded={open && !disabled}
        aria-controls={open ? menuId : undefined}
        title={t(text.choose)}
        onClick={() => open ? close(false) : showMenu()}
      >
        {generatingAgentId
          ? <RefreshCw className="spin" size={16} aria-hidden="true" />
          : <Bot size={16} aria-hidden="true" />}
        <span>{generatingAgentId ? t("aiReplySuggestion.generating") : t(text.button)}</span>
        <ChevronDown size={13} aria-hidden="true" />
      </button>
      {position && createPortal(
        <AnimatedDisclosure open={open && !disabled} motion="fade"
          elementRef={menuRef}
          id={menuId}
          className="ai-reply-suggestion-menu"
          role="menu"
          aria-label={t(text.choose)}
          style={position}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              close();
            }
          }}
        >
          <p className="ai-reply-suggestion-menu-title">{t("aiReplySuggestion.selectAgent")}</p>
          {generationError && <p className="message-editor-error" role="alert">{t(text.generateError)}</p>}
          {tooLong && <p className="message-editor-error" role="alert">{t(text.insertTooLong)}</p>}
          {loading ? (
            <p role="status" className="ai-reply-suggestion-status">{t("aiReplySuggestion.loading")}</p>
          ) : loadError ? (
            <div className="ai-reply-suggestion-status" role="alert">
              <p>{t("aiReplySuggestion.loadError")}</p>
              <button type="button" className="secondary-button" onClick={() => {
                setLoading(true);
                setLoadError(false);
                setRevision((value) => value + 1);
              }}>
                {t("aiReplySuggestion.retry")}
              </button>
            </div>
          ) : agents.length === 0 ? (
            <p role="status" className="ai-reply-suggestion-status">{t(text.empty)}</p>
          ) : (
            <ul>
              {agents.map((agent) => (
                <li key={agent.id}>
                  <DemoActionButton
                    type="button"
                    role="menuitem"
                    disabled={generatingAgentId !== null}
                    onClick={() => void generate(agent)}
                  >
                    <OperatorAvatar displayName={agent.name} avatarUrl={agent.avatar_url} />
                    <span>{agent.name}</span>
                    {generatingAgentId === agent.id && <RefreshCw className="spin" size={15} aria-hidden="true" />}
                  </DemoActionButton>
                </li>
              ))}
            </ul>
          )}
        </AnimatedDisclosure>,
        document.body,
      )}
    </>
  );
}
