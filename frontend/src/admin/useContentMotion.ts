import { useCallback, useRef } from "react";

/** Animate selection changes without remounting forms or disturbing focus. */
export function useContentMotion<T extends HTMLElement>(selection: string) {
  const stopCurrent = useRef<(() => void) | null>(null);

  // A callback ref also runs when an async/conditional panel mounts after its
  // selection was set. An effect depending only on selection misses that case.
  return useCallback((element: T | null) => {
    stopCurrent.current?.();
    stopCurrent.current = null;
    if (!element) return;
    element.dataset.contentMotion = selection;
    if (!element.animate) return;
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    if (preference.matches) return;

    const animation = element.animate(
      [{ opacity: 0, transform: "translateY(12px)" }, { opacity: 1, transform: "none" }],
      { duration: 340, easing: "cubic-bezier(.2, .75, .25, 1)" },
    );
    const stop = () => {
      preference.removeEventListener("change", stop);
      animation.cancel();
    };
    stopCurrent.current = stop;
    preference.addEventListener("change", stop, { once: true });
    void animation.finished.then(stop, () => preference.removeEventListener("change", stop));
  }, [selection]);
}
