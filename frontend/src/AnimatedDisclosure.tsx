import { useCallback, useLayoutEffect, useRef, useState, type ComponentPropsWithoutRef, type RefObject } from "react";
import "./disclosure-motion.css";
import { animateDisclosure } from "./disclosure-motion";

/** Keep closing content mounted for its exit, but immediately remove it from keyboard access. */
export function AnimatedDisclosure({ open, keepMounted = false, motion = "height", elementRef, children, ...props }: ComponentPropsWithoutRef<"div"> & {
  open: boolean; keepMounted?: boolean; motion?: "height" | "fade"; elementRef?: RefObject<HTMLDivElement | null>;
}) {
  const [present, setPresent] = useState(open);
  const [previousOpen, setPreviousOpen] = useState(open);
  const ref = useRef<HTMLDivElement>(null);
  const setRef = useCallback((node: HTMLDivElement | null) => { ref.current = node; if (elementRef) elementRef.current = node; }, [elementRef]);
  const stop = useRef<(() => void) | null>(null);
  const wasOpen = useRef(false);
  if (previousOpen !== open) { setPreviousOpen(open); if (open) setPresent(true); }
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const start = wasOpen.current || stop.current ? element.getBoundingClientRect().height : 0;
    stop.current?.(); stop.current = null;
    wasOpen.current = open;
    if (!open && !present) return;
    const end = open ? element.getBoundingClientRect().height : 0;
    stop.current = animateDisclosure(element, start, end, () => {
      stop.current = null;
      if (!open) setPresent(false);
    }, motion === "fade", open);
    // Presence only controls final unmounting; the transition is driven by open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, motion]);
  useLayoutEffect(() => () => { stop.current?.(); }, []);
  if (!open && !present && !keepMounted) return null;
  return <div {...props} ref={setRef} data-disclosure-motion="" hidden={!open && !present} aria-hidden={!open || undefined} inert={!open}>
    {children}
  </div>;
}
