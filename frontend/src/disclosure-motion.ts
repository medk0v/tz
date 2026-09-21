export const DISCLOSURE_DURATION = 220;

/** Temporary layout animation; restore automatic height and visible overflow afterward. */
export function animateDisclosure(
  element: HTMLElement,
  from: number,
  to: number,
  finish: () => void,
  fadeOnly = false,
  opening = true,
): (() => void) | null {
  const preference = window.matchMedia?.("(prefers-reduced-motion: reduce)");
  if (!element.animate || preference?.matches) { finish(); return null; }
  const overflow = element.style.overflow;
  const boxSizing = element.style.boxSizing;
  if (!fadeOnly) { element.style.overflow = "hidden"; element.style.boxSizing = "border-box"; }
  const keyframes: Keyframe[] = fadeOnly
    ? [{ opacity: opening ? 0 : 1 }, { opacity: opening ? 1 : 0 }]
    : [{ height: `${from}px` }, { height: `${to}px` }];
  if (!fadeOnly && (from === 0 || to === 0)) {
    const style = getComputedStyle(element);
    // Padded panels must reach zero too; otherwise their padding snaps away at unmount.
    for (const property of ["paddingTop", "paddingBottom", "marginTop", "marginBottom", "borderTopWidth", "borderBottomWidth"] as const) {
      keyframes[0][property] = from === 0 ? "0px" : style[property];
      keyframes[1][property] = to === 0 ? "0px" : style[property];
    }
  }
  const animation = element.animate(keyframes,
    { duration: DISCLOSURE_DURATION, easing: "cubic-bezier(.2, .75, .25, 1)", fill: "both" });
  let active = true;
  function clear() {
    active = false;
    preference?.removeEventListener("change", complete);
    animation.cancel();
    element.style.overflow = overflow;
    element.style.boxSizing = boxSizing;
  }
  function complete() { if (active) { clear(); finish(); } }
  preference?.addEventListener("change", complete, { once: true });
  void animation.finished.then(complete, () => { if (active) clear(); });
  return clear;
}
