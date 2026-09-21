import { useEffect, useRef } from "react";
import "./TaskDialogMotion.css";

/** Keep the native modal and its backdrop mounted until their exit motion ends. */
export function useTaskDialog(onClose: () => void) {
  const dialog = useRef<HTMLDialogElement>(null);
  const pendingClose = useRef<(() => void) | null>(null);

  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    element.dataset.taskDialog = "";
    if (element.showModal) element.showModal();
    else element.setAttribute("open", "");
    element.querySelector<HTMLElement>("[data-autofocus]")?.focus();
    return () => {
      pendingClose.current = null;
      element.close?.();
    };
  }, []);

  function close(afterClose = onClose) {
    const element = dialog.current;
    if (!element || pendingClose.current) return;
    pendingClose.current = afterClose;
    element.dataset.closing = "true";
    element.inert = true;

    // Include the backdrop, but never wait for spinners or other child animations.
    const animations = (element.getAnimations?.({ subtree: true }) ?? [])
      .filter((animation) => (animation.effect as KeyframeEffect | null)?.target === element);
    const finish = () => {
      if (pendingClose.current !== afterClose) return;
      pendingClose.current = null;
      element.close?.();
      afterClose();
    };
    // Reduced motion (or unavailable animations) closes without an artificial delay.
    if (animations.length === 0) finish();
    else void Promise.allSettled(animations.map((animation) => animation.finished)).then(finish);
  }

  return { dialog, close };
}
