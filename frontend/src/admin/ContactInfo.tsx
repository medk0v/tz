import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Info } from "lucide-react";

export function ContactInfo({ label, text }: { label: string; text: string }) {
  const id = useId();
  const timer = useRef<number | undefined>(undefined);
  const [position, setPosition] = useState<{ top: number; right: number } | null>(null);
  const cancelClose = () => window.clearTimeout(timer.current);
  const close = () => { cancelClose(); timer.current = window.setTimeout(() => setPosition(null), 120); };
  useEffect(() => () => window.clearTimeout(timer.current), []);
  useEffect(() => {
    if (!position) return;
    const dismiss = (event: Event) => {
      if (event.target instanceof Element && event.target.closest(".contact-info-tooltip")) return;
      setPosition(null);
    };
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") setPosition(null); };
    window.addEventListener("keydown", onKey);
    window.addEventListener("scroll", dismiss, true);
    window.addEventListener("resize", dismiss);
    return () => { window.removeEventListener("keydown", onKey); window.removeEventListener("scroll", dismiss, true); window.removeEventListener("resize", dismiss); };
  }, [position]);
  function show(button: HTMLButtonElement) {
    cancelClose();
    const rect = button.getBoundingClientRect();
    setPosition({ top: Math.max(12, Math.min(rect.top, window.innerHeight - 340)), right: Math.max(12, window.innerWidth - rect.left + 4) });
  }
  return <>
    <button type="button" className="icon-button" aria-label={label} aria-describedby={position ? id : undefined}
      onMouseEnter={(event) => show(event.currentTarget)} onMouseLeave={close}
      onFocus={(event) => show(event.currentTarget)} onBlur={close}
      onClick={(event) => show(event.currentTarget)}
      onKeyDown={(event) => { if (event.key === "Escape") { cancelClose(); setPosition(null); } }}>
      <Info size={17} aria-hidden="true" />
    </button>
    {position && createPortal(<div id={id} role="tooltip" className="contact-info-tooltip" style={position} onMouseEnter={cancelClose} onMouseLeave={close}>{text}</div>, document.body)}
  </>;
}
