import { useLayoutEffect, useRef, useState, type ComponentPropsWithoutRef, type MouseEvent } from "react";
import "./disclosure-motion.css";
import { animateDisclosure } from "./disclosure-motion";

/** Native details semantics, including toggle events and keyboard activation. */
export function AnimatedDetails({ open, onClick, children, ...props }: ComponentPropsWithoutRef<"details">) {
  const ref = useRef<HTMLDetailsElement>(null);
  const [initialOpen] = useState(open);
  const target = useRef(Boolean(open));
  const stop = useRef<(() => void) | null>(null);
  const frame = useRef<number | null>(null);
  const mounted = useRef(false);

  function cancel() {
    if (frame.current !== null) cancelAnimationFrame(frame.current);
    frame.current = null;
    stop.current?.(); stop.current = null;
  }
  function move(expanded: boolean) {
    const element = ref.current;
    if (!element) return;
    const start = element.getBoundingClientRect().height;
    cancel();
    target.current = expanded;
    if (!element.animate || window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
      element.open = expanded; return;
    }
    // Measure the closed height using its actual padding, borders and summary margins.
    element.open = false;
    const collapsed = element.getBoundingClientRect().height;
    element.open = true;
    // Let onToggle consumers render lazily loaded content before measuring the target.
    frame.current = requestAnimationFrame(() => {
      frame.current = null;
      const full = element.getBoundingClientRect().height;
      const finish = () => { element.open = expanded; stop.current = null; };
      if (Math.abs(full - collapsed) < 1) {
        // Overflow menus have absolutely positioned contents; never clip their popup.
        const content = Array.from(element.children).find((child) => child.tagName !== "SUMMARY") as HTMLElement | undefined;
        stop.current = content ? animateDisclosure(content, 0, 0, finish, true, expanded) : null;
        if (!content) finish();
      } else stop.current = animateDisclosure(element, start, expanded ? full : collapsed, finish);
    });
  }
  useLayoutEffect(() => {
    if (mounted.current) move(Boolean(open));
    else mounted.current = true;
    // Only a changed open prop requests a controlled transition; user toggles stay native.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);
  useLayoutEffect(() => () => cancel(), []);

  function click(event: MouseEvent<HTMLDetailsElement>) {
    onClick?.(event);
    if (event.defaultPrevented || !(event.target instanceof Element)) return;
    const summary = event.target.closest("summary");
    if (!summary || summary.parentElement !== ref.current) return;
    if (event.target.closest("a, button, input, select, textarea")) return;
    event.preventDefault();
    move(stop.current || frame.current !== null ? !target.current : !event.currentTarget.open);
  }
  return <details {...props} ref={ref} open={initialOpen} data-disclosure-motion="" onClick={click}>{children}</details>;
}
